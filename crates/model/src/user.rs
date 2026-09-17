use serde::{Deserialize, Serialize};

use crate::{Id, Timestamp, sql_enum::sql_string_enum};

/// Coarse-grained role used for authorization. A user can additionally be the
/// resolved approver for a specific step without holding the `approver` role
/// globally (see `ApproverResolution`); this field mainly gates admin/agent
/// surfaces of the API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Requester,
    Approver,
    Agent,
    Admin,
}

sql_string_enum!(Role {
    Requester => "requester",
    Approver => "approver",
    Agent => "agent",
    Admin => "admin",
});

impl Role {
    /// Human-readable label for UI surfaces (the web UI's nav/user badges,
    /// admin user list, etc.).
    pub fn label(&self) -> &'static str {
        match self {
            Role::Requester => "Requester",
            Role::Approver => "Approver",
            Role::Agent => "Agent",
            Role::Admin => "Admin",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: Id,
    pub email: String,
    pub display_name: String,
    /// Argon2id hash. `None` for SSO-only accounts.
    #[serde(skip_serializing)]
    pub password_hash: Option<String>,
    pub external_idp_subject: Option<String>,
    pub department_id: Option<Id>,
    pub manager_user_id: Option<Id>,
    pub role: Role,
    pub is_active: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// Safe-to-serialize projection of `User` for API responses; never carries
/// `password_hash` even by accident.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    pub id: Id,
    pub email: String,
    pub display_name: String,
    pub department_id: Option<Id>,
    pub manager_user_id: Option<Id>,
    pub role: Role,
    pub is_active: bool,
}

impl From<User> for UserProfile {
    fn from(u: User) -> Self {
        Self {
            id: u.id,
            email: u.email,
            display_name: u.display_name,
            department_id: u.department_id,
            manager_user_id: u.manager_user_id,
            role: u.role,
            is_active: u.is_active,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewUser {
    pub email: String,
    pub display_name: String,
    /// Plaintext password from the admin/registration form; hashed before
    /// it ever reaches the db crate. Omitted for SSO-provisioned accounts.
    pub password: Option<String>,
    pub external_idp_subject: Option<String>,
    pub department_id: Option<Id>,
    pub manager_user_id: Option<Id>,
    pub role: Role,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateUser {
    pub display_name: Option<String>,
    pub department_id: Option<Id>,
    pub manager_user_id: Option<Id>,
    pub role: Option<Role>,
    pub is_active: Option<bool>,
}
