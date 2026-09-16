//! Wires the pure `workflow-engine` interpreter to the side effects it
//! triggers: persisting instance progress, resolving and notifying
//! approvers, and rendering + dispatching tickets through a connector. This
//! is the "glue" layer the engine itself deliberately stays free of.

use serde_json::{Map, Value as JsonValue};
use servcat_approvals::ApprovalNotifier;
use servcat_connectors::{render_field_mapping, TemplateContext};
use servcat_db::{
    repositories::{catalog, instances, tickets as tickets_repo, users, workflow_definitions},
    Pool,
};
use servcat_model::{ApprovalDecision, EndOutcome, InstanceStatus, User, WorkflowDefinition, WorkflowInstance};
use servcat_workflow_engine::{self as engine, Outcome};
use uuid::Uuid;

use crate::{connector_registry::ConnectorRegistry, error::ApiError};

/// The handles every orchestration step needs. Bundled so call signatures
/// stay readable as the number of collaborators grows.
pub struct Deps<'a> {
    pub pool: &'a Pool,
    pub notifier: &'a dyn ApprovalNotifier,
    pub connectors: &'a ConnectorRegistry,
}

fn answers_map(value: &JsonValue) -> Map<String, JsonValue> {
    value.as_object().cloned().unwrap_or_default()
}

pub async fn start_instance(
    deps: &Deps<'_>,
    catalog_item_id: Uuid,
    requester: &User,
) -> Result<WorkflowInstance, ApiError> {
    let item = catalog::get_by_id(deps.pool, catalog_item_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("service catalog item not found".into()))?;
    if !item.is_active {
        return Err(ApiError::Conflict("this service is not currently offered".into()));
    }

    let definition = workflow_definitions::get_by_id(deps.pool, item.workflow_definition_id)
        .await?
        .ok_or(ApiError::Internal)?;

    let (step_id, outcome) = engine::start(&definition.graph)?;
    let instance = instances::create(deps.pool, definition.id, item.id, requester.id, &step_id).await?;

    apply_outcome(
        deps,
        &definition,
        instance.id,
        requester.id,
        answers_map(&instance.answers_json),
        step_id,
        outcome,
    )
    .await
}

pub async fn submit_answer(
    deps: &Deps<'_>,
    instance_id: Uuid,
    requester: &User,
    field_key: &str,
    value: JsonValue,
) -> Result<WorkflowInstance, ApiError> {
    let instance = instances::get_by_id(deps.pool, instance_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("request not found".into()))?;
    if instance.requester_user_id != requester.id {
        return Err(ApiError::Forbidden);
    }
    if instance.status != InstanceStatus::InProgress {
        return Err(ApiError::Conflict("this request is not currently awaiting input".into()));
    }

    let definition = workflow_definitions::get_by_id(deps.pool, instance.workflow_definition_id)
        .await?
        .ok_or(ApiError::Internal)?;

    let mut answers = answers_map(&instance.answers_json);
    let (step_id, outcome) =
        engine::submit_answer(&definition.graph, &instance.current_step_id, &mut answers, field_key, value)?;

    apply_outcome(deps, &definition, instance.id, requester.id, answers, step_id, outcome).await
}

/// Shared by the approvals API route (a human decides) and the expiry
/// poller (a timeout auto-decides as a rejection).
pub async fn resume_after_approval(
    deps: &Deps<'_>,
    instance_id: Uuid,
    step_id: &str,
    decision: ApprovalDecision,
) -> Result<WorkflowInstance, ApiError> {
    let instance = instances::get_by_id(deps.pool, instance_id).await?.ok_or(ApiError::Internal)?;
    let definition = workflow_definitions::get_by_id(deps.pool, instance.workflow_definition_id)
        .await?
        .ok_or(ApiError::Internal)?;

    let answers = answers_map(&instance.answers_json);
    let (next_step_id, outcome) = engine::resume_after_approval(&definition.graph, step_id, &answers, decision)?;

    apply_outcome(deps, &definition, instance.id, instance.requester_user_id, answers, next_step_id, outcome).await
}

/// Drives `outcome` to the next point that needs external input (or to
/// completion), performing whatever side effect each `Outcome` implies along
/// the way. `step_id` is the step `outcome` was produced for/at.
async fn apply_outcome(
    deps: &Deps<'_>,
    definition: &WorkflowDefinition,
    instance_id: Uuid,
    requester_user_id: Uuid,
    answers: Map<String, JsonValue>,
    mut step_id: String,
    mut outcome: Outcome,
) -> Result<WorkflowInstance, ApiError> {
    loop {
        match outcome {
            Outcome::AwaitingAnswer { .. } => {
                return instances::save_progress(
                    deps.pool,
                    instance_id,
                    &step_id,
                    &JsonValue::Object(answers),
                    InstanceStatus::InProgress,
                )
                .await?
                .ok_or(ApiError::Internal);
            }

            Outcome::AwaitingApproval { resolution, timeout_seconds } => {
                let requester = users::get_by_id(deps.pool, requester_user_id).await?.ok_or(ApiError::Internal)?;
                servcat_approvals::create_for_instance(
                    deps.pool,
                    deps.notifier,
                    instance_id,
                    &step_id,
                    &requester,
                    &resolution,
                    timeout_seconds,
                )
                .await?;

                return instances::save_progress(
                    deps.pool,
                    instance_id,
                    &step_id,
                    &JsonValue::Object(answers),
                    InstanceStatus::AwaitingApproval,
                )
                .await?
                .ok_or(ApiError::Internal);
            }

            Outcome::ReadyToSubmitTicket => {
                if let Err(err) = dispatch_ticket(deps, definition, instance_id, requester_user_id, &answers).await {
                    instances::save_progress(
                        deps.pool,
                        instance_id,
                        &step_id,
                        &JsonValue::Object(answers.clone()),
                        InstanceStatus::Failed,
                    )
                    .await?;
                    return Err(err);
                }

                let (next_step_id, next_outcome) =
                    engine::resume_after_ticket_dispatch(&definition.graph, &step_id, &answers)?;
                step_id = next_step_id;
                outcome = next_outcome;
            }

            Outcome::Finished { outcome: end_outcome } => {
                let status = match end_outcome {
                    EndOutcome::Completed => InstanceStatus::Completed,
                    EndOutcome::Rejected => InstanceStatus::Rejected,
                    EndOutcome::Cancelled => InstanceStatus::Cancelled,
                };
                return instances::save_progress(deps.pool, instance_id, &step_id, &JsonValue::Object(answers), status)
                    .await?
                    .ok_or(ApiError::Internal);
            }
        }
    }
}

async fn dispatch_ticket(
    deps: &Deps<'_>,
    definition: &WorkflowDefinition,
    instance_id: Uuid,
    requester_user_id: Uuid,
    answers: &Map<String, JsonValue>,
) -> Result<(), ApiError> {
    let target_system = definition.target_system.ok_or_else(|| {
        tracing::error!(workflow_definition_id = %definition.id, "SubmitTicket step reached but no target_system configured");
        ApiError::Internal
    })?;
    let mapping = definition.field_mapping.as_ref().ok_or_else(|| {
        tracing::error!(workflow_definition_id = %definition.id, "SubmitTicket step reached but no field_mapping configured");
        ApiError::Internal
    })?;
    let connector = deps.connectors.get(target_system).ok_or_else(|| {
        tracing::error!(?target_system, "no connector registered for this deployment");
        ApiError::Internal
    })?;

    let requester = users::get_by_id(deps.pool, requester_user_id).await?.ok_or(ApiError::Internal)?;
    let instance = instances::get_by_id(deps.pool, instance_id).await?.ok_or(ApiError::Internal)?;
    let item = catalog::get_by_id(deps.pool, instance.catalog_item_id).await?.ok_or(ApiError::Internal)?;

    let context = TemplateContext::new(
        &JsonValue::Object(answers.clone()),
        &requester.email,
        &requester.display_name,
        &instance_id.to_string(),
        &item.name,
    );
    let payload = render_field_mapping(mapping, &context).map_err(|err| {
        tracing::error!(error = %err, workflow_definition_id = %definition.id, "field mapping render failed");
        ApiError::Internal
    })?;

    let ticket = tickets_repo::create(deps.pool, instance_id, target_system, &payload).await?;

    match connector.dispatch(&payload).await {
        Ok(result) => {
            tickets_repo::mark_sent(
                deps.pool,
                ticket.id,
                &result.external_ticket_id,
                result.external_ticket_url.as_deref(),
            )
            .await?;
            Ok(())
        }
        Err(err) => {
            tickets_repo::mark_failed(deps.pool, ticket.id, &err.to_string()).await?;
            Err(ApiError::from(err))
        }
    }
}
