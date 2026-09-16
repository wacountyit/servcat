use servcat_model::Role;

#[derive(Debug, thiserror::Error)]
pub enum ApprovalsError {
    #[error("database error: {0}")]
    Db(#[from] servcat_db::DbError),

    #[error("cannot resolve an approver: requester has no manager on file")]
    RequesterHasNoManager,

    #[error("cannot resolve an approver: requester has no department on file")]
    RequesterHasNoDepartment,

    #[error("cannot resolve an approver: no active user holds role '{role:?}' in the requester's department")]
    NoUserWithRoleInDepartment { role: Role },

    #[error("approval {0} was already decided or has expired")]
    AlreadyDecided(uuid::Uuid),

    #[error("user {user_id} is not the approver for approval {approval_id}")]
    NotTheApprover { approval_id: uuid::Uuid, user_id: uuid::Uuid },

    #[error("approval {0} not found")]
    NotFound(uuid::Uuid),
}
