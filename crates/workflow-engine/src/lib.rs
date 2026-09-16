//! Pure, synchronous interpreter for a `WorkflowGraph`. Deliberately does no
//! I/O: creating a `PendingApproval` row, dispatching a ticket through a
//! connector, sending a notification, etc. are all the caller's job. That
//! keeps this crate trivially unit-testable and keeps the engine from ever
//! blocking on a network call while holding, say, a workflow instance's
//! implicit "lock" (this repo's concurrency model, not a real DB lock).
//!
//! Typical call sequence from the server:
//!
//! 1. `start(graph)` when a requester picks a catalog item.
//! 2. `submit_answer(...)` each time they answer the current `Question`.
//! 3. `resume_after_approval(...)` when an approver decides a
//!    `WaitForApproval` step, or `resume_after_ticket_dispatch(...)` once a
//!    connector finishes a `SubmitTicket` step's dispatch.
//!
//! Each call returns the instance's new `current_step_id` plus an `Outcome`
//! describing what the caller must now do or wait for.

use serde_json::{Map, Value as JsonValue};
use servcat_model::{ApprovalDecision, ApproverResolution, Condition, EndOutcome, Step, StepKind, WorkflowGraph};

pub type Answers = Map<String, JsonValue>;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("workflow graph references unknown step id '{0}'")]
    UnknownStep(String),

    #[error("step '{step_id}' is a {found}, but a {expected} step was expected here")]
    WrongStepKind {
        step_id: String,
        expected: &'static str,
        found: &'static str,
    },

    #[error("step '{step_id}' expects an answer for field '{expected_field_key}', got '{actual_field_key}'")]
    UnexpectedField {
        step_id: String,
        expected_field_key: String,
        actual_field_key: String,
    },

    #[error("workflow graph has no entry step")]
    EmptyGraph,

    #[error(
        "workflow graph exceeded {0} auto-advanced steps without reaching input, \
         approval, ticket submission, or an end step -- check for a branch cycle"
    )]
    TooManySteps(usize),
}

/// What the caller must do next after an engine call returns.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Render the current step's question to the requester and wait for
    /// `submit_answer`.
    AwaitingAnswer { field_key: String },
    /// Resolve `resolution` to a concrete approver (see the `approvals`
    /// crate) and create a `PendingApproval` row.
    AwaitingApproval {
        resolution: ApproverResolution,
        timeout_seconds: Option<i64>,
    },
    /// Render the workflow's field mapping against `answers` and dispatch a
    /// ticket through the matching connector, then call
    /// `resume_after_ticket_dispatch`.
    ReadyToSubmitTicket,
    /// Terminal: persist this as the instance's final status.
    Finished { outcome: EndOutcome },
}

/// Upper bound on steps auto-advanced in one call, guarding against a
/// malformed or maliciously authored graph (e.g. a Branch/Branch cycle)
/// hanging the request thread.
const MAX_AUTO_STEPS: usize = 256;

fn find<'g>(graph: &'g WorkflowGraph, step_id: &str) -> Result<&'g Step, EngineError> {
    graph.step(step_id).ok_or_else(|| EngineError::UnknownStep(step_id.to_string()))
}

fn kind_name(kind: &StepKind) -> &'static str {
    match kind {
        StepKind::Question { .. } => "question",
        StepKind::Branch { .. } => "branch",
        StepKind::WaitForApproval { .. } => "wait_for_approval",
        StepKind::SubmitTicket { .. } => "submit_ticket",
        StepKind::End { .. } => "end",
    }
}

fn eval_condition(condition: &Condition, answers: &Answers) -> bool {
    match condition {
        Condition::Equals { field_key, value } => answers.get(field_key) == Some(value),
        Condition::NotEquals { field_key, value } => answers.get(field_key) != Some(value),
        Condition::Exists { field_key } => answers.contains_key(field_key),
        Condition::In { field_key, values } => {
            answers.get(field_key).is_some_and(|v| values.contains(v))
        }
        Condition::And { conditions } => conditions.iter().all(|c| eval_condition(c, answers)),
        Condition::Or { conditions } => conditions.iter().any(|c| eval_condition(c, answers)),
        Condition::Not { condition } => !eval_condition(condition, answers),
    }
}

/// Walks forward from `step_id`, silently resolving any `Branch` nodes,
/// until it reaches a step that needs external input/action (`Question`,
/// `WaitForApproval`, `SubmitTicket`) or an `End`.
fn auto_advance(graph: &WorkflowGraph, mut step_id: String, answers: &Answers) -> Result<(String, Outcome), EngineError> {
    for _ in 0..MAX_AUTO_STEPS {
        let step = find(graph, &step_id)?;
        match &step.kind {
            StepKind::Question { field_key, .. } => {
                return Ok((step_id, Outcome::AwaitingAnswer { field_key: field_key.clone() }));
            }
            StepKind::Branch { condition, on_true, on_false } => {
                step_id = if eval_condition(condition, answers) { on_true.clone() } else { on_false.clone() };
            }
            StepKind::WaitForApproval { approver_resolution, timeout_seconds, .. } => {
                return Ok((
                    step_id,
                    Outcome::AwaitingApproval {
                        resolution: approver_resolution.clone(),
                        timeout_seconds: *timeout_seconds,
                    },
                ));
            }
            StepKind::SubmitTicket { .. } => {
                return Ok((step_id, Outcome::ReadyToSubmitTicket));
            }
            StepKind::End { outcome } => {
                return Ok((step_id, Outcome::Finished { outcome: *outcome }));
            }
        }
    }
    Err(EngineError::TooManySteps(MAX_AUTO_STEPS))
}

/// Begins a new instance at the graph's entry step.
pub fn start(graph: &WorkflowGraph) -> Result<(String, Outcome), EngineError> {
    if graph.step(&graph.entry_step_id).is_none() {
        return Err(EngineError::EmptyGraph);
    }
    auto_advance(graph, graph.entry_step_id.clone(), &Answers::new())
}

/// Records an answer for the current `Question` step and advances.
pub fn submit_answer(
    graph: &WorkflowGraph,
    current_step_id: &str,
    answers: &mut Answers,
    field_key: &str,
    value: JsonValue,
) -> Result<(String, Outcome), EngineError> {
    let step = find(graph, current_step_id)?;
    let StepKind::Question { field_key: expected_field_key, next, .. } = &step.kind else {
        return Err(EngineError::WrongStepKind {
            step_id: current_step_id.to_string(),
            expected: "question",
            found: kind_name(&step.kind),
        });
    };
    if field_key != expected_field_key {
        return Err(EngineError::UnexpectedField {
            step_id: current_step_id.to_string(),
            expected_field_key: expected_field_key.clone(),
            actual_field_key: field_key.to_string(),
        });
    }

    answers.insert(field_key.to_string(), value);
    auto_advance(graph, next.clone(), answers)
}

/// Advances past a `WaitForApproval` step once an approver has decided.
pub fn resume_after_approval(
    graph: &WorkflowGraph,
    current_step_id: &str,
    answers: &Answers,
    decision: ApprovalDecision,
) -> Result<(String, Outcome), EngineError> {
    let step = find(graph, current_step_id)?;
    let StepKind::WaitForApproval { on_approve, on_reject, .. } = &step.kind else {
        return Err(EngineError::WrongStepKind {
            step_id: current_step_id.to_string(),
            expected: "wait_for_approval",
            found: kind_name(&step.kind),
        });
    };

    let next = match decision {
        ApprovalDecision::Approve => on_approve.clone(),
        ApprovalDecision::Reject => on_reject.clone(),
    };
    auto_advance(graph, next, answers)
}

/// Advances past a `SubmitTicket` step once the connector dispatch has been
/// (successfully) attempted by the caller.
pub fn resume_after_ticket_dispatch(
    graph: &WorkflowGraph,
    current_step_id: &str,
    answers: &Answers,
) -> Result<(String, Outcome), EngineError> {
    let step = find(graph, current_step_id)?;
    let StepKind::SubmitTicket { next } = &step.kind else {
        return Err(EngineError::WrongStepKind {
            step_id: current_step_id.to_string(),
            expected: "submit_ticket",
            found: kind_name(&step.kind),
        });
    };
    auto_advance(graph, next.clone(), answers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use servcat_model::InputType;

    fn graph_with_branch() -> WorkflowGraph {
        WorkflowGraph {
            entry_step_id: "ask_urgent".into(),
            steps: vec![
                Step {
                    id: "ask_urgent".into(),
                    label: "Is this urgent?".into(),
                    kind: StepKind::Question {
                        field_key: "urgent".into(),
                        input_type: InputType::Boolean,
                        required: true,
                        options: vec![],
                        next: "branch_urgent".into(),
                    },
                },
                Step {
                    id: "branch_urgent".into(),
                    label: "Route by urgency".into(),
                    kind: StepKind::Branch {
                        condition: Condition::Equals { field_key: "urgent".into(), value: JsonValue::Bool(true) },
                        on_true: "submit".into(),
                        on_false: "end_ok".into(),
                    },
                },
                Step {
                    id: "submit".into(),
                    label: "Submit ticket".into(),
                    kind: StepKind::SubmitTicket { next: "end_ok".into() },
                },
                Step {
                    id: "end_ok".into(),
                    label: "Done".into(),
                    kind: StepKind::End { outcome: EndOutcome::Completed },
                },
            ],
        }
    }

    #[test]
    fn starts_at_entry_question() {
        let graph = graph_with_branch();
        let (step_id, outcome) = start(&graph).unwrap();
        assert_eq!(step_id, "ask_urgent");
        assert_eq!(outcome, Outcome::AwaitingAnswer { field_key: "urgent".into() });
    }

    #[test]
    fn branch_true_reaches_submit_ticket() {
        let graph = graph_with_branch();
        let mut answers = Answers::new();
        let (step_id, outcome) =
            submit_answer(&graph, "ask_urgent", &mut answers, "urgent", JsonValue::Bool(true)).unwrap();
        assert_eq!(step_id, "submit");
        assert_eq!(outcome, Outcome::ReadyToSubmitTicket);
    }

    #[test]
    fn branch_false_skips_ticket_and_finishes() {
        let graph = graph_with_branch();
        let mut answers = Answers::new();
        let (step_id, outcome) =
            submit_answer(&graph, "ask_urgent", &mut answers, "urgent", JsonValue::Bool(false)).unwrap();
        assert_eq!(step_id, "end_ok");
        assert_eq!(outcome, Outcome::Finished { outcome: EndOutcome::Completed });
    }

    #[test]
    fn wrong_field_key_is_rejected() {
        let graph = graph_with_branch();
        let mut answers = Answers::new();
        let err = submit_answer(&graph, "ask_urgent", &mut answers, "not_urgent", JsonValue::Bool(true)).unwrap_err();
        assert!(matches!(err, EngineError::UnexpectedField { .. }));
    }

    #[test]
    fn detects_branch_cycles() {
        let graph = WorkflowGraph {
            entry_step_id: "a".into(),
            steps: vec![
                Step {
                    id: "a".into(),
                    label: "A".into(),
                    kind: StepKind::Branch {
                        condition: Condition::Exists { field_key: "never".into() },
                        on_true: "b".into(),
                        on_false: "b".into(),
                    },
                },
                Step {
                    id: "b".into(),
                    label: "B".into(),
                    kind: StepKind::Branch {
                        condition: Condition::Exists { field_key: "never".into() },
                        on_true: "a".into(),
                        on_false: "a".into(),
                    },
                },
            ],
        };
        let err = start(&graph).unwrap_err();
        assert!(matches!(err, EngineError::TooManySteps(_)));
    }
}
