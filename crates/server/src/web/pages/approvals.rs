use askama::Template;
use axum::{
    Form, Router,
    extract::{Path, State},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::Deserialize;
use servcat_db::repositories::{approvals as approvals_repo, catalog, instances, users};
use servcat_model::ApprovalDecision;
use uuid::Uuid;

use crate::{
    error::ApiError,
    orchestrator,
    state::AppState,
    web::{
        layout::Layout,
        render::{WebError, html},
        session::WebUser,
    },
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/approvals", get(list_mine))
        .route("/approvals/{id}/decide", post(decide))
}

struct ApprovalRow {
    id: Uuid,
    catalog_item_name: String,
    requester_display_name: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Template)]
#[template(path = "approvals_list.html")]
struct ApprovalsListTemplate {
    layout: Layout,
    approvals: Vec<ApprovalRow>,
}

async fn list_mine(
    State(state): State<AppState>,
    WebUser(user): WebUser,
) -> Result<Response, WebError> {
    let layout = Layout::load(&state, &user, "approvals").await?;
    let pending = approvals_repo::list_pending_for_approver(&state.pool, user.id).await?;

    let mut approvals = Vec::with_capacity(pending.len());
    for approval in pending {
        let Some(instance) =
            instances::get_by_id(&state.pool, approval.workflow_instance_id).await?
        else {
            continue;
        };
        let item_name = catalog::get_by_id(&state.pool, instance.catalog_item_id)
            .await?
            .map(|i| i.name)
            .unwrap_or_else(|| "(deleted service)".to_string());
        let requester_name = users::get_by_id(&state.pool, instance.requester_user_id)
            .await?
            .map(|u| u.display_name)
            .unwrap_or_else(|| "(deactivated user)".to_string());

        approvals.push(ApprovalRow {
            id: approval.id,
            catalog_item_name: item_name,
            requester_display_name: requester_name,
            created_at: approval.created_at,
        });
    }

    Ok(html(ApprovalsListTemplate { layout, approvals }))
}

#[derive(Deserialize)]
struct DecideForm {
    decision: String,
    comment: Option<String>,
}

async fn decide(
    State(state): State<AppState>,
    WebUser(user): WebUser,
    Path(id): Path<Uuid>,
    Form(form): Form<DecideForm>,
) -> Result<Response, WebError> {
    let decision = match form.decision.as_str() {
        "approve" => ApprovalDecision::Approve,
        "reject" => ApprovalDecision::Reject,
        _ => return Err(WebError(ApiError::BadRequest("invalid decision".into()))),
    };
    let comment = form.comment.filter(|c| !c.trim().is_empty());

    let approval = servcat_approvals::decide(&state.pool, id, user.id, decision, comment).await?;

    let deps = orchestrator::Deps {
        pool: &state.pool,
        notifier: state.notifier.as_ref(),
        connectors: &state.connectors,
    };
    orchestrator::resume_after_approval(
        &deps,
        approval.workflow_instance_id,
        &approval.step_id,
        decision,
    )
    .await?;

    Ok(Redirect::to("/approvals").into_response())
}
