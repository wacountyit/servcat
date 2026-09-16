use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, patch, post},
};
use serde::Deserialize;
use serde_json::json;
use servcat_db::repositories::{audit, users};
use servcat_model::{NewAuditLogEntry, NewUser, Role, UpdateUser, UserProfile};
use uuid::Uuid;

use crate::{
    auth::{AuthUser, hash_password},
    error::ApiError,
    state::AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/users/me", get(me))
        .route("/users", get(list_users).post(create_user))
        .route("/users/{id}", patch(update_user))
        .route("/users/{id}/deactivate", post(deactivate_user))
}

async fn me(AuthUser(user): AuthUser) -> Json<UserProfile> {
    Json(user.into())
}

#[derive(Deserialize)]
struct ListUsersQuery {
    #[serde(default)]
    include_inactive: bool,
}

async fn list_users(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Query(query): Query<ListUsersQuery>,
) -> Result<Json<Vec<UserProfile>>, ApiError> {
    auth_user.require_role(&[Role::Admin])?;
    let users = users::list(&state.pool, query.include_inactive).await?;
    Ok(Json(users.into_iter().map(UserProfile::from).collect()))
}

async fn create_user(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Json(new_user): Json<NewUser>,
) -> Result<Json<UserProfile>, ApiError> {
    auth_user.require_role(&[Role::Admin])?;

    if new_user.password.is_none() && new_user.external_idp_subject.is_none() {
        return Err(ApiError::BadRequest(
            "a new user needs either a password or an external_idp_subject".into(),
        ));
    }
    let password_hash = new_user
        .password
        .as_deref()
        .map(hash_password)
        .transpose()?;

    let created = users::create(&state.pool, &new_user, password_hash.as_deref()).await?;

    audit::record(
        &state.pool,
        &NewAuditLogEntry {
            actor_user_id: Some(auth_user.0.id),
            action: "user.create".into(),
            entity_type: "user".into(),
            entity_id: Some(created.id.to_string()),
            metadata_json: Some(json!({ "email": created.email, "role": created.role })),
        },
    )
    .await?;

    Ok(Json(created.into()))
}

async fn update_user(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(patch): Json<UpdateUser>,
) -> Result<Json<UserProfile>, ApiError> {
    auth_user.require_role(&[Role::Admin])?;

    let updated = users::update(&state.pool, id, &patch)
        .await?
        .ok_or_else(|| ApiError::NotFound("user not found".into()))?;

    audit::record(
        &state.pool,
        &NewAuditLogEntry {
            actor_user_id: Some(auth_user.0.id),
            action: "user.update".into(),
            entity_type: "user".into(),
            entity_id: Some(id.to_string()),
            metadata_json: None,
        },
    )
    .await?;

    Ok(Json(updated.into()))
}

async fn deactivate_user(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<(), ApiError> {
    auth_user.require_role(&[Role::Admin])?;
    if auth_user.0.id == id {
        return Err(ApiError::BadRequest(
            "you cannot deactivate your own account".into(),
        ));
    }

    users::deactivate(&state.pool, id).await?;
    crate::auth::revoke_all_sessions(&state.pool, id).await?;

    audit::record(
        &state.pool,
        &NewAuditLogEntry {
            actor_user_id: Some(auth_user.0.id),
            action: "user.deactivate".into(),
            entity_type: "user".into(),
            entity_id: Some(id.to_string()),
            metadata_json: None,
        },
    )
    .await?;

    Ok(())
}
