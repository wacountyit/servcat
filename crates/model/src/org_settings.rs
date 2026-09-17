use serde::{Deserialize, Serialize};

use crate::Timestamp;

/// Organization-wide branding and auth-policy settings, editable at runtime
/// by an admin (`PATCH /admin/settings`) rather than baked into a redeploy.
/// Always exactly one row (`id = 1` in the backing table).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct OrgSettings {
    pub app_name: String,
    pub logo_url: Option<String>,
    pub allow_local_signup: bool,
    /// IANA timezone name (e.g. `"America/Chicago"`) used only to render
    /// timestamps in the web UI and in notification emails -- every stored
    /// timestamp remains UTC regardless of this setting. Defaults to `"UTC"`.
    pub timezone: String,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct UpdateOrgSettings {
    pub app_name: Option<String>,
    pub allow_local_signup: Option<bool>,
    pub timezone: Option<String>,
}

/// Unauthenticated projection served to a browser/Tauri app that doesn't
/// have a session yet, so it can render the org's name/seal and decide
/// whether to show a local sign-up option before login.
#[derive(Debug, Clone, Serialize)]
pub struct PublicConfig {
    pub app_name: String,
    pub logo_url: Option<String>,
    pub allow_local_signup: bool,
    pub sso_enabled: bool,
}
