use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, header},
    routing::{get, patch, post},
};
use servcat_db::repositories::departments;
use servcat_model::{Department, NewDepartment, Role, UpdateDepartment};
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, state::AppState, uploads};

const MAX_LOGO_UPLOAD_BYTES: usize = 6 * 1024 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/departments", get(list).post(create))
        .route("/departments/{id}", patch(update).delete(delete))
        .route(
            "/departments/{id}/logo",
            post(upload_logo).delete(remove_logo),
        )
        .layer(DefaultBodyLimit::max(MAX_LOGO_UPLOAD_BYTES))
}

async fn list(
    _auth_user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<Department>>, ApiError> {
    Ok(Json(departments::list(&state.pool).await?))
}

async fn create(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Json(new_department): Json<NewDepartment>,
) -> Result<Json<Department>, ApiError> {
    auth_user.require_role(&[Role::Admin])?;
    Ok(Json(
        departments::create(&state.pool, &new_department).await?,
    ))
}

async fn update(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(patch): Json<UpdateDepartment>,
) -> Result<Json<Department>, ApiError> {
    auth_user.require_role(&[Role::Admin])?;
    let updated = departments::update(&state.pool, id, &patch)
        .await?
        .ok_or_else(|| ApiError::NotFound("department not found".into()))?;
    Ok(Json(updated))
}

async fn delete(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<(), ApiError> {
    auth_user.require_role(&[Role::Admin])?;
    departments::delete(&state.pool, id).await?;
    Ok(())
}

async fn upload_logo(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Department>, ApiError> {
    auth_user.require_role(&[Role::Admin])?;
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| ApiError::BadRequest("Content-Type header is required".into()))?;

    let current = departments::get_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::NotFound("department not found".into()))?;
    let logo_url = uploads::save_logo(
        &state.uploads_dir,
        "departments",
        &id.to_string(),
        content_type,
        body,
        current.logo_url.as_deref(),
    )
    .await?;

    let updated = departments::set_logo_url(&state.pool, id, Some(&logo_url))
        .await?
        .ok_or_else(|| ApiError::NotFound("department not found".into()))?;
    Ok(Json(updated))
}

async fn remove_logo(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Department>, ApiError> {
    auth_user.require_role(&[Role::Admin])?;
    let current = departments::get_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::NotFound("department not found".into()))?;
    uploads::delete_logo(&state.uploads_dir, current.logo_url.as_deref()).await;

    let updated = departments::set_logo_url(&state.pool, id, None)
        .await?
        .ok_or_else(|| ApiError::NotFound("department not found".into()))?;
    Ok(Json(updated))
}
