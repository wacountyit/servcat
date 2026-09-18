//! Loads `seed/starter_catalog.json` into the database: a starter set of
//! organization-neutral IT service catalog items (see that file), each
//! backed by its own `WorkflowDefinition` built from one of five reusable
//! *patterns* (`WorkflowPattern` below) rather than one bespoke workflow
//! shape per item. The pattern is what's actually reusable -- the code
//! that wires up "some questions, then this approval shape, then a
//! ticket" -- while each item still gets its own real, tailored Question
//! steps, since a single shared set of questions could never fit both e.g.
//! "New Computer Request" and "Printer Access".
//!
//! Idempotent by `slug`: an item already present (by slug) is skipped
//! entirely, whether or not it's still `is_active` -- so an admin's edits,
//! or a soft-delete via deactivation, are never overwritten by re-running
//! this. See `routes: crate::web::pages::admin::catalog` for how this is
//! exposed both automatically (main.rs, first run only) and as an admin
//! action ("Load starter catalog").

use serde::Deserialize;
use servcat_db::{
    Pool,
    repositories::{catalog, workflow_definitions},
};
use servcat_model::{
    ApproverResolution, CatalogItemDetails, EndOutcome, InputType, NewServiceCatalogItem,
    NewWorkflowDefinition, QuestionOption, Role, Step, StepKind, TargetUnit, WorkflowGraph,
};
use uuid::Uuid;

use crate::error::ApiError;

const SEED_JSON: &str = include_str!("../seed/starter_catalog.json");

#[derive(Debug, Deserialize)]
struct SeedItem {
    slug: String,
    name: String,
    category: String,
    icon: String,
    summary: String,
    description: String,
    included: Vec<String>,
    not_included: Vec<String>,
    needed: Vec<String>,
    keywords: Vec<String>,
    approval_label: String,
    workflow_pattern: WorkflowPattern,
    target_value: i32,
    target_unit: TargetUnit,
    published: bool,
    questions: Vec<SeedQuestion>,
}

/// The five reusable approval/ticket *shapes* every seeded item is built
/// from -- see `build_graph`. `InputType`/`TargetUnit` are the real model
/// types (already `snake_case`-tagged the same way the JSON needs), so
/// only this pattern name needs its own small enum here.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WorkflowPattern {
    SimpleTicket,
    ManagerApprovalTicket,
    SecurityApprovalTicket,
    FinanceApprovalTicket,
    InfoOnly,
}

#[derive(Debug, Deserialize)]
struct SeedQuestion {
    field_key: String,
    label: String,
    input_type: InputType,
    required: bool,
    #[serde(default)]
    options: Vec<SeedOption>,
}

#[derive(Debug, Deserialize)]
struct SeedOption {
    value: String,
    label: String,
}

#[derive(Debug, Default)]
pub struct SeedSummary {
    pub added: usize,
    pub skipped: usize,
    /// Names of items skipped because their slug already existed --
    /// surfaced in the admin-triggered "Load starter catalog" confirmation
    /// so it's obvious *why* the count is what it is.
    pub skipped_names: Vec<String>,
}

fn parse_seed_items() -> Vec<SeedItem> {
    serde_json::from_str(SEED_JSON).unwrap_or_else(|err| {
        panic!(
            "crates/server/seed/starter_catalog.json failed to parse against SeedItem's shape: {err}"
        )
    })
}

/// Runs the idempotent seed against `pool`, attributing every created
/// `WorkflowDefinition`/`ServiceCatalogItem` to `created_by`. Fetches every
/// existing item's slug up front (one query) rather than one
/// `get_by_slug` lookup per seed item, since the whole seed file is
/// typically checked in a single call anyway.
pub async fn run(pool: &Pool, created_by: Uuid) -> Result<SeedSummary, ApiError> {
    let items = parse_seed_items();
    let existing_slugs: std::collections::HashSet<String> = catalog::list(pool, true)
        .await?
        .into_iter()
        .filter_map(|item| item.slug)
        .collect();

    let mut summary = SeedSummary::default();

    for (index, item) in items.iter().enumerate() {
        if existing_slugs.contains(&item.slug) {
            summary.skipped += 1;
            summary.skipped_names.push(item.name.clone());
            continue;
        }

        let graph = build_graph(&item.workflow_pattern, &item.questions);
        let workflow = workflow_definitions::create(
            pool,
            &NewWorkflowDefinition {
                name: format!("{} (starter catalog)", item.name),
                graph,
                field_mapping: None,
                target_system: None,
            },
            created_by,
        )
        .await?;
        // Published, not left as a draft: these are meant to be usable the
        // moment they're seeded, same as the catalog item itself (unless
        // the item is one of the optional, seeded-as-draft packs below).
        workflow_definitions::set_published(pool, workflow.id, true).await?;

        let created = catalog::create(
            pool,
            &NewServiceCatalogItem {
                slug: Some(item.slug.clone()),
                name: item.name.clone(),
                description: Some(item.description.clone()),
                summary: Some(item.summary.clone()),
                details: Some(CatalogItemDetails {
                    included: item.included.clone(),
                    not_included: item.not_included.clone(),
                    needed: item.needed.clone(),
                    keywords: item.keywords.clone(),
                }),
                approval_label: Some(item.approval_label.clone()),
                target_value: Some(item.target_value),
                target_unit: Some(item.target_unit),
                category: Some(item.category.clone()),
                icon: Some(item.icon.clone()),
                sort_order: index as i32,
                workflow_definition_id: workflow.id,
            },
            created_by,
        )
        .await?;

        if !item.published {
            catalog::deactivate(pool, created.id).await?;
        }
        summary.added += 1;
    }

    Ok(summary)
}

/// True once every item in the seed file has already been loaded (by
/// slug) -- used to skip the automatic first-run seed's own "is the
/// catalog empty" check from re-parsing/re-running needlessly, and by
/// admin/catalog.html to decide whether "Load starter catalog" has
/// anything left to do.
pub fn seed_item_count() -> usize {
    parse_seed_items().len()
}

/// Builds the step graph for one of the five reusable patterns. Every
/// pattern starts with the item's own Question steps (in the order given),
/// then branches into whichever approval/ticket tail that pattern needs.
/// `entry_step_id` is simply the first step actually produced, so this
/// still does the right thing in the (seed data shouldn't ever produce
/// this) case of an item with zero questions.
fn build_graph(pattern: &WorkflowPattern, questions: &[SeedQuestion]) -> WorkflowGraph {
    let mut steps = Vec::new();

    match pattern {
        WorkflowPattern::SimpleTicket => {
            append_questions(&mut steps, questions, "submit");
            push_submit_and_ends(&mut steps, "submit", "end_completed", None);
        }
        WorkflowPattern::InfoOnly => {
            append_questions(&mut steps, questions, "end_completed");
            steps.push(end_step("end_completed", EndOutcome::Completed));
        }
        WorkflowPattern::ManagerApprovalTicket => {
            append_questions(&mut steps, questions, "manager_approval");
            steps.push(Step {
                id: "manager_approval".to_string(),
                label: "Manager approval".to_string(),
                kind: StepKind::WaitForApproval {
                    approver_resolution: ApproverResolution::ManagerOfRequester,
                    on_approve: "submit".to_string(),
                    on_reject: "end_rejected".to_string(),
                    timeout_seconds: None,
                },
            });
            push_submit_and_ends(&mut steps, "submit", "end_completed", Some("end_rejected"));
        }
        WorkflowPattern::SecurityApprovalTicket | WorkflowPattern::FinanceApprovalTicket => {
            let (second_id, second_label) = match pattern {
                WorkflowPattern::SecurityApprovalTicket => (
                    "security_approval",
                    "Security approval (placeholder: routes to any \"Approver\"-role user in \
                     the requester's department -- reassign this step once your organization's \
                     real security approver/team is set up)",
                ),
                _ => (
                    "finance_approval",
                    "Finance approval (placeholder: routes to any \"Approver\"-role user in the \
                     requester's department -- reassign this step once your organization's real \
                     finance approver/team is set up)",
                ),
            };
            append_questions(&mut steps, questions, "manager_approval");
            steps.push(Step {
                id: "manager_approval".to_string(),
                label: "Manager approval".to_string(),
                kind: StepKind::WaitForApproval {
                    approver_resolution: ApproverResolution::ManagerOfRequester,
                    on_approve: second_id.to_string(),
                    on_reject: "end_rejected".to_string(),
                    timeout_seconds: None,
                },
            });
            steps.push(Step {
                id: second_id.to_string(),
                label: second_label.to_string(),
                kind: StepKind::WaitForApproval {
                    approver_resolution: ApproverResolution::RoleInDepartment {
                        role: Role::Approver,
                    },
                    on_approve: "submit".to_string(),
                    on_reject: "end_rejected".to_string(),
                    timeout_seconds: None,
                },
            });
            push_submit_and_ends(&mut steps, "submit", "end_completed", Some("end_rejected"));
        }
    }

    let entry_step_id = steps
        .first()
        .map(|s| s.id.clone())
        .expect("build_graph always produces at least one step");
    WorkflowGraph {
        entry_step_id,
        steps,
    }
}

fn append_questions(steps: &mut Vec<Step>, questions: &[SeedQuestion], next_after: &str) {
    for (i, q) in questions.iter().enumerate() {
        let next = if i + 1 < questions.len() {
            format!("q{}", i + 2)
        } else {
            next_after.to_string()
        };
        steps.push(Step {
            id: format!("q{}", i + 1),
            label: q.label.clone(),
            kind: StepKind::Question {
                field_key: q.field_key.clone(),
                input_type: q.input_type,
                required: q.required,
                options: q
                    .options
                    .iter()
                    .map(|o| QuestionOption {
                        value: o.value.clone(),
                        label: o.label.clone(),
                    })
                    .collect(),
                next,
            },
        });
    }
}

fn push_submit_and_ends(
    steps: &mut Vec<Step>,
    submit_id: &str,
    completed_id: &str,
    rejected_id: Option<&str>,
) {
    steps.push(Step {
        id: submit_id.to_string(),
        label: "Submit ticket".to_string(),
        kind: StepKind::SubmitTicket {
            next: completed_id.to_string(),
        },
    });
    steps.push(end_step(completed_id, EndOutcome::Completed));
    if let Some(rejected_id) = rejected_id {
        steps.push(Step {
            id: rejected_id.to_string(),
            label: "Request not approved".to_string(),
            kind: StepKind::End {
                outcome: EndOutcome::Rejected,
            },
        });
    }
}

fn end_step(id: &str, outcome: EndOutcome) -> Step {
    Step {
        id: id.to_string(),
        label: match outcome {
            EndOutcome::Completed => "Request completed".to_string(),
            EndOutcome::Rejected => "Request not approved".to_string(),
            EndOutcome::Cancelled => "Request cancelled".to_string(),
        },
        kind: StepKind::End { outcome },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // `run`'s idempotency itself is a one-query-then-loop against a real
    // pool (see its doc comment) and isn't exercised here, since this
    // crate has no test-database fixture to spin one up against; these
    // cover the two things that would silently break it -- the seed file
    // parsing into unique, non-empty slugs, and the skip-by-slug decision
    // `run` makes from that set -- without needing a database at all.

    #[test]
    fn seed_file_parses_into_unique_non_empty_slugs() {
        let items = parse_seed_items();
        assert!(!items.is_empty());
        let mut seen = HashSet::new();
        for item in &items {
            assert!(!item.slug.is_empty(), "{} has an empty slug", item.name);
            assert!(
                seen.insert(item.slug.clone()),
                "duplicate slug '{}' would break idempotent re-seeding",
                item.slug
            );
        }
    }

    #[test]
    fn seed_item_count_matches_the_parsed_file() {
        assert_eq!(seed_item_count(), parse_seed_items().len());
    }

    #[test]
    fn slugs_already_in_the_database_are_skipped_not_readded() {
        let items = parse_seed_items();
        let already_seeded: HashSet<String> =
            items.iter().take(2).map(|item| item.slug.clone()).collect();

        let to_add: Vec<&SeedItem> = items
            .iter()
            .filter(|item| !already_seeded.contains(&item.slug))
            .collect();

        assert_eq!(to_add.len(), items.len() - already_seeded.len());
        assert!(
            to_add
                .iter()
                .all(|item| !already_seeded.contains(&item.slug))
        );
    }

    #[test]
    fn every_workflow_pattern_builds_a_graph_with_a_reachable_entry_step() {
        let questions = vec![SeedQuestion {
            field_key: "details".to_string(),
            label: "What do you need?".to_string(),
            input_type: InputType::TextArea,
            required: true,
            options: vec![],
        }];
        for pattern in [
            WorkflowPattern::SimpleTicket,
            WorkflowPattern::ManagerApprovalTicket,
            WorkflowPattern::SecurityApprovalTicket,
            WorkflowPattern::FinanceApprovalTicket,
            WorkflowPattern::InfoOnly,
        ] {
            let graph = build_graph(&pattern, &questions);
            assert!(
                graph.step(&graph.entry_step_id).is_some(),
                "{pattern:?} produced a graph whose entry step doesn't exist"
            );
        }
    }
}
