use serde::{Deserialize, Serialize};

use crate::{Id, Timestamp};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ServiceCatalogItem {
    pub id: Id,
    pub name: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub icon: Option<String>,
    pub workflow_definition_id: Id,
    pub is_active: bool,
    pub created_by: Option<Id>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewServiceCatalogItem {
    pub name: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub icon: Option<String>,
    pub workflow_definition_id: Id,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateServiceCatalogItem {
    pub name: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub icon: Option<String>,
    pub is_active: Option<bool>,
}
