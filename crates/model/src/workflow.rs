use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::{sql_enum::sql_string_enum, Id, Role, Timestamp};

/// External systems a rendered ticket can be dispatched to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetSystem {
    Jira,
    Glpi,
    Freshservice,
    Webhook,
}

sql_string_enum!(TargetSystem {
    Jira => "jira",
    Glpi => "glpi",
    Freshservice => "freshservice",
    Webhook => "webhook",
});

/// Row stored in `workflow_definitions`. `graph` is the parsed
/// `definition_json` column; `field_mapping` is the parsed
/// `field_mapping_json` column. Kept as separate typed fields at this layer
/// (rather than raw `JsonValue`) so the engine and connectors never have to
/// re-parse or guess at shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    pub id: Id,
    pub name: String,
    pub version: i32,
    pub graph: WorkflowGraph,
    pub field_mapping: Option<FieldMapping>,
    pub target_system: Option<TargetSystem>,
    pub is_published: bool,
    pub created_by: Option<Id>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewWorkflowDefinition {
    pub name: String,
    pub graph: WorkflowGraph,
    pub field_mapping: Option<FieldMapping>,
    pub target_system: Option<TargetSystem>,
}

/// The step graph an admin authors (today via JSON, eventually via a
/// drag-and-drop builder). `entry_step_id` is where a new `WorkflowInstance`
/// starts; every `next`/`on_true`/`on_false`/`on_approve`/`on_reject` value
/// elsewhere in the graph must reference a valid step id. The engine
/// validates this at publish time, not at every step transition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowGraph {
    pub entry_step_id: String,
    pub steps: Vec<Step>,
}

impl WorkflowGraph {
    pub fn step(&self, id: &str) -> Option<&Step> {
        self.steps.iter().find(|s| s.id == id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub id: String,
    pub label: String,
    pub kind: StepKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StepKind {
    /// Collects one answer keyed by `field_key` and stores it into the
    /// instance's `answers` map before moving to `next`.
    Question {
        field_key: String,
        input_type: InputType,
        required: bool,
        #[serde(default)]
        options: Vec<QuestionOption>,
        next: String,
    },
    /// Pure control-flow node: evaluates `condition` against the instance's
    /// answers so far and branches without collecting new input.
    Branch {
        condition: Condition,
        on_true: String,
        on_false: String,
    },
    /// Pauses the instance until an approver acts. See `approvals` crate for
    /// resolution and resume logic.
    WaitForApproval {
        approver_resolution: ApproverResolution,
        on_approve: String,
        on_reject: String,
        /// Auto-expire (and treat as rejected) if untouched this long.
        timeout_seconds: Option<i64>,
    },
    /// Renders the workflow's `field_mapping` against the collected answers
    /// and hands the result to the connectors crate for dispatch.
    SubmitTicket { next: String },
    /// Terminal node.
    End { outcome: EndOutcome },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndOutcome {
    Completed,
    Rejected,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputType {
    Text,
    TextArea,
    Number,
    Boolean,
    Select,
    MultiSelect,
    Date,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionOption {
    pub value: String,
    pub label: String,
}

/// Who must act on a `WaitForApproval` step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ApproverResolution {
    /// A specific, fixed user.
    Static { user_id: Id },
    /// The requester's `manager_user_id`. Resolution fails the instance
    /// (rather than silently skipping approval) if the requester has none.
    ManagerOfRequester,
    /// Any active user holding `role` in the requester's department.
    RoleInDepartment { role: Role },
}

/// Simple boolean expression evaluated against an instance's `answers` JSON
/// object. Intentionally not a general expression language: it covers the
/// branching a catalog admin actually needs (a visual builder will only ever
/// emit these shapes), and it never executes admin-authored code.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Condition {
    Equals { field_key: String, value: JsonValue },
    NotEquals { field_key: String, value: JsonValue },
    Exists { field_key: String },
    In { field_key: String, values: Vec<JsonValue> },
    And { conditions: Vec<Condition> },
    Or { conditions: Vec<Condition> },
    Not { condition: Box<Condition> },
}

/// Maps a target ticketing system's field name to a Tera template string,
/// rendered at dispatch time with the instance's answers, requester profile
/// and instance metadata in scope. Stored as `field_mapping_json`; letting an
/// org reconfigure e.g. their Jira project key/issue type/priority scheme
/// without a code change or redeploy.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FieldMapping(pub HashMap<String, String>);

impl FieldMapping {
    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.0.iter()
    }
}
