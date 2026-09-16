use servcat_db::{Pool, repositories::users};
use servcat_model::{ApproverResolution, User};
use uuid::Uuid;

use crate::ApprovalsError;

/// Resolves a `WaitForApproval` step's `ApproverResolution` to a single
/// concrete user for `requester`.
///
/// `RoleInDepartment` can match more than one active user; this
/// implementation picks the first (alphabetically by display name, via the
/// repository's `ORDER BY`) rather than fanning a single approval step out
/// into a multi-approver queue. That keeps the `pending_approvals` schema
/// simple (one row per step instance) at the cost of not supporting "any one
/// of N approvers" -- worth revisiting if that becomes a real requirement.
pub async fn resolve_approver_user_id(
    pool: &Pool,
    requester: &User,
    resolution: &ApproverResolution,
) -> Result<Uuid, ApprovalsError> {
    match resolution {
        ApproverResolution::Static { user_id } => Ok(*user_id),
        ApproverResolution::ManagerOfRequester => requester
            .manager_user_id
            .ok_or(ApprovalsError::RequesterHasNoManager),
        ApproverResolution::RoleInDepartment { role } => {
            let department_id = requester
                .department_id
                .ok_or(ApprovalsError::RequesterHasNoDepartment)?;
            let candidates =
                users::list_active_by_role_in_department(pool, *role, department_id).await?;
            candidates
                .first()
                .map(|u| u.id)
                .ok_or(ApprovalsError::NoUserWithRoleInDepartment { role: *role })
        }
    }
}
