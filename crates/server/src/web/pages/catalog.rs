use askama::Template;
use axum::{
    Router,
    extract::{Path, State},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use servcat_db::repositories::catalog;
use servcat_model::ServiceCatalogItem;
use uuid::Uuid;

use crate::{
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
        .route("/catalog/{id}/request", post(start_request))
}

#[derive(Template)]
#[template(path = "catalog.html")]
struct CatalogTemplate {
    layout: Layout,
    items: Vec<ServiceCatalogItem>,
}

async fn show(State(state): State<AppState>, WebUser(user): WebUser) -> Result<Response, WebError> {
    let layout = Layout::load(&state, &user, "catalog").await?;
    let items = catalog::list(&state.pool, false).await?;
    Ok(html(CatalogTemplate { layout, items }))
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
