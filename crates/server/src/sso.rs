//! Microsoft Entra ID (Azure AD) login via the OAuth2 authorization-code +
//! PKCE flow. This server is the confidential client (it holds
//! `client_secret`), so the SPA/Tauri frontend never sees an Entra ID token
//! directly -- after validating the ID token and provisioning/linking the
//! local `User` row, we hand the frontend a short-lived, single-use code of
//! our own to exchange for a normal access/refresh JWT pair (see
//! `routes/sso.rs`). That keeps the handoff identical on the frontend
//! whether a session started via SSO or local login.

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{config::AzureSsoConfig, error::ApiError};

const PENDING_LOGIN_TTL: Duration = Duration::from_secs(10 * 60);
const HANDOFF_CODE_TTL: Duration = Duration::from_secs(60);
/// Entra ID rotates its signing keys infrequently; re-fetching at most this
/// often avoids hammering the discovery endpoint on every login while still
/// picking up a rotation within an hour.
const JWKS_CACHE_TTL: Duration = Duration::from_secs(3600);

struct PendingLogin {
    pkce_verifier: String,
    nonce: String,
    created_at: Instant,
}

struct HandoffCode {
    user_id: Uuid,
    created_at: Instant,
}

#[derive(Clone, Deserialize)]
struct Jwk {
    kid: String,
    n: String,
    e: String,
}

#[derive(Deserialize)]
struct JwkSet {
    keys: Vec<Jwk>,
}

#[derive(Deserialize)]
struct TokenResponse {
    id_token: String,
}

#[derive(Deserialize)]
struct IdTokenClaims {
    oid: String,
    nonce: Option<String>,
    email: Option<String>,
    preferred_username: Option<String>,
    name: Option<String>,
}

/// The caller's identity as asserted by Entra ID, after full signature and
/// claim validation.
pub struct AuthenticatedIdentity {
    pub external_idp_subject: String,
    pub email: String,
    pub display_name: String,
}

pub struct SsoService {
    config: AzureSsoConfig,
    http: reqwest::Client,
    pending_logins: Mutex<HashMap<String, PendingLogin>>,
    handoff_codes: Mutex<HashMap<String, HandoffCode>>,
    jwks_cache: Mutex<Option<(Instant, Vec<Jwk>)>>,
}

impl SsoService {
    pub fn new(config: AzureSsoConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
            pending_logins: Mutex::new(HashMap::new()),
            handoff_codes: Mutex::new(HashMap::new()),
            jwks_cache: Mutex::new(None),
        }
    }

    fn authorize_endpoint(&self) -> String {
        format!(
            "https://login.microsoftonline.com/{}/oauth2/v2.0/authorize",
            self.config.tenant_id
        )
    }

    fn token_endpoint(&self) -> String {
        format!(
            "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
            self.config.tenant_id
        )
    }

    fn jwks_endpoint(&self) -> String {
        format!(
            "https://login.microsoftonline.com/{}/discovery/v2.0/keys",
            self.config.tenant_id
        )
    }

    fn expected_issuer(&self) -> String {
        format!(
            "https://login.microsoftonline.com/{}/v2.0",
            self.config.tenant_id
        )
    }

    pub fn frontend_redirect_url(&self) -> &str {
        &self.config.frontend_redirect_url
    }

    /// Builds the URL to send the browser to, recording the PKCE verifier
    /// and nonce (keyed by the CSRF `state`) so `complete_login` can
    /// validate the callback actually belongs to this login attempt.
    pub fn begin_login(&self) -> String {
        let state = random_token(32);
        let nonce = random_token(32);
        let pkce_verifier = random_token(64);
        let pkce_challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(pkce_verifier.as_bytes()));

        {
            let mut pending_logins = self.pending_logins.lock().unwrap();
            sweep_expired(&mut pending_logins, PENDING_LOGIN_TTL, |p| p.created_at);
            pending_logins.insert(
                state.clone(),
                PendingLogin {
                    pkce_verifier,
                    nonce: nonce.clone(),
                    created_at: Instant::now(),
                },
            );
        }

        let mut url =
            url::Url::parse(&self.authorize_endpoint()).expect("static authorize URL is valid");
        url.query_pairs_mut()
            .append_pair("client_id", &self.config.client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", &self.config.redirect_uri)
            .append_pair("response_mode", "query")
            .append_pair("scope", "openid profile email")
            .append_pair("state", &state)
            .append_pair("nonce", &nonce)
            .append_pair("code_challenge", &pkce_challenge)
            .append_pair("code_challenge_method", "S256");
        url.into()
    }

    /// Exchanges the authorization `code` for an ID token, validates it
    /// (signature, issuer, audience, expiry, and that its nonce matches this
    /// `state`'s pending login), and returns the caller's identity.
    pub async fn complete_login(
        &self,
        code: &str,
        state: &str,
    ) -> Result<AuthenticatedIdentity, ApiError> {
        let pending = {
            let mut pending_logins = self.pending_logins.lock().unwrap();
            sweep_expired(&mut pending_logins, PENDING_LOGIN_TTL, |p| p.created_at);
            pending_logins.remove(state).ok_or_else(|| {
                ApiError::BadRequest("sign-in session expired or invalid; please try again".into())
            })?
        };

        let params = [
            ("client_id", self.config.client_id.as_str()),
            ("client_secret", self.config.client_secret.as_str()),
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", self.config.redirect_uri.as_str()),
            ("code_verifier", pending.pkce_verifier.as_str()),
        ];

        let response = self
            .http
            .post(self.token_endpoint())
            .form(&params)
            .send()
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Entra ID token exchange request failed");
                ApiError::Internal
            })?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            tracing::error!(%body, "Entra ID rejected the token exchange");
            return Err(ApiError::Unauthorized);
        }

        let token_response: TokenResponse = response.json().await.map_err(|e| {
            tracing::error!(error = %e, "Entra ID token response was not the expected shape");
            ApiError::Internal
        })?;

        let claims = self.validate_id_token(&token_response.id_token).await?;

        if claims.nonce.as_deref() != Some(pending.nonce.as_str()) {
            tracing::error!("Entra ID token nonce did not match this login attempt");
            return Err(ApiError::Unauthorized);
        }

        let email = claims.email.or(claims.preferred_username).ok_or_else(|| {
            tracing::error!("Entra ID token had neither an email nor a preferred_username claim");
            ApiError::Internal
        })?;

        Ok(AuthenticatedIdentity {
            external_idp_subject: claims.oid,
            display_name: claims.name.unwrap_or_else(|| email.clone()),
            email,
        })
    }

    async fn validate_id_token(&self, id_token: &str) -> Result<IdTokenClaims, ApiError> {
        let header = decode_header(id_token).map_err(|_| ApiError::Unauthorized)?;
        let kid = header.kid.ok_or(ApiError::Unauthorized)?;

        let decoding_key = self.signing_key(&kid).await?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&[self.config.client_id.as_str()]);
        validation.set_issuer(&[self.expected_issuer()]);

        let data = decode::<IdTokenClaims>(id_token, &decoding_key, &validation).map_err(|e| {
            tracing::error!(error = %e, "Entra ID token signature/claim validation failed");
            ApiError::Unauthorized
        })?;
        Ok(data.claims)
    }

    async fn signing_key(&self, kid: &str) -> Result<DecodingKey, ApiError> {
        if let Some(jwk) = self.cached_jwk(kid) {
            return DecodingKey::from_rsa_components(&jwk.n, &jwk.e)
                .map_err(|_| ApiError::Internal);
        }
        // Not found (or cache stale/empty) -- refresh once in case Entra ID
        // rotated its keys, then give up if it's genuinely not there.
        self.refresh_jwks().await?;
        let jwk = self.cached_jwk(kid).ok_or_else(|| {
            tracing::error!(%kid, "no matching Entra ID signing key found, even after refreshing");
            ApiError::Unauthorized
        })?;
        DecodingKey::from_rsa_components(&jwk.n, &jwk.e).map_err(|_| ApiError::Internal)
    }

    fn cached_jwk(&self, kid: &str) -> Option<Jwk> {
        let cache = self.jwks_cache.lock().unwrap();
        let (fetched_at, keys) = cache.as_ref()?;
        if fetched_at.elapsed() > JWKS_CACHE_TTL {
            return None;
        }
        keys.iter().find(|k| k.kid == kid).cloned()
    }

    async fn refresh_jwks(&self) -> Result<(), ApiError> {
        let jwk_set: JwkSet = self
            .http
            .get(self.jwks_endpoint())
            .send()
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "failed to fetch Entra ID signing keys");
                ApiError::Internal
            })?
            .json()
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Entra ID JWKS response was not the expected shape");
                ApiError::Internal
            })?;

        *self.jwks_cache.lock().unwrap() = Some((Instant::now(), jwk_set.keys));
        Ok(())
    }

    /// Issues a single-use handoff code the frontend exchanges for a real
    /// token pair via `POST /auth/sso/token` -- keeps Entra ID's tokens (and
    /// ours) out of the browser's address bar/history.
    pub fn issue_handoff_code(&self, user_id: Uuid) -> String {
        let code = random_token(32);
        let mut codes = self.handoff_codes.lock().unwrap();
        sweep_expired(&mut codes, HANDOFF_CODE_TTL, |c| c.created_at);
        codes.insert(
            code.clone(),
            HandoffCode {
                user_id,
                created_at: Instant::now(),
            },
        );
        code
    }

    pub fn redeem_handoff_code(&self, code: &str) -> Result<Uuid, ApiError> {
        let mut codes = self.handoff_codes.lock().unwrap();
        sweep_expired(&mut codes, HANDOFF_CODE_TTL, |c| c.created_at);
        codes
            .remove(code)
            .map(|c| c.user_id)
            .ok_or(ApiError::Unauthorized)
    }
}

fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

fn sweep_expired<K, V>(map: &mut HashMap<K, V>, ttl: Duration, created_at: impl Fn(&V) -> Instant)
where
    K: std::hash::Hash + Eq,
{
    map.retain(|_, v| created_at(v).elapsed() < ttl);
}
