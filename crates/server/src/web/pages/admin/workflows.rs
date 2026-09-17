use std::collections::HashMap;

use askama::Template;
use axum::{
    Form, Router,
    extract::{Path, State},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::Deserialize;
use servcat_db::repositories::workflow_definitions;
use servcat_model::{
    FieldMapping, NewWorkflowDefinition, TargetSystem, WorkflowDefinition, WorkflowGraph,
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
    error: Option<String>,
}

async fn list(State(state): State<AppState>, WebUser(user): WebUser) -> Result<Response, WebError> {
    require_admin(&user)?;
    let layout = Layout::load(&state, &user, "admin").await?;
    let definitions = workflow_definitions::list(&state.pool, false).await?;
    Ok(html(WorkflowsTemplate {
        layout,
        admin_section: "workflows",
        definitions,
        error: None,
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
        Err(err) => return workflow_form_error(&state, &user, format!("graph JSON: {err}")).await,
    };
    let field_mapping = if form.field_mapping_json.trim().is_empty() {
        None
    } else {
        match serde_json::from_str::<HashMap<String, String>>(&form.field_mapping_json) {
            Ok(map) => Some(FieldMapping(map)),
            Err(err) => {
                return workflow_form_error(&state, &user, format!("field mapping JSON: {err}"))
                    .await;
            }
        }
    };
    let target_system = if form.target_system.is_empty() {
        None
    } else {
        match form.target_system.parse::<TargetSystem>() {
            Ok(t) => Some(t),
            Err(err) => return workflow_form_error(&state, &user, err).await,
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

async fn workflow_form_error(
    state: &AppState,
    user: &servcat_model::User,
    error: String,
) -> Result<Response, WebError> {
    let layout = Layout::load(state, user, "admin").await?;
    let definitions = workflow_definitions::list(&state.pool, false).await?;
    Ok(html(WorkflowsTemplate {
        layout,
        admin_section: "workflows",
        definitions,
        error: Some(error),
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
