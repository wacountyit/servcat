use std::{env, net::SocketAddr, path::PathBuf, time::Duration};

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub database_url: String,
    pub bind_addr: SocketAddr,
    pub jwt_signing_secret: String,
    pub access_token_ttl: Duration,
    pub refresh_token_ttl: Duration,
    pub cors_allowed_origins: Vec<String>,
    pub bootstrap_admin_email: Option<String>,
    pub bootstrap_admin_password: Option<String>,
    /// Directory uploaded org/department seals are written to and served
    /// from (see `routes::build_router`'s `/uploads` route).
    pub uploads_dir: PathBuf,
    /// Organization name to seed `org_settings` with on first startup only;
    /// change it afterwards via `PATCH /admin/settings` instead.
    pub bootstrap_app_name: Option<String>,
    /// Whether local email/password self-registration starts out enabled;
    /// only applied on first startup, same as `bootstrap_app_name`.
    pub bootstrap_allow_local_signup: bool,
    pub sso: Option<AzureSsoConfig>,
    pub connectors: ConnectorsConfig,
}

/// Microsoft Entra ID (Azure AD) OIDC configuration. Only enabled if every
/// field's env var is present -- see `AppConfig::from_env`.
#[derive(Debug, Clone)]
pub struct AzureSsoConfig {
    pub tenant_id: String,
    pub client_id: String,
    pub client_secret: String,
    /// This backend's own callback URL, registered in the Azure app
    /// registration as a redirect URI (e.g. `https://servcat.example.com/auth/sso/callback`).
    pub redirect_uri: String,
    /// Where to send the browser after a successful login, with a one-time
    /// handoff code appended as `?code=...` for the frontend to exchange at
    /// `POST /auth/sso/token`. Typically the frontend's own `/auth/sso/complete` route.
    pub frontend_redirect_url: String,
}

/// Credentials for outward-facing ticket connectors. Every field is
/// optional: a connector is only registered (and therefore usable by a
/// workflow's `target_system`) if its required env vars are all present.
#[derive(Debug, Clone, Default)]
pub struct ConnectorsConfig {
    pub webhook_url: Option<String>,
    pub webhook_bearer_token: Option<String>,
    pub jira_base_url: Option<String>,
    pub jira_email: Option<String>,
    pub jira_api_token: Option<String>,
    pub glpi_base_url: Option<String>,
    pub glpi_app_token: Option<String>,
    pub glpi_user_token: Option<String>,
}

fn env_var(key: &str) -> Option<String> {
    env::var(key).ok().filter(|v| !v.is_empty())
}

impl AppConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let database_url = env::var("DATABASE_URL").map_err(|_| anyhow::anyhow!("DATABASE_URL must be set"))?;
        let bind_addr: SocketAddr = env::var("SERVER_BIND_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid SERVER_BIND_ADDR: {e}"))?;

        let jwt_signing_secret =
            env::var("JWT_SIGNING_SECRET").map_err(|_| anyhow::anyhow!("JWT_SIGNING_SECRET must be set"))?;
        if jwt_signing_secret.len() < 32 {
            anyhow::bail!("JWT_SIGNING_SECRET must be at least 32 characters");
        }

        let access_token_ttl = Duration::from_secs(
            env::var("JWT_ACCESS_TOKEN_TTL_SECONDS").ok().and_then(|v| v.parse().ok()).unwrap_or(900),
        );
        let refresh_token_ttl = Duration::from_secs(
            env::var("JWT_REFRESH_TOKEN_TTL_SECONDS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1_209_600),
        );

        let cors_allowed_origins = env::var("CORS_ALLOWED_ORIGINS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();

        let uploads_dir = PathBuf::from(env::var("UPLOADS_DIR").unwrap_or_else(|_| "data/uploads".to_string()));

        let bootstrap_allow_local_signup = env::var("ALLOW_LOCAL_SIGNUP")
            .ok()
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(false);

        let sso = match (
            env_var("AZURE_TENANT_ID"),
            env_var("AZURE_CLIENT_ID"),
            env_var("AZURE_CLIENT_SECRET"),
            env_var("AZURE_REDIRECT_URI"),
            env_var("SSO_FRONTEND_REDIRECT_URL"),
        ) {
            (Some(tenant_id), Some(client_id), Some(client_secret), Some(redirect_uri), Some(frontend_redirect_url)) => {
                Some(AzureSsoConfig { tenant_id, client_id, client_secret, redirect_uri, frontend_redirect_url })
            }
            (None, None, None, None, None) => None,
            _ => {
                tracing::warn!(
                    "Entra ID SSO env vars are only partially set (need AZURE_TENANT_ID, AZURE_CLIENT_ID, \
                     AZURE_CLIENT_SECRET, AZURE_REDIRECT_URI and SSO_FRONTEND_REDIRECT_URL all together) \
                     -- SSO login is disabled"
                );
                None
            }
        };

        Ok(Self {
            database_url,
            bind_addr,
            jwt_signing_secret,
            access_token_ttl,
            refresh_token_ttl,
            cors_allowed_origins,
            bootstrap_admin_email: env_var("BOOTSTRAP_ADMIN_EMAIL"),
            bootstrap_admin_password: env_var("BOOTSTRAP_ADMIN_PASSWORD"),
            uploads_dir,
            bootstrap_app_name: env_var("APP_NAME"),
            bootstrap_allow_local_signup,
            sso,
            connectors: ConnectorsConfig {
                webhook_url: env_var("WEBHOOK_URL"),
                webhook_bearer_token: env_var("WEBHOOK_BEARER_TOKEN"),
                jira_base_url: env_var("JIRA_BASE_URL"),
                jira_email: env_var("JIRA_EMAIL"),
                jira_api_token: env_var("JIRA_API_TOKEN"),
                glpi_base_url: env_var("GLPI_BASE_URL"),
                glpi_app_token: env_var("GLPI_APP_TOKEN"),
                glpi_user_token: env_var("GLPI_USER_TOKEN"),
            },
        })
    }
}
