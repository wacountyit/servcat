use serde::{Deserialize, Serialize};

use crate::{Id, Timestamp};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Department {
    pub id: Id,
    pub name: String,
    /// Public URL to this department's seal/logo, overriding the org-wide
    /// one from `OrgSettings` wherever this department's identity is shown.
    /// Set via `POST /departments/{id}/logo`, not through `UpdateDepartment`.
    pub logo_url: Option<String>,
    pub parent_department_id: Option<Id>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewDepartment {
    pub name: String,
    pub parent_department_id: Option<Id>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateDepartment {
    pub name: Option<String>,
    pub parent_department_id: Option<Id>,
}
