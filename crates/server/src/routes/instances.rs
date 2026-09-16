use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use servcat_db::repositories::instances;
use servcat_model::{Role, StartWorkflowInstance, SubmitAnswer, WorkflowInstance};
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, orchestrator, state::AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/instances", get(list_mine).post(start))
        .route("/instances/{id}", get(get_one))
        .route("/instances/{id}/answers", post(submit_answer))
}

async fn start(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Json(req): Json<StartWorkflowInstance>,
) -> Result<Json<WorkflowInstance>, ApiError> {
    let deps = orchestrator::Deps {
        pool: &state.pool,
        notifier: state.notifier.as_ref(),
        connectors: &state.connectors,
    };
    let instance = orchestrator::start_instance(&deps, req.catalog_item_id, &auth_user.0).await?;
    Ok(Json(instance))
}

async fn list_mine(
    auth_user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<WorkflowInstance>>, ApiError> {
    Ok(Json(
        instances::list_for_requester(&state.pool, auth_user.0.id).await?,
    ))
}

async fn get_one(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<WorkflowInstance>, ApiError> {
    let instance = instances::get_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::NotFound("request not found".into()))?;

    let is_owner = instance.requester_user_id == auth_user.0.id;
    let is_staff = matches!(auth_user.0.role, Role::Admin | Role::Agent);
    if !is_owner && !is_staff {
        return Err(ApiError::Forbidden);
    }

    Ok(Json(instance))
}

async fn submit_answer(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(answer): Json<SubmitAnswer>,
) -> Result<Json<WorkflowInstance>, ApiError> {
    let deps = orchestrator::Deps {
        pool: &state.pool,
        notifier: state.notifier.as_ref(),
        connectors: &state.connectors,
    };
    let instance =
        orchestrator::submit_answer(&deps, id, &auth_user.0, &answer.field_key, answer.value)
            .await?;
    Ok(Json(instance))
}
