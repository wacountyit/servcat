use askama::Template;
use axum::response::Response;
use servcat_db::repositories::{approvals as approvals_repo, instances};

use crate::state::AppState;
use crate::web::{
    layout::Layout,
    render::{WebError, html},
    session::WebUser,
};
use axum::extract::State;

#[derive(Template)]
#[template(path = "dashboard.html")]
struct DashboardTemplate {
    layout: Layout,
    open_request_count: usize,
    pending_approval_count: usize,
}

pub async fn show(
    State(state): State<AppState>,
    WebUser(user): WebUser,
) -> Result<Response, WebError> {
    let layout = Layout::load(&state, &user, "dashboard").await?;

    let open_request_count = instances::list_for_requester(&state.pool, user.id)
        .await?
        .iter()
        .filter(|i| {
            matches!(
                i.status,
                servcat_model::InstanceStatus::InProgress
                    | servcat_model::InstanceStatus::AwaitingApproval
            )
        })
        .count();
    let pending_approval_count = approvals_repo::list_pending_for_approver(&state.pool, user.id)
        .await?
        .len();

    Ok(html(DashboardTemplate {
        layout,
        open_request_count,
        pending_approval_count,
    }))
}
