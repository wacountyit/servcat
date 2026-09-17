use askama::Template;
use axum::{
    Form, Router,
    extract::{Path, State},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::Deserialize;
use servcat_db::repositories::departments;
use servcat_model::{Department, NewDepartment, UpdateDepartment};
use uuid::Uuid;

use crate::{
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
        .route("/admin/departments", get(list).post(create))
        .route("/admin/departments/{id}/update", post(update))
        .route("/admin/departments/{id}/delete", post(delete))
}

/// Askama template filter (`{{ expr|is_selected_parent(other) }}`): a plain
/// `d.parent_department_id == Some(p.id)` in the template won't type-check
/// (Askama passes call arguments by reference regardless of the field's
/// actual type, so that'd try to construct `Some(&Uuid)` against a
/// `Uuid`-typed variant); comparing through `Option<&Uuid>` on both sides
/// sidesteps that entirely.
mod filters {
    use uuid::Uuid;

    pub fn is_selected_parent(parent: &Option<Uuid>, candidate: &Uuid) -> askama::Result<bool> {
        Ok(parent.as_ref() == Some(candidate))
    }
}

#[derive(Template)]
#[template(path = "admin/departments.html")]
struct DepartmentsTemplate {
    layout: Layout,
    admin_section: &'static str,
    departments: Vec<Department>,
}

async fn list(State(state): State<AppState>, WebUser(user): WebUser) -> Result<Response, WebError> {
    require_admin(&user)?;
    let layout = Layout::load(&state, &user, "admin").await?;
    let all_departments = departments::list(&state.pool).await?;
    Ok(html(DepartmentsTemplate {
        layout,
        admin_section: "departments",
        departments: all_departments,
    }))
}

#[derive(Deserialize)]
struct CreateDepartmentForm {
    name: String,
    parent_department_id: Option<String>,
}

async fn create(
    State(state): State<AppState>,
    WebUser(user): WebUser,
    Form(form): Form<CreateDepartmentForm>,
) -> Result<Response, WebError> {
    require_admin(&user)?;
    let parent_department_id = form
        .parent_department_id
        .filter(|s| !s.is_empty())
        .and_then(|s| Uuid::parse_str(&s).ok());
    departments::create(
        &state.pool,
        &NewDepartment {
            name: form.name,
            parent_department_id,
        },
    )
    .await?;
    Ok(Redirect::to("/admin/departments").into_response())
}

#[derive(Deserialize)]
struct UpdateDepartmentForm {
    name: String,
    parent_department_id: Option<String>,
}

async fn update(
    State(state): State<AppState>,
    WebUser(user): WebUser,
    Path(id): Path<Uuid>,
    Form(form): Form<UpdateDepartmentForm>,
) -> Result<Response, WebError> {
    require_admin(&user)?;
    let parent_department_id = form
        .parent_department_id
        .filter(|s| !s.is_empty())
        .and_then(|s| Uuid::parse_str(&s).ok());
    departments::update(
        &state.pool,
        id,
        &UpdateDepartment {
            name: Some(form.name),
            parent_department_id,
        },
    )
    .await?
    .ok_or_else(|| ApiError::NotFound("department not found".into()))?;
    Ok(Redirect::to("/admin/departments").into_response())
}

async fn delete(
    State(state): State<AppState>,
    WebUser(user): WebUser,
    Path(id): Path<Uuid>,
) -> Result<Response, WebError> {
    require_admin(&user)?;
    departments::delete(&state.pool, id).await?;
    Ok(Redirect::to("/admin/departments").into_response())
}
