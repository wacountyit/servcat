use std::collections::HashMap;

use askama::Template;
use axum::{
    Form, Router,
    extract::{Path, State},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::Deserialize;
use servcat_db::repositories::{users, workflow_definitions};
use servcat_model::{
    FieldMapping, NewWorkflowDefinition, TargetSystem, User, WorkflowDefinition, WorkflowGraph,
};

use crate::{
    state::AppState,
    web::{
        layout::Layout,
        render::{WebError, html},
        session::WebUser,
    },
};
use uuid::Uuid;

use super::require_admin;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/admin/workflows", get(list).post(create))
        .route("/admin/workflows/{id}/publish", post(publish))
        .route("/admin/workflows/{id}/unpublish", post(unpublish))
}

#[derive(Template)]
#[template(path = "admin/workflows.html")]
struct WorkflowsTemplate {
    layout: Layout,
    admin_section: &'static str,
    definitions: Vec<WorkflowDefinition>,
    /// For the visual builder's "specific person" approver picker
    /// (`ApproverResolution::Static`) -- active users only, same set an
    /// admin could otherwise only reference by pasting a raw user id.
    users: Vec<User>,
    error: Option<String>,
    /// Re-populates the builder with what was actually submitted after a
    /// validation error, rather than losing it -- empty on a fresh page
    /// load, in which case the builder's own JS falls back to a small
    /// built-in starter graph instead of an empty canvas.
    prefill_name: String,
    prefill_target_system: String,
    prefill_graph_json: String,
    prefill_field_mapping_json: String,
}

async fn list(State(state): State<AppState>, WebUser(user): WebUser) -> Result<Response, WebError> {
    require_admin(&user)?;
    let layout = Layout::load(&state, &user, "admin").await?;
    let definitions = workflow_definitions::list(&state.pool, false).await?;
    let users = users::list(&state.pool, false).await?;
    Ok(html(WorkflowsTemplate {
        layout,
        admin_section: "workflows",
        definitions,
        users,
        error: None,
        prefill_name: String::new(),
        prefill_target_system: String::new(),
        prefill_graph_json: String::new(),
        prefill_field_mapping_json: String::new(),
    }))
}

#[derive(Deserialize)]
struct CreateWorkflowForm {
    name: String,
    target_system: String,
    graph_json: String,
    field_mapping_json: String,
}

async fn create(
    State(state): State<AppState>,
    WebUser(user): WebUser,
    Form(form): Form<CreateWorkflowForm>,
) -> Result<Response, WebError> {
    require_admin(&user)?;

    let graph: WorkflowGraph = match serde_json::from_str(&form.graph_json) {
        Ok(g) => g,
        Err(err) => {
            return workflow_form_error(&state, &user, format!("graph JSON: {err}"), &form).await;
        }
    };
    let field_mapping = if form.field_mapping_json.trim().is_empty() {
        None
    } else {
        match serde_json::from_str::<HashMap<String, String>>(&form.field_mapping_json) {
            Ok(map) => Some(FieldMapping(map)),
            Err(err) => {
                return workflow_form_error(
                    &state,
                    &user,
                    format!("field mapping JSON: {err}"),
                    &form,
                )
                .await;
            }
        }
    };
    let target_system = if form.target_system.is_empty() {
        None
    } else {
        match form.target_system.parse::<TargetSystem>() {
            Ok(t) => Some(t),
            Err(err) => return workflow_form_error(&state, &user, err, &form).await,
        }
    };

    workflow_definitions::create(
        &state.pool,
        &NewWorkflowDefinition {
            name: form.name,
            graph,
            field_mapping,
            target_system,
        },
        user.id,
    )
    .await?;

    Ok(Redirect::to("/admin/workflows").into_response())
}

/// Re-renders the page with `error` shown and the builder re-populated from
/// exactly what was submitted, so a validation failure doesn't discard
/// several minutes of visually building out a graph.
async fn workflow_form_error(
    state: &AppState,
    user: &User,
    error: String,
    form: &CreateWorkflowForm,
) -> Result<Response, WebError> {
    let layout = Layout::load(state, user, "admin").await?;
    let definitions = workflow_definitions::list(&state.pool, false).await?;
    let users = users::list(&state.pool, false).await?;
    Ok(html(WorkflowsTemplate {
        layout,
        admin_section: "workflows",
        definitions,
        users,
        error: Some(error),
        prefill_name: form.name.clone(),
        prefill_target_system: form.target_system.clone(),
        prefill_graph_json: form.graph_json.clone(),
        prefill_field_mapping_json: form.field_mapping_json.clone(),
    }))
}

async fn publish(
    State(state): State<AppState>,
    WebUser(user): WebUser,
    Path(id): Path<Uuid>,
) -> Result<Response, WebError> {
    require_admin(&user)?;
    workflow_definitions::set_published(&state.pool, id, true).await?;
    Ok(Redirect::to("/admin/workflows").into_response())
}

async fn unpublish(
    State(state): State<AppState>,
    WebUser(user): WebUser,
    Path(id): Path<Uuid>,
) -> Result<Response, WebError> {
    require_admin(&user)?;
    workflow_definitions::set_published(&state.pool, id, false).await?;
    Ok(Redirect::to("/admin/workflows").into_response())
}
