use askama::Template;
use axum::{
    Form, Router,
    extract::{Path, State},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::Value as JsonValue;
use servcat_db::repositories::{catalog, tickets as tickets_repo, workflow_definitions};
use servcat_model::{InputType, QuestionOption, Role, StepKind, Ticket, WorkflowInstance};
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
        .route("/requests", get(list_mine))
        .route("/requests/{id}", get(show))
        .route("/requests/{id}/answer", post(submit_answer))
}

#[derive(Template)]
#[template(path = "requests_list.html")]
struct RequestsListTemplate {
    layout: Layout,
    requests: Vec<WorkflowInstance>,
}

async fn list_mine(
    State(state): State<AppState>,
    WebUser(user): WebUser,
) -> Result<Response, WebError> {
    let layout = Layout::load(&state, &user, "requests").await?;
    let requests =
        servcat_db::repositories::instances::list_for_requester(&state.pool, user.id).await?;
    Ok(html(RequestsListTemplate { layout, requests }))
}

struct CurrentQuestion {
    field_key: String,
    label: String,
    input_type: InputType,
    required: bool,
    options: Vec<QuestionOption>,
}

#[derive(Template)]
#[template(path = "request_detail.html")]
struct RequestDetailTemplate {
    layout: Layout,
    instance: WorkflowInstance,
    catalog_item_name: String,
    question: Option<CurrentQuestion>,
    answers: Vec<(String, String)>,
    ticket: Option<Ticket>,
    /// Only populated for staff (admin/agent) -- a requester sees that
    /// dispatch failed but not the raw connector error, which can contain
    /// internal details (auth failures, target-system URLs, etc).
    ticket_error: Option<String>,
}

async fn show(
    State(state): State<AppState>,
    WebUser(user): WebUser,
    Path(id): Path<Uuid>,
) -> Result<Response, WebError> {
    let layout = Layout::load(&state, &user, "requests").await?;
    let instance = servcat_db::repositories::instances::get_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::NotFound("request not found".into()))?;

    let is_owner = instance.requester_user_id == user.id;
    let is_staff = matches!(user.role, Role::Admin | Role::Agent);
    if !is_owner && !is_staff {
        return Err(WebError(ApiError::Forbidden));
    }

    let item = catalog::get_by_id(&state.pool, instance.catalog_item_id)
        .await?
        .ok_or(ApiError::Internal)?;
    let definition = workflow_definitions::get_by_id(&state.pool, instance.workflow_definition_id)
        .await?
        .ok_or(ApiError::Internal)?;

    let question = if instance.status == servcat_model::InstanceStatus::InProgress {
        definition
            .graph
            .step(&instance.current_step_id)
            .and_then(|step| match &step.kind {
                StepKind::Question {
                    field_key,
                    input_type,
                    required,
                    options,
                    ..
                } => Some(CurrentQuestion {
                    field_key: field_key.clone(),
                    label: step.label.clone(),
                    input_type: *input_type,
                    required: *required,
                    options: options.clone(),
                }),
                _ => None,
            })
    } else {
        None
    };

    let answers = instance
        .answers_json
        .as_object()
        .map(|map| {
            map.iter()
                .map(|(k, v)| (k.clone(), display_value(v)))
                .collect()
        })
        .unwrap_or_default();

    let ticket = tickets_repo::get_by_instance_id(&state.pool, instance.id).await?;
    let ticket_error = if is_staff {
        ticket.as_ref().and_then(|t| t.last_error.clone())
    } else {
        None
    };

    Ok(html(RequestDetailTemplate {
        layout,
        instance,
        catalog_item_name: item.name,
        question,
        answers,
        ticket,
        ticket_error,
    }))
}

fn display_value(value: &JsonValue) -> String {
    match value {
        JsonValue::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[derive(Deserialize)]
struct AnswerForm {
    field_key: String,
    value: String,
}

async fn submit_answer(
    State(state): State<AppState>,
    WebUser(user): WebUser,
    Path(id): Path<Uuid>,
    Form(form): Form<AnswerForm>,
) -> Result<Response, WebError> {
    let instance = servcat_db::repositories::instances::get_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::NotFound("request not found".into()))?;
    let definition = workflow_definitions::get_by_id(&state.pool, instance.workflow_definition_id)
        .await?
        .ok_or(ApiError::Internal)?;

    let step = definition.graph.step(&instance.current_step_id);
    let input_type = step.and_then(|s| match &s.kind {
        StepKind::Question { input_type, .. } => Some(*input_type),
        _ => None,
    });
    let value = coerce_answer(input_type, &form.value);

    let deps = orchestrator::Deps {
        pool: &state.pool,
        notifier: state.notifier.as_ref(),
        connectors: &state.connectors,
    };
    orchestrator::submit_answer(&deps, id, &user, &form.field_key, value).await?;

    Ok(Redirect::to(&format!("/requests/{id}")).into_response())
}

/// HTML forms only ever submit strings; coerce back to the JSON shape the
/// workflow engine's `Condition` evaluation expects (an `Equals` condition on
/// a boolean/number field compares against a real `JsonValue::Bool`/`Number`,
/// not the string `"true"`).
fn coerce_answer(input_type: Option<InputType>, raw: &str) -> JsonValue {
    match input_type {
        Some(InputType::Boolean) => JsonValue::Bool(raw == "true" || raw == "on"),
        Some(InputType::Number) => raw
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        Some(InputType::MultiSelect) => JsonValue::Array(
            raw.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| JsonValue::String(s.to_string()))
                .collect(),
        ),
        _ => JsonValue::String(raw.to_string()),
    }
}
