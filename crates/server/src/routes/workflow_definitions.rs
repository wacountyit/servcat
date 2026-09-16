use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use servcat_db::repositories::workflow_definitions;
use servcat_model::{NewWorkflowDefinition, Role, WorkflowDefinition};
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, state::AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/workflow-definitions", get(list).post(create))
        .route("/workflow-definitions/{id}/publish", post(publish))
        .route("/workflow-definitions/{id}/unpublish", post(unpublish))
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default)]
    published_only: bool,
}

async fn list(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<WorkflowDefinition>>, ApiError> {
    // Authoring/publishing a graph is an admin concern; only admins need the
    // full list including unpublished drafts.
    auth_user.require_role(&[Role::Admin])?;
    Ok(Json(workflow_definitions::list(&state.pool, query.published_only).await?))
}

async fn create(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Json(new_def): Json<NewWorkflowDefinition>,
) -> Result<Json<WorkflowDefinition>, ApiError> {
    auth_user.require_role(&[Role::Admin])?;
    let created = workflow_definitions::create(&state.pool, &new_def, auth_user.0.id).await?;
    Ok(Json(created))
}

async fn publish(auth_user: AuthUser, State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<(), ApiError> {
    auth_user.require_role(&[Role::Admin])?;
    workflow_definitions::set_published(&state.pool, id, true).await?;
    Ok(())
}

async fn unpublish(auth_user: AuthUser, State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<(), ApiError> {
    auth_user.require_role(&[Role::Admin])?;
    workflow_definitions::set_published(&state.pool, id, false).await?;
    Ok(())
}
