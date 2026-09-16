use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use servcat_db::repositories::approvals as approvals_repo;
use servcat_model::{DecideApproval, PendingApproval, WorkflowInstance};
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, orchestrator, state::AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/approvals", get(list_mine))
        .route("/approvals/{id}/decide", post(decide))
}

async fn list_mine(
    auth_user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<PendingApproval>>, ApiError> {
    Ok(Json(
        approvals_repo::list_pending_for_approver(&state.pool, auth_user.0.id).await?,
    ))
}

async fn decide(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<DecideApproval>,
) -> Result<Json<WorkflowInstance>, ApiError> {
    let approval =
        servcat_approvals::decide(&state.pool, id, auth_user.0.id, req.decision, req.comment)
            .await?;

    let deps = orchestrator::Deps {
        pool: &state.pool,
        notifier: state.notifier.as_ref(),
        connectors: &state.connectors,
    };
    let instance = orchestrator::resume_after_approval(
        &deps,
        approval.workflow_instance_id,
        &approval.step_id,
        req.decision,
    )
    .await?;

    Ok(Json(instance))
}
