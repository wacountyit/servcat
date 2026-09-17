use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};
use serde::{Deserialize, Serialize};
use servcat_db::repositories::audit;
use servcat_model::{AuditLogEntry, Role};

use crate::{auth::AuthUser, error::ApiError, state::AppState};

/// Rows per page, and the hard cap on a caller-supplied `page_size` -- large
/// enough for a human to page through comfortably, small enough that a
/// caller can't force an unbounded scan of a table that grows forever.
const DEFAULT_PAGE_SIZE: i64 = 50;
const MAX_PAGE_SIZE: i64 = 200;

pub fn routes() -> Router<AppState> {
    Router::new().route("/audit-log", get(list_audit_log))
}

#[derive(Deserialize)]
struct AuditLogQuery {
    /// 1-based, like the web UI's `?page=`.
    page: Option<i64>,
    page_size: Option<i64>,
}

#[derive(Serialize)]
struct AuditLogPage {
    entries: Vec<AuditLogEntry>,
    page: i64,
    page_size: i64,
    total: i64,
}

async fn list_audit_log(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Query(query): Query<AuditLogQuery>,
) -> Result<Json<AuditLogPage>, ApiError> {
    auth_user.require_role(&[Role::Admin])?;

    let page = query.page.unwrap_or(1).max(1);
    let page_size = query
        .page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);

    let total = audit::count(&state.pool).await?;
    let entries = audit::list(&state.pool, page - 1, page_size).await?;

    Ok(Json(AuditLogPage {
        entries,
        page,
        page_size,
        total,
    }))
}
