use askama::Template;
use axum::{
    Router,
    extract::{Path, State},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use servcat_db::repositories::{catalog, workflow_definitions};
use servcat_model::{CatalogItemDetails, ServiceCatalogItem, Step, StepKind, WorkflowGraph};
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
        .route("/catalog", get(show))
        .route("/catalog/{id}", get(item_detail))
        .route("/catalog/{id}/request", post(start_request))
}

/// Section order for the catalog's category groupings -- matches the
/// starter catalog's own category list (see `catalog_seed`) so the page
/// reads the same way it was designed, but this is just a display order:
/// any category an admin types in by hand that isn't on this list still
/// gets its own section, sorted alphabetically after these.
const CATEGORY_ORDER: &[&str] = &[
    "Accounts & Access",
    "Network & Connectivity",
    "Workstations & Devices",
    "Software",
    "Printing, Copying & Scanning",
    "Telephone & Mobile",
    "Audio/Visual & Collaboration",
    "Servers, Data & Infrastructure",
    "Security & Physical Access",
    "Other",
];

struct CatalogCard {
    item: ServiceCatalogItem,
    /// Lowercased "name summary keywords...", searched client-side against
    /// the lowercased contents of the search box -- see
    /// `static/catalog_search.js`.
    search_blob: String,
}

struct CategoryGroup {
    name: String,
    cards: Vec<CatalogCard>,
}

#[derive(Template)]
#[template(path = "catalog.html")]
struct CatalogTemplate {
    layout: Layout,
    groups: Vec<CategoryGroup>,
    categories: Vec<String>,
    has_items: bool,
}

fn build_groups(items: Vec<ServiceCatalogItem>) -> (Vec<CategoryGroup>, Vec<String>) {
    let mut by_category: std::collections::HashMap<String, Vec<CatalogCard>> =
        std::collections::HashMap::new();
    for item in items {
        let details = item.details();
        let search_blob = format!(
            "{} {} {}",
            item.name.to_lowercase(),
            item.summary.clone().unwrap_or_default().to_lowercase(),
            details.keywords.join(" ").to_lowercase()
        );
        let category = item.category.clone().unwrap_or_else(|| "Other".to_string());
        by_category
            .entry(category)
            .or_default()
            .push(CatalogCard { item, search_blob });
    }
    for cards in by_category.values_mut() {
        cards.sort_by(|a, b| {
            a.item
                .sort_order
                .cmp(&b.item.sort_order)
                .then_with(|| a.item.name.cmp(&b.item.name))
        });
    }

    let mut names: Vec<String> = CATEGORY_ORDER
        .iter()
        .map(|s| s.to_string())
        .filter(|c| by_category.contains_key(c))
        .collect();
    let mut leftover: Vec<String> = by_category
        .keys()
        .filter(|c| !CATEGORY_ORDER.contains(&c.as_str()))
        .cloned()
        .collect();
    leftover.sort();
    names.extend(leftover);

    let groups = names
        .iter()
        .map(|name| CategoryGroup {
            name: name.clone(),
            cards: by_category.remove(name).unwrap_or_default(),
        })
        .collect();
    (groups, names)
}

async fn show(State(state): State<AppState>, WebUser(user): WebUser) -> Result<Response, WebError> {
    let layout = Layout::load(&state, &user, "catalog").await?;
    let items = catalog::list(&state.pool, false).await?;
    let has_items = !items.is_empty();
    let (groups, categories) = build_groups(items);
    Ok(html(CatalogTemplate {
        layout,
        groups,
        categories,
        has_items,
    }))
}

struct QuestionPreview {
    label: String,
    required: bool,
}

/// Walks the leading run of `Question` steps starting at the workflow's
/// entry step, stopping at the first non-question step -- just enough to
/// show a requester what they'll be asked before they click "Request",
/// without re-implementing the engine's own branching/approval logic here.
fn preview_questions(graph: &WorkflowGraph) -> Vec<QuestionPreview> {
    let mut questions = Vec::new();
    let mut current: Option<&Step> = graph.step(&graph.entry_step_id);
    while let Some(step) = current {
        match &step.kind {
            StepKind::Question { required, next, .. } => {
                questions.push(QuestionPreview {
                    label: step.label.clone(),
                    required: *required,
                });
                current = graph.step(next);
            }
            _ => break,
        }
    }
    questions
}

#[derive(Template)]
#[template(path = "catalog_item.html")]
struct CatalogItemTemplate {
    layout: Layout,
    item: ServiceCatalogItem,
    details: CatalogItemDetails,
    questions: Vec<QuestionPreview>,
}

async fn item_detail(
    State(state): State<AppState>,
    WebUser(user): WebUser,
    Path(id): Path<Uuid>,
) -> Result<Response, WebError> {
    let layout = Layout::load(&state, &user, "catalog").await?;
    let item = catalog::get_by_id(&state.pool, id)
        .await?
        .filter(|item| item.is_active)
        .ok_or_else(|| ApiError::NotFound("service catalog item not found".into()))?;
    let definition = workflow_definitions::get_by_id(&state.pool, item.workflow_definition_id)
        .await?
        .ok_or(ApiError::Internal)?;
    let questions = preview_questions(&definition.graph);
    let details = item.details();
    Ok(html(CatalogItemTemplate {
        layout,
        item,
        details,
        questions,
    }))
}

async fn start_request(
    State(state): State<AppState>,
    WebUser(user): WebUser,
    Path(id): Path<Uuid>,
) -> Result<Response, WebError> {
    let deps = orchestrator::Deps {
        pool: &state.pool,
        notifier: state.notifier.as_ref(),
        connectors: &state.connectors,
    };
    let instance = orchestrator::start_instance(&deps, id, &user).await?;
    Ok(Redirect::to(&format!("/requests/{}", instance.id)).into_response())
}
