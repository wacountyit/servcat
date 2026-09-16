use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// Every handler returns this. Variants map to the HTTP status a client
/// should act on; the `Display` message is what actually reaches the
/// client, so internal error detail (SQL errors, connector response bodies,
/// stack-shaped context) is logged via `tracing` and never included here --
/// avoids leaking schema/infrastructure details to callers.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    BadRequest(String),

    #[error("authentication required")]
    Unauthorized,

    #[error("you do not have permission to do that")]
    Forbidden,

    #[error("{0}")]
    NotFound(String),

    #[error("{0}")]
    Conflict(String),

    #[error("internal error")]
    Internal,
}

impl ApiError {
    fn status(&self) -> StatusCode {
        match self {
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::Unauthorized => StatusCode::UNAUTHORIZED,
            ApiError::Forbidden => StatusCode::FORBIDDEN,
            ApiError::NotFound(_) => StatusCode::NOT_FOUND,
            ApiError::Conflict(_) => StatusCode::CONFLICT,
            ApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = %self, "internal error");
        }
        (status, Json(json!({ "error": self.to_string() }))).into_response()
    }
}

impl From<servcat_db::DbError> for ApiError {
    fn from(err: servcat_db::DbError) -> Self {
        tracing::error!(error = %err, "database error");
        match err {
            servcat_db::DbError::NotFound { entity, id } => ApiError::NotFound(format!("{entity} {id} not found")),
            _ => ApiError::Internal,
        }
    }
}

impl From<servcat_approvals::ApprovalsError> for ApiError {
    fn from(err: servcat_approvals::ApprovalsError) -> Self {
        use servcat_approvals::ApprovalsError as E;
        match err {
            E::Db(inner) => inner.into(),
            E::NotFound(id) => ApiError::NotFound(format!("approval {id} not found")),
            E::AlreadyDecided(_) => ApiError::Conflict("this approval was already decided or has expired".into()),
            E::NotTheApprover { .. } => ApiError::Forbidden,
            E::RequesterHasNoManager | E::RequesterHasNoDepartment | E::NoUserWithRoleInDepartment { .. } => {
                tracing::error!(error = %err, "approval routing misconfigured");
                ApiError::Conflict(
                    "this request can't be routed for approval -- the requester's manager/department/role \
                     setup is incomplete; contact an administrator"
                        .into(),
                )
            }
        }
    }
}

impl From<servcat_workflow_engine::EngineError> for ApiError {
    fn from(err: servcat_workflow_engine::EngineError) -> Self {
        use servcat_workflow_engine::EngineError as E;
        match err {
            E::UnexpectedField { .. } => ApiError::BadRequest(err.to_string()),
            E::WrongStepKind { .. } => ApiError::Conflict(
                "this request is not currently waiting on that kind of input".into(),
            ),
            E::UnknownStep(_) | E::EmptyGraph | E::TooManySteps(_) => {
                tracing::error!(error = %err, "workflow graph error");
                ApiError::Internal
            }
        }
    }
}

impl From<servcat_connectors::ConnectorError> for ApiError {
    fn from(err: servcat_connectors::ConnectorError) -> Self {
        tracing::error!(error = %err, "connector dispatch failed");
        ApiError::Internal
    }
}
