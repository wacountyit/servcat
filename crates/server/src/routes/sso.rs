use axum::{
    extract::{Query, State},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use servcat_db::repositories::users;
use servcat_model::{NewUser, Role, User};

use crate::{
    auth::{self, TokenResponse},
    error::ApiError,
    sso::AuthenticatedIdentity,
    state::AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/sso/login", get(login))
        .route("/auth/sso/callback", get(callback))
        .route("/auth/sso/token", post(token))
}

async fn login(State(state): State<AppState>) -> Result<Response, ApiError> {
    let sso = state.sso.as_ref().ok_or_else(sso_not_configured)?;
    Ok(Redirect::to(&sso.begin_login()).into_response())
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

async fn callback(State(state): State<AppState>, Query(query): Query<CallbackQuery>) -> Result<Response, ApiError> {
    let sso = state.sso.as_ref().ok_or_else(sso_not_configured)?;

    if let Some(error) = query.error {
        tracing::warn!(%error, description = ?query.error_description, "Entra ID sign-in was not completed");
        return Err(ApiError::Unauthorized);
    }
    let (Some(code), Some(state_param)) = (query.code, query.state) else {
        return Err(ApiError::BadRequest("missing code/state from Entra ID".into()));
    };

    let identity = sso.complete_login(&code, &state_param).await?;
    let user = provision_or_link_user(&state, &identity).await?;
    if !user.is_active {
        return Err(ApiError::Unauthorized);
    }

    let handoff_code = sso.issue_handoff_code(user.id);
    let redirect_url = format!("{}?code={}", sso.frontend_redirect_url(), handoff_code);
    Ok(Redirect::to(&redirect_url).into_response())
}

/// Matches the Entra ID identity to a local account: by `external_idp_subject`
/// if this person has signed in before; otherwise by email, linking a
/// password-less account an admin pre-created for them (common for
/// approvers/admins provisioned ahead of their first SSO login); otherwise
/// auto-provisions a new `Requester` account. Trusting Entra ID to gate who
/// can even reach this callback (via app assignment/conditional access on
/// the tenant side) is what makes auto-provisioning safe here -- this is not
/// the same thing as the local self-registration path, which stays behind
/// `allow_local_signup`.
async fn provision_or_link_user(state: &AppState, identity: &AuthenticatedIdentity) -> Result<User, ApiError> {
    if let Some(user) = users::get_by_external_idp_subject(&state.pool, &identity.external_idp_subject).await? {
        return Ok(user);
    }

    if let Some(user) = users::get_by_email(&state.pool, &identity.email).await? {
        if user.external_idp_subject.is_some() {
            tracing::error!(email = %identity.email, "Entra ID login email matches a user already linked to a different subject");
            return Err(ApiError::Conflict("this email is already linked to a different sign-in method".into()));
        }
        return users::link_external_idp_subject(&state.pool, user.id, &identity.external_idp_subject)
            .await?
            .ok_or_else(|| ApiError::NotFound("user not found".into()));
    }

    users::create(
        &state.pool,
        &NewUser {
            email: identity.email.clone(),
            display_name: identity.display_name.clone(),
            password: None,
            external_idp_subject: Some(identity.external_idp_subject.clone()),
            department_id: None,
            manager_user_id: None,
            role: Role::Requester,
        },
        None,
    )
    .await
    .map_err(ApiError::from)
}

#[derive(Deserialize)]
struct SsoTokenRequest {
    code: String,
}

async fn token(State(state): State<AppState>, Json(req): Json<SsoTokenRequest>) -> Result<Json<TokenResponse>, ApiError> {
    let sso = state.sso.as_ref().ok_or_else(sso_not_configured)?;
    let user_id = sso.redeem_handoff_code(&req.code)?;
    let user = users::get_by_id(&state.pool, user_id).await?.filter(|u| u.is_active).ok_or(ApiError::Unauthorized)?;

    let (access_token, refresh_token) = auth::issue_token_pair(&state, &user).await?;
    Ok(Json(TokenResponse { access_token, refresh_token, user: user.into() }))
}

fn sso_not_configured() -> ApiError {
    ApiError::NotFound("single sign-on is not configured".into())
}
