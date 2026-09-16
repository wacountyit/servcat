use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::{header, HeaderMap},
    routing::{get, patch, post},
    Json, Router,
};
use servcat_db::repositories::org_settings;
use servcat_model::{OrgSettings, PublicConfig, Role, UpdateOrgSettings};

use crate::{auth::AuthUser, error::ApiError, state::AppState, uploads};

const MAX_LOGO_UPLOAD_BYTES: usize = 6 * 1024 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new().route("/config", get(public_config)).merge(
        Router::new()
            .route("/admin/settings", patch(update_settings))
            .route("/admin/settings/logo", post(upload_logo).delete(remove_logo))
            .layer(DefaultBodyLimit::max(MAX_LOGO_UPLOAD_BYTES)),
    )
}

async fn public_config(State(state): State<AppState>) -> Result<Json<PublicConfig>, ApiError> {
    let settings = org_settings::get(&state.pool).await?;
    Ok(Json(PublicConfig {
        app_name: settings.app_name,
        logo_url: settings.logo_url,
        allow_local_signup: settings.allow_local_signup,
        sso_enabled: state.sso.is_some(),
    }))
}

async fn update_settings(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Json(patch): Json<UpdateOrgSettings>,
) -> Result<Json<OrgSettings>, ApiError> {
    auth_user.require_role(&[Role::Admin])?;
    Ok(Json(org_settings::update(&state.pool, &patch).await?))
}

fn content_type(headers: &HeaderMap) -> Result<&str, ApiError> {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| ApiError::BadRequest("Content-Type header is required".into()))
}

async fn upload_logo(
    auth_user: AuthUser,
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<OrgSettings>, ApiError> {
    auth_user.require_role(&[Role::Admin])?;
    let content_type = content_type(&headers)?;

    let current = org_settings::get(&state.pool).await?;
    let logo_url = uploads::save_logo(&state.uploads_dir, "org", "org", content_type, body, current.logo_url.as_deref()).await?;
    Ok(Json(org_settings::set_logo_url(&state.pool, Some(&logo_url)).await?))
}

async fn remove_logo(auth_user: AuthUser, State(state): State<AppState>) -> Result<Json<OrgSettings>, ApiError> {
    auth_user.require_role(&[Role::Admin])?;
    let current = org_settings::get(&state.pool).await?;
    uploads::delete_logo(&state.uploads_dir, current.logo_url.as_deref()).await;
    Ok(Json(org_settings::set_logo_url(&state.pool, None).await?))
}
