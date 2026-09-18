use serde::{Deserialize, Serialize};

use crate::{Id, Timestamp, sql_enum::sql_string_enum};

/// Unit for a catalog item's `target_value` -- a *target*, not a
/// guarantee, for how long fulfilling a request typically takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetUnit {
    Minutes,
    Hours,
    BusinessDays,
    CalendarDays,
}

sql_string_enum!(TargetUnit {
    Minutes => "minutes",
    Hours => "hours",
    BusinessDays => "business_days",
    CalendarDays => "calendar_days",
});

impl TargetUnit {
    /// Singular/plural noun, e.g. "hour"/"hours" -- see
    /// `target_time::format_target` for the full "3 business days"
    /// formatting this feeds into.
    pub fn noun(&self, value: i32) -> &'static str {
        match (self, value == 1) {
            (TargetUnit::Minutes, true) => "minute",
            (TargetUnit::Minutes, false) => "minutes",
            (TargetUnit::Hours, true) => "hour",
            (TargetUnit::Hours, false) => "hours",
            (TargetUnit::BusinessDays, true) => "business day",
            (TargetUnit::BusinessDays, false) => "business days",
            (TargetUnit::CalendarDays, true) => "day",
            (TargetUnit::CalendarDays, false) => "days",
        }
    }
}

/// Structured content for a catalog item beyond its `description` --
/// stored as `details_json`, parsed the same way `workflow_definitions`
/// already parses `definition_json`/`field_mapping_json` into typed Rust
/// values rather than leaving them as loosely-typed JSON everywhere else
/// in the codebase.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CatalogItemDetails {
    /// What's included in this service.
    #[serde(default)]
    pub included: Vec<String>,
    /// What's *not* included, or when to use something else instead.
    #[serde(default)]
    pub not_included: Vec<String>,
    /// What the requester should have ready before starting -- shown as a
    /// checklist, and should roughly match the workflow's own Question
    /// steps (see `catalog_seed` for how the starter catalog builds both
    /// from the same source data).
    #[serde(default)]
    pub needed: Vec<String>,
    /// Search aliases (e.g. "locked out", "forgot password") so the
    /// catalog's search box finds an item under terms an employee would
    /// actually type, not just its formal name.
    #[serde(default)]
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ServiceCatalogItem {
    pub id: Id,
    /// Stable key for idempotent seeding (see the server crate's
    /// `catalog_seed` module) -- `None` for an item an admin created by
    /// hand through the UI, which was never seeded from a file and has no
    /// reason to need one.
    pub slug: Option<String>,
    pub name: String,
    pub description: Option<String>,
    /// One sentence for the catalog card; `description` is the longer,
    /// 2-4 sentence version shown on the item's detail page.
    pub summary: Option<String>,
    pub details_json: Option<sqlx::types::Json<CatalogItemDetails>>,
    /// Display-only label for the catalog card/detail page (e.g. "Manager
    /// approval"). This does not drive actual approval behavior -- that
    /// comes entirely from the linked `WorkflowDefinition`'s own
    /// `WaitForApproval` steps -- it's just what a requester sees before
    /// they start, so keep it in sync with the workflow by hand if you
    /// change one without the other.
    pub approval_label: Option<String>,
    pub target_value: Option<i32>,
    pub target_unit: Option<TargetUnit>,
    pub category: Option<String>,
    pub icon: Option<String>,
    /// Lower sorts first within a category; ties break on `name`.
    pub sort_order: i32,
    pub workflow_definition_id: Id,
    pub is_active: bool,
    pub created_by: Option<Id>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl ServiceCatalogItem {
    /// "3 business days", or `None` if this item has no target set at all
    /// (both fields are optional so an admin-created item isn't forced to
    /// pick one).
    pub fn target_display(&self) -> Option<String> {
        match (self.target_value, self.target_unit) {
            (Some(value), Some(unit)) => Some(crate::target_time::format_target(value, unit)),
            _ => None,
        }
    }

    /// A `CatalogItemDetails` to render even when `details_json` is unset
    /// (an admin-created item that predates -- or never bothered with --
    /// the structured included/not_included/needed/keywords content),
    /// rather than every caller needing its own `unwrap_or_default`.
    pub fn details(&self) -> CatalogItemDetails {
        self.details_json
            .as_ref()
            .map(|json| json.0.clone())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewServiceCatalogItem {
    pub slug: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub summary: Option<String>,
    pub details: Option<CatalogItemDetails>,
    pub approval_label: Option<String>,
    pub target_value: Option<i32>,
    pub target_unit: Option<TargetUnit>,
    pub category: Option<String>,
    pub icon: Option<String>,
    #[serde(default)]
    pub sort_order: i32,
    pub workflow_definition_id: Id,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateServiceCatalogItem {
    pub name: Option<String>,
    pub description: Option<String>,
    pub summary: Option<String>,
    pub details: Option<CatalogItemDetails>,
    pub approval_label: Option<String>,
    pub target_value: Option<i32>,
    pub target_unit: Option<TargetUnit>,
    pub category: Option<String>,
    pub icon: Option<String>,
    pub sort_order: Option<i32>,
    pub is_active: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // format_target itself (pluralization, the 24-hour-to-days collapse,
    // etc.) is tested directly in `target_time`; these only cover the
    // `ServiceCatalogItem` methods added here.

    #[test]
    fn target_display_is_none_without_both_fields() {
        let item = sample_item(None, None);
        assert_eq!(item.target_display(), None);

        let item = sample_item(Some(3), None);
        assert_eq!(item.target_display(), None);

        let item = sample_item(Some(3), Some(TargetUnit::BusinessDays));
        assert_eq!(item.target_display(), Some("3 business days".to_string()));
    }

    #[test]
    fn details_defaults_when_unset() {
        let item = sample_item(None, None);
        let details = item.details();
        assert!(details.included.is_empty());
        assert!(details.keywords.is_empty());
    }

    fn sample_item(
        target_value: Option<i32>,
        target_unit: Option<TargetUnit>,
    ) -> ServiceCatalogItem {
        let now = chrono::Utc::now();
        ServiceCatalogItem {
            id: uuid::Uuid::nil(),
            slug: None,
            name: "Test item".to_string(),
            description: None,
            summary: None,
            details_json: None,
            approval_label: None,
            target_value,
            target_unit,
            category: None,
            icon: None,
            sort_order: 0,
            workflow_definition_id: uuid::Uuid::nil(),
            is_active: true,
            created_by: None,
            created_at: now,
            updated_at: now,
        }
    }
}
