//! The header/nav chrome every page template shares. Every leaf template
//! embeds a `Layout` field and `base.html` reads through it (`{{
//! layout.app_name }}` etc.) -- Askama has no separate "master page data"
//! concept, so composition is how every page gets the org name/logo and
//! current user without repeating itself.

use servcat_db::repositories::org_settings;
use servcat_model::{Role, User};

use crate::{error::ApiError, state::AppState};

pub struct Layout {
    pub app_name: String,
    pub logo_url: Option<String>,
    pub user_display_name: String,
    pub user_role_label: &'static str,
    pub is_admin: bool,
    pub active_nav: &'static str,
}

impl Layout {
    pub async fn load(
        state: &AppState,
        user: &User,
        active_nav: &'static str,
    ) -> Result<Self, ApiError> {
        let settings = org_settings::get(&state.pool).await?;
        Ok(Self {
            app_name: settings.app_name,
            logo_url: settings.logo_url,
            user_display_name: user.display_name.clone(),
            user_role_label: user.role.label(),
            is_admin: user.role == Role::Admin,
            active_nav,
        })
    }
}
