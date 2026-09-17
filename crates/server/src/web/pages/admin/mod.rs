mod catalog;
mod departments;
mod settings;
mod users;
mod workflows;

use axum::Router;
use servcat_model::{Role, User};

use crate::{error::ApiError, state::AppState, web::render::WebError};

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(users::routes())
        .merge(departments::routes())
        .merge(settings::routes())
        .merge(workflows::routes())
        .merge(catalog::routes())
}

/// Every handler in `admin::*` calls this first -- there's no router-level
/// role gate because the web UI's session middleware only establishes *who*
/// is signed in, not what they're allowed to do (same division of
/// responsibility as the JSON API's `AuthUser::require_role`).
fn require_admin(user: &User) -> Result<(), WebError> {
    if user.role == Role::Admin {
        Ok(())
    } else {
        Err(WebError(ApiError::Forbidden))
    }
}
