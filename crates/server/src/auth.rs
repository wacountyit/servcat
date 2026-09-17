use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use axum::{RequestPartsExt, extract::FromRequestParts, http::request::Parts};
use axum_extra::TypedHeader;
use axum_extra::headers::{Authorization, authorization::Bearer};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use servcat_db::{Pool, repositories::users};
use servcat_model::{Role, User, UserProfile};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{error::ApiError, state::AppState};

/// Response shape for every route that hands out a fresh session: local
/// login/refresh (`routes/auth.rs`) and the SSO handoff-code exchange
/// (`routes/sso.rs`).
#[derive(Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub user: UserProfile,
}

pub async fn issue_token_pair(state: &AppState, user: &User) -> Result<(String, String), ApiError> {
    let access_token = issue_access_token(&state.config, user)?;
    let refresh_token = create_session(&state.pool, &state.config, user.id, None, None).await?;
    Ok((access_token, refresh_token))
}

/// Local email/password authentication, shared by the JSON API's
/// `POST /auth/login` and the web UI's `POST /login`. Returns the same
/// generic `Unauthorized` whether the email doesn't exist, has no local
/// password (SSO-only), is deactivated, or the password is wrong -- avoids
/// confirming to a caller which emails have accounts.
pub async fn authenticate_local(
    pool: &Pool,
    email: &str,
    password: &str,
) -> Result<User, ApiError> {
    let user = users::get_by_email(pool, email)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    if !user.is_active {
        return Err(ApiError::Unauthorized);
    }
    let Some(password_hash) = &user.password_hash else {
        return Err(ApiError::Unauthorized);
    };
    if !verify_password(password, password_hash)? {
        return Err(ApiError::Unauthorized);
    }
    Ok(user)
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: Uuid,
    role: Role,
    exp: i64,
    iat: i64,
}

pub fn hash_password(password: &str) -> Result<String, ApiError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|_| ApiError::Internal)
}

pub fn verify_password(password: &str, hash: &str) -> Result<bool, ApiError> {
    let parsed = PasswordHash::new(hash).map_err(|_| ApiError::Internal)?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

pub fn issue_access_token(
    config: &crate::config::AppConfig,
    user: &User,
) -> Result<String, ApiError> {
    let now = Utc::now();
    let claims = Claims {
        sub: user.id,
        role: user.role,
        iat: now.timestamp(),
        exp: (now + ChronoDuration::from_std(config.access_token_ttl).unwrap()).timestamp(),
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(config.jwt_signing_secret.as_bytes()),
    )
    .map_err(|_| ApiError::Internal)
}

pub(crate) fn verify_access_token(
    config: &crate::config::AppConfig,
    token: &str,
) -> Result<(Uuid, Role), ApiError> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(config.jwt_signing_secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| ApiError::Unauthorized)?;
    Ok((data.claims.sub, data.claims.role))
}

fn hash_refresh_token(raw: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    hex::encode(hasher.finalize())
}

/// Generates a new opaque refresh token, stores only its hash in `sessions`,
/// and returns the raw token to hand to the client -- the raw value never
/// touches the database or logs.
pub async fn create_session(
    pool: &Pool,
    config: &crate::config::AppConfig,
    user_id: Uuid,
    user_agent: Option<&str>,
    ip_address: Option<&str>,
) -> Result<String, ApiError> {
    let mut raw_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw_bytes);
    let raw_token = hex::encode(raw_bytes);
    let token_hash = hash_refresh_token(&raw_token);
    let expires_at: DateTime<Utc> =
        Utc::now() + ChronoDuration::from_std(config.refresh_token_ttl).unwrap();

    sqlx::query(
        "INSERT INTO sessions (id, user_id, refresh_token_hash, user_agent, ip_address, expires_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(&token_hash)
    .bind(user_agent)
    .bind(ip_address)
    .bind(expires_at)
    .execute(pool)
    .await
    .map_err(servcat_db::DbError::from)?;

    Ok(raw_token)
}

/// Validates a raw refresh token against `sessions`, then revokes it --
/// refresh tokens are single-use; the caller must issue a fresh one via
/// `create_session` alongside a new access token (rotation limits the blast
/// radius of a leaked refresh token to a single use).
pub async fn redeem_refresh_token(pool: &Pool, raw_token: &str) -> Result<Uuid, ApiError> {
    let token_hash = hash_refresh_token(raw_token);

    let row: Option<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT id, user_id FROM sessions \
         WHERE refresh_token_hash = ? AND revoked_at IS NULL AND expires_at > CURRENT_TIMESTAMP",
    )
    .bind(&token_hash)
    .fetch_optional(pool)
    .await
    .map_err(servcat_db::DbError::from)?;

    let (session_id, user_id) = row.ok_or(ApiError::Unauthorized)?;

    sqlx::query("UPDATE sessions SET revoked_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(session_id)
        .execute(pool)
        .await
        .map_err(servcat_db::DbError::from)?;

    Ok(user_id)
}

pub async fn revoke_all_sessions(pool: &Pool, user_id: Uuid) -> Result<(), ApiError> {
    sqlx::query("UPDATE sessions SET revoked_at = CURRENT_TIMESTAMP WHERE user_id = ? AND revoked_at IS NULL")
        .bind(user_id)
        .execute(pool)
        .await
        .map_err(servcat_db::DbError::from)?;
    Ok(())
}

/// Authenticated-user extractor: validates the bearer JWT's signature and
/// expiry, then re-checks `is_active` against the database on every request
/// so a deactivated account's still-unexpired access token stops working
/// immediately rather than waiting out its (short) TTL.
pub struct AuthUser(pub User);

impl AuthUser {
    pub fn require_role(&self, allowed: &[Role]) -> Result<(), ApiError> {
        if allowed.contains(&self.0.role) {
            Ok(())
        } else {
            Err(ApiError::Forbidden)
        }
    }
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let TypedHeader(Authorization(bearer)) = parts
            .extract::<TypedHeader<Authorization<Bearer>>>()
            .await
            .map_err(|_| ApiError::Unauthorized)?;

        let (user_id, _role) = verify_access_token(&state.config, bearer.token())?;

        let user = users::get_by_id(&state.pool, user_id)
            .await
            .map_err(ApiError::from)?
            .filter(|u| u.is_active)
            .ok_or(ApiError::Unauthorized)?;

        Ok(AuthUser(user))
    }
}
