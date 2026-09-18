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
    CatalogItemDetails, NewServiceCatalogItem, ServiceCatalogItem, TargetUnit,
    UpdateServiceCatalogItem, WorkflowDefinition,
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
        .route("/admin/catalog/{id}/edit", get(edit_form).post(edit))
        .route("/admin/catalog/load-starter", post(load_starter))
}

#[derive(Template)]
#[template(path = "admin/catalog.html")]
struct CatalogAdminTemplate {
    layout: Layout,
    admin_section: &'static str,
    items: Vec<ServiceCatalogItem>,
    workflow_definitions: Vec<WorkflowDefinition>,
    seed_message: Option<String>,
    seed_item_count: usize,
}

#[derive(Deserialize)]
struct ListQuery {
    seed_added: Option<usize>,
    seed_skipped: Option<usize>,
}

async fn list(
    State(state): State<AppState>,
    WebUser(user): WebUser,
    axum::extract::Query(query): axum::extract::Query<ListQuery>,
) -> Result<Response, WebError> {
    require_admin(&user)?;
    let layout = Layout::load(&state, &user, "admin").await?;
    let items = catalog::list(&state.pool, true).await?;
    let definitions = workflow_definitions::list(&state.pool, false).await?;
    let seed_message = match (query.seed_added, query.seed_skipped) {
        (Some(added), Some(skipped)) => Some(format!(
            "Loaded the starter catalog: added {added} item{}, skipped {skipped} already present.",
            if added == 1 { "" } else { "s" }
        )),
        _ => None,
    };
    Ok(html(CatalogAdminTemplate {
        layout,
        admin_section: "catalog",
        items,
        workflow_definitions: definitions,
        seed_message,
        seed_item_count: crate::catalog_seed::seed_item_count(),
    }))
}

/// Shared by the create and edit forms -- newline-separated text areas for
/// the four list-shaped fields, since a plain HTML form has no clean way
/// to submit a `Vec<String>` otherwise, and this is far more pleasant to
/// hand-edit than a JSON array in a text box.
#[derive(Deserialize)]
struct CatalogItemForm {
    name: String,
    summary: String,
    description: String,
    category: String,
    icon: String,
    approval_label: String,
    target_value: String,
    target_unit: String,
    included: String,
    not_included: String,
    needed: String,
    keywords: String,
    #[serde(default)]
    workflow_definition_id: Option<Uuid>,
    #[serde(default)]
    is_active: bool,
}

fn lines_to_vec(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

/// Parses the target-time pair, requiring both-or-neither -- a value with
/// no unit (or vice versa) can't be rendered ("3 <nothing>"?) so it's
/// rejected here rather than silently stored half-complete.
fn parse_target(value: &str, unit: &str) -> Result<(Option<i32>, Option<TargetUnit>), String> {
    let value = value.trim();
    let unit = unit.trim();
    match (value.is_empty(), unit.is_empty()) {
        (true, true) => Ok((None, None)),
        (false, false) => {
            let parsed_value: i32 = value
                .parse()
                .map_err(|_| format!("target time '{value}' isn't a whole number"))?;
            if parsed_value < 0 {
                return Err("target time can't be negative".to_string());
            }
            let parsed_unit: TargetUnit = unit
                .parse()
                .map_err(|_| format!("'{unit}' isn't a recognized target time unit"))?;
            Ok((Some(parsed_value), Some(parsed_unit)))
        }
        _ => Err("set both a target time and a unit, or leave both blank".to_string()),
    }
}

fn details_from_form(form: &CatalogItemForm) -> CatalogItemDetails {
    CatalogItemDetails {
        included: lines_to_vec(&form.included),
        not_included: lines_to_vec(&form.not_included),
        needed: lines_to_vec(&form.needed),
        keywords: lines_to_vec(&form.keywords),
    }
}

async fn create(
    State(state): State<AppState>,
    WebUser(user): WebUser,
    Form(form): Form<CatalogItemForm>,
) -> Result<Response, WebError> {
    require_admin(&user)?;

    let Some(workflow_definition_id) = form.workflow_definition_id else {
        return list_with_error(&state, &user, "Choose a workflow for this catalog item.").await;
    };
    let (target_value, target_unit) = match parse_target(&form.target_value, &form.target_unit) {
        Ok(t) => t,
        Err(err) => return list_with_error(&state, &user, &err).await,
    };

    let details = details_from_form(&form);
    catalog::create(
        &state.pool,
        &NewServiceCatalogItem {
            slug: None,
            name: form.name,
            description: Some(form.description).filter(|s| !s.is_empty()),
            summary: Some(form.summary).filter(|s| !s.is_empty()),
            details: Some(details),
            approval_label: Some(form.approval_label).filter(|s| !s.is_empty()),
            target_value,
            target_unit,
            category: Some(form.category).filter(|s| !s.is_empty()),
            icon: Some(form.icon).filter(|s| !s.is_empty()),
            sort_order: 0,
            workflow_definition_id,
        },
        user.id,
    )
    .await?;
    Ok(Redirect::to("/admin/catalog").into_response())
}

async fn list_with_error(
    state: &AppState,
    user: &servcat_model::User,
    error: &str,
) -> Result<Response, WebError> {
    // Re-render the list page with an error banner rather than a generic
    // 500/400 -- there's no dedicated "create item" page to redisplay
    // (the form lives inline on the list page), so this is that page's
    // own equivalent of the workflow builder's `workflow_form_error`.
    let layout = Layout::load(state, user, "admin").await?;
    let items = catalog::list(&state.pool, true).await?;
    let definitions = workflow_definitions::list(&state.pool, false).await?;
    Ok((
        axum::http::StatusCode::BAD_REQUEST,
        html(CatalogAdminTemplate {
            layout,
            admin_section: "catalog",
            items,
            workflow_definitions: definitions,
            seed_message: Some(format!("Couldn't save: {error}")),
            seed_item_count: crate::catalog_seed::seed_item_count(),
        }),
    )
        .into_response())
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
            summary: None,
            details: None,
            approval_label: None,
            target_value: None,
            target_unit: None,
            category: None,
            icon: None,
            sort_order: None,
            is_active: Some(form.is_active),
        },
    )
    .await?
    .ok_or_else(|| ApiError::NotFound("catalog item not found".into()))?;
    Ok(Redirect::to("/admin/catalog").into_response())
}

#[derive(Template)]
#[template(path = "admin/catalog_edit.html")]
struct CatalogEditTemplate {
    layout: Layout,
    admin_section: &'static str,
    item: ServiceCatalogItem,
    details: CatalogItemDetails,
    error: Option<String>,
}

async fn edit_form(
    State(state): State<AppState>,
    WebUser(user): WebUser,
    Path(id): Path<Uuid>,
) -> Result<Response, WebError> {
    require_admin(&user)?;
    let layout = Layout::load(&state, &user, "admin").await?;
    let item = catalog::get_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::NotFound("catalog item not found".into()))?;
    let details = item.details();
    Ok(html(CatalogEditTemplate {
        layout,
        admin_section: "catalog",
        item,
        details,
        error: None,
    }))
}

async fn edit(
    State(state): State<AppState>,
    WebUser(user): WebUser,
    Path(id): Path<Uuid>,
    Form(form): Form<CatalogItemForm>,
) -> Result<Response, WebError> {
    require_admin(&user)?;

    let (target_value, target_unit) = match parse_target(&form.target_value, &form.target_unit) {
        Ok(t) => t,
        Err(err) => return edit_form_error(&state, &user, id, &form, &err).await,
    };

    // Every field below is a full resubmission of the edit form (not a
    // partial patch), so it's always `Some(...)` -- including on an empty
    // string, which is a deliberate "clear this field" rather than "leave
    // it alone" (unlike `target_value`/`target_unit`: `catalog::update`'s
    // COALESCE-based SQL treats `None` as "don't touch", with no way to
    // explicitly null out a field through it, so leaving *both* target
    // inputs blank here means "no change", not "remove the target" --
    // the same known limitation `UpdateUser` already has for
    // department_id/manager_user_id).
    let updated = catalog::update(
        &state.pool,
        id,
        &UpdateServiceCatalogItem {
            name: Some(form.name.clone()),
            description: Some(form.description.clone()),
            summary: Some(form.summary.clone()),
            details: Some(details_from_form(&form)),
            approval_label: Some(form.approval_label.clone()),
            target_value,
            target_unit,
            category: Some(form.category.clone()),
            icon: Some(form.icon.clone()),
            sort_order: None,
            is_active: Some(form.is_active),
        },
    )
    .await?;
    if updated.is_none() {
        return Err(WebError(ApiError::NotFound(
            "catalog item not found".into(),
        )));
    }
    Ok(Redirect::to("/admin/catalog").into_response())
}

async fn edit_form_error(
    state: &AppState,
    user: &servcat_model::User,
    id: Uuid,
    form: &CatalogItemForm,
    error: &str,
) -> Result<Response, WebError> {
    let layout = Layout::load(state, user, "admin").await?;
    // Re-fetch so we still have `workflow_definition_id`/`created_at`/etc,
    // then overlay it with exactly what was submitted so nothing the admin
    // just typed is lost.
    let mut item = catalog::get_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::NotFound("catalog item not found".into()))?;
    item.name = form.name.clone();
    item.description = Some(form.description.clone());
    item.summary = Some(form.summary.clone());
    item.approval_label = Some(form.approval_label.clone());
    item.category = Some(form.category.clone());
    item.icon = Some(form.icon.clone());
    item.is_active = form.is_active;
    let details = details_from_form(form);
    Ok(html(CatalogEditTemplate {
        layout,
        admin_section: "catalog",
        item,
        details,
        error: Some(error.to_string()),
    }))
}

async fn load_starter(
    State(state): State<AppState>,
    WebUser(user): WebUser,
) -> Result<Response, WebError> {
    require_admin(&user)?;
    let summary = crate::catalog_seed::run(&state.pool, user.id).await?;
    Ok(Redirect::to(&format!(
        "/admin/catalog?seed_added={}&seed_skipped={}",
        summary.added, summary.skipped
    ))
    .into_response())
}
