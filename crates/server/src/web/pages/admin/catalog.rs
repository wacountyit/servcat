use askama::Template;
use axum::{
    Form, Router,
    extract::{Path, State},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::Deserialize;
use servcat_db::repositories::{catalog, workflow_definitions};
use servcat_model::{
    NewServiceCatalogItem, ServiceCatalogItem, UpdateServiceCatalogItem, WorkflowDefinition,
};
use uuid::Uuid;

use crate::{
    error::ApiError,
    state::AppState,
    web::{
        layout::Layout,
        render::{WebError, html},
        session::WebUser,
    },
};

use super::require_admin;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/admin/catalog", get(list).post(create))
        .route("/admin/catalog/{id}/toggle", post(toggle))
}

#[derive(Template)]
#[template(path = "admin/catalog.html")]
struct CatalogAdminTemplate {
    layout: Layout,
    admin_section: &'static str,
    items: Vec<ServiceCatalogItem>,
    workflow_definitions: Vec<WorkflowDefinition>,
}

async fn list(State(state): State<AppState>, WebUser(user): WebUser) -> Result<Response, WebError> {
    require_admin(&user)?;
    let layout = Layout::load(&state, &user, "admin").await?;
    let items = catalog::list(&state.pool, true).await?;
    let definitions = workflow_definitions::list(&state.pool, false).await?;
    Ok(html(CatalogAdminTemplate {
        layout,
        admin_section: "catalog",
        items,
        workflow_definitions: definitions,
    }))
}

#[derive(Deserialize)]
struct CreateCatalogItemForm {
    name: String,
    description: Option<String>,
    category: Option<String>,
    workflow_definition_id: Uuid,
}

async fn create(
    State(state): State<AppState>,
    WebUser(user): WebUser,
    Form(form): Form<CreateCatalogItemForm>,
) -> Result<Response, WebError> {
    require_admin(&user)?;
    catalog::create(
        &state.pool,
        &NewServiceCatalogItem {
            name: form.name,
            description: form.description.filter(|s| !s.is_empty()),
            category: form.category.filter(|s| !s.is_empty()),
            icon: None,
            workflow_definition_id: form.workflow_definition_id,
        },
        user.id,
    )
    .await?;
    Ok(Redirect::to("/admin/catalog").into_response())
}

#[derive(Deserialize)]
struct ToggleForm {
    is_active: bool,
}

async fn toggle(
    State(state): State<AppState>,
    WebUser(user): WebUser,
    Path(id): Path<Uuid>,
    Form(form): Form<ToggleForm>,
) -> Result<Response, WebError> {
    require_admin(&user)?;
    catalog::update(
        &state.pool,
        id,
        &UpdateServiceCatalogItem {
            name: None,
            description: None,
            category: None,
            icon: None,
            is_active: Some(form.is_active),
        },
    )
    .await?
    .ok_or_else(|| ApiError::NotFound("catalog item not found".into()))?;
    Ok(Redirect::to("/admin/catalog").into_response())
}
