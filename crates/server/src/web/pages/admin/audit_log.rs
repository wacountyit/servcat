use std::collections::HashMap;

use askama::Template;
use axum::{
    Router,
    extract::{Query, State},
    response::Response,
    routing::get,
};
use serde::Deserialize;
use servcat_db::repositories::{audit, users};
use uuid::Uuid;

use crate::{
    state::AppState,
    web::{
        layout::Layout,
        render::{WebError, html},
        session::WebUser,
    },
};

use super::require_admin;

const PAGE_SIZE: i64 = 50;

pub fn routes() -> Router<AppState> {
    Router::new().route("/admin/audit-log", get(list))
}

/// One row as the template actually renders it -- the actor's display name
/// resolved (rather than a bare `actor_user_id`) and `metadata_json`
/// stringified, so the template only ever does field access, never a
/// method call Askama might pass arguments to unexpectedly (see
/// `is_selected_parent` in `admin/departments.rs`).
struct AuditRow {
    actor_label: String,
    action: String,
    entity_type: String,
    entity_id: Option<String>,
    metadata: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Template)]
#[template(path = "admin/audit_log.html")]
struct AuditLogTemplate {
    layout: Layout,
    admin_section: &'static str,
    rows: Vec<AuditRow>,
    page: i64,
    total_pages: i64,
    has_prev: bool,
    has_next: bool,
    /// Precomputed rather than done as `page - 1`/`page + 1` in the
    /// template -- Askama's arithmetic has no precedent elsewhere in this
    /// codebase, so plain field access is the safer bet.
    prev_page: i64,
    next_page: i64,
}

#[derive(Deserialize)]
struct AuditLogQuery {
    page: Option<i64>,
}

async fn list(
    State(state): State<AppState>,
    WebUser(user): WebUser,
    Query(query): Query<AuditLogQuery>,
) -> Result<Response, WebError> {
    require_admin(&user)?;
    let layout = Layout::load(&state, &user, "admin").await?;

    let page = query.page.unwrap_or(1).max(1);
    let total = audit::count(&state.pool).await?;
    let entries = audit::list(&state.pool, page - 1, PAGE_SIZE).await?;

    let mut actor_names: HashMap<Uuid, String> = HashMap::new();
    let mut rows = Vec::with_capacity(entries.len());
    for entry in entries {
        let actor_label = match entry.actor_user_id {
            Some(actor_id) => match actor_names.get(&actor_id) {
                Some(name) => name.clone(),
                None => {
                    let name = users::get_by_id(&state.pool, actor_id)
                        .await?
                        .map(|u| u.display_name)
                        .unwrap_or_else(|| "(deactivated or removed user)".to_string());
                    actor_names.insert(actor_id, name.clone());
                    name
                }
            },
            None => "System".to_string(),
        };

        rows.push(AuditRow {
            actor_label,
            action: entry.action,
            entity_type: entry.entity_type,
            entity_id: entry.entity_id,
            metadata: entry.metadata_json.map(|v| v.to_string()),
            created_at: entry.created_at,
        });
    }

    let total_pages = ((total.max(0) + PAGE_SIZE - 1) / PAGE_SIZE).max(1);

    Ok(html(AuditLogTemplate {
        layout,
        admin_section: "audit-log",
        rows,
        page,
        total_pages,
        has_prev: page > 1,
        has_next: page < total_pages,
        prev_page: page - 1,
        next_page: page + 1,
    }))
}
