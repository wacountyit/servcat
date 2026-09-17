use askama::Template;
use axum::{
    Form, Router,
    extract::{Path, State},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::Deserialize;
use servcat_db::repositories::{audit, departments, users};
use servcat_model::{Department, NewAuditLogEntry, NewUser, Role, UpdateUser};
use uuid::Uuid;

use crate::{
    auth::hash_password,
    error::ApiError,
    state::AppState,
    web::{
        layout::Layout,
        render::{WebError, html},
        session::WebUser,
    },
};

use super::require_admin;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/admin/users", get(list).post(create))
        .route("/admin/users/{id}/update", post(update))
        .route("/admin/users/{id}/deactivate", post(deactivate))
}

struct UserRow {
    id: Uuid,
    email: String,
    display_name: String,
    role: Role,
    department_id: Option<Uuid>,
    is_active: bool,
}

#[derive(Template)]
#[template(path = "admin/users.html")]
struct UsersTemplate {
    layout: Layout,
    admin_section: &'static str,
    users: Vec<UserRow>,
    departments: Vec<Department>,
    current_user_id: Uuid,
}

async fn list(State(state): State<AppState>, WebUser(user): WebUser) -> Result<Response, WebError> {
    require_admin(&user)?;
    let layout = Layout::load(&state, &user, "admin").await?;
    let all_users = users::list(&state.pool, true)
        .await?
        .into_iter()
        .map(|u| UserRow {
            id: u.id,
            email: u.email,
            display_name: u.display_name,
            role: u.role,
            department_id: u.department_id,
            is_active: u.is_active,
        })
        .collect();
    let all_departments = departments::list(&state.pool).await?;
    Ok(html(UsersTemplate {
        layout,
        admin_section: "users",
        users: all_users,
        departments: all_departments,
        current_user_id: user.id,
    }))
}

#[derive(Deserialize)]
struct CreateUserForm {
    email: String,
    display_name: String,
    password: String,
    role: String,
}

async fn create(
    State(state): State<AppState>,
    WebUser(user): WebUser,
    Form(form): Form<CreateUserForm>,
) -> Result<Response, WebError> {
    require_admin(&user)?;
    let role: Role = form
        .role
        .parse()
        .map_err(|_| WebError(ApiError::BadRequest("invalid role".into())))?;
    let password_hash = hash_password(&form.password)?;

    let created = users::create(
        &state.pool,
        &NewUser {
            email: form.email,
            display_name: form.display_name,
            password: None,
            external_idp_subject: None,
            department_id: None,
            manager_user_id: None,
            role,
        },
        Some(&password_hash),
    )
    .await?;

    audit::record(
        &state.pool,
        &NewAuditLogEntry {
            actor_user_id: Some(user.id),
            action: "user.create".into(),
            entity_type: "user".into(),
            entity_id: Some(created.id.to_string()),
            metadata_json: Some(
                serde_json::json!({ "email": created.email, "role": created.role }),
            ),
        },
    )
    .await?;

    Ok(Redirect::to("/admin/users").into_response())
}

#[derive(Deserialize)]
struct UpdateUserForm {
    role: String,
    department_id: Option<String>,
    manager_user_id: Option<String>,
    is_active: Option<String>,
}

async fn update(
    State(state): State<AppState>,
    WebUser(user): WebUser,
    Path(id): Path<Uuid>,
    Form(form): Form<UpdateUserForm>,
) -> Result<Response, WebError> {
    require_admin(&user)?;
    let role: Role = form
        .role
        .parse()
        .map_err(|_| WebError(ApiError::BadRequest("invalid role".into())))?;
    let department_id = form
        .department_id
        .filter(|s| !s.is_empty())
        .and_then(|s| Uuid::parse_str(&s).ok());
    let manager_user_id = form
        .manager_user_id
        .filter(|s| !s.is_empty())
        .and_then(|s| Uuid::parse_str(&s).ok());
    let is_active = form.is_active.as_deref() == Some("true");

    let updated = users::update(
        &state.pool,
        id,
        &UpdateUser {
            display_name: None,
            department_id,
            manager_user_id,
            role: Some(role),
            is_active: Some(is_active),
        },
    )
    .await?
    .ok_or_else(|| ApiError::NotFound("user not found".into()))?;

    audit::record(
        &state.pool,
        &NewAuditLogEntry {
            actor_user_id: Some(user.id),
            action: "user.update".into(),
            entity_type: "user".into(),
            entity_id: Some(updated.id.to_string()),
            metadata_json: None,
        },
    )
    .await?;

    Ok(Redirect::to("/admin/users").into_response())
}

async fn deactivate(
    State(state): State<AppState>,
    WebUser(user): WebUser,
    Path(id): Path<Uuid>,
) -> Result<Response, WebError> {
    require_admin(&user)?;
    if user.id == id {
        return Err(WebError(ApiError::BadRequest(
            "you cannot deactivate your own account".into(),
        )));
    }

    users::deactivate(&state.pool, id).await?;
    crate::auth::revoke_all_sessions(&state.pool, id).await?;

    audit::record(
        &state.pool,
        &NewAuditLogEntry {
            actor_user_id: Some(user.id),
            action: "user.deactivate".into(),
            entity_type: "user".into(),
            entity_id: Some(id.to_string()),
            metadata_json: None,
        },
    )
    .await?;

    Ok(Redirect::to("/admin/users").into_response())
}
