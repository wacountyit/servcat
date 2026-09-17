use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use serde::Deserialize;
use servcat_db::repositories::{org_settings, users};
use servcat_model::{NewUser, Role};

use crate::{
    auth::{self, TokenResponse},
    error::ApiError,
    state::AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/login", post(login))
        .route("/auth/register", post(register))
        .route("/auth/refresh", post(refresh))
        .route("/auth/logout", post(logout))
        .route("/auth/password-reset/request", post(request_password_reset))
        .route("/auth/password-reset/confirm", post(confirm_password_reset))
}

#[derive(Deserialize)]
struct LoginRequest {
    email: String,
    password: String,
}

async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<TokenResponse>, ApiError> {
    let user = auth::authenticate_local(&state.pool, &req.email, &req.password).await?;
    let (access_token, refresh_token) = auth::issue_token_pair(&state, &user).await?;
    Ok(Json(TokenResponse {
        access_token,
        refresh_token,
        user: user.into(),
    }))
}

#[derive(Deserialize)]
struct RegisterRequest {
    email: String,
    display_name: String,
    password: String,
}

/// Local email/password self-registration. Disabled by default -- an org
/// running on SSO can leave this permanently off and never expose it in the
/// frontend at all; `GET /config`'s `allow_local_signup` is what the
/// frontend should check before even showing the option, but this endpoint
/// enforces the same flag independently so it can't be reached by calling
/// the API directly while the toggle is off.
async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<TokenResponse>, ApiError> {
    let settings = org_settings::get(&state.pool).await?;
    if !settings.allow_local_signup {
        return Err(ApiError::Forbidden);
    }
    if req.password.len() < 12 {
        return Err(ApiError::BadRequest(
            "password must be at least 12 characters".into(),
        ));
    }
    if users::get_by_email(&state.pool, &req.email)
        .await?
        .is_some()
    {
        return Err(ApiError::Conflict(
            "an account with that email already exists".into(),
        ));
    }

    let password_hash = auth::hash_password(&req.password)?;
    let user = users::create(
        &state.pool,
        &NewUser {
            email: req.email,
            display_name: req.display_name,
            password: None,
            external_idp_subject: None,
            department_id: None,
            manager_user_id: None,
            role: Role::Requester,
        },
        Some(&password_hash),
    )
    .await?;

    let (access_token, refresh_token) = auth::issue_token_pair(&state, &user).await?;
    Ok(Json(TokenResponse {
        access_token,
        refresh_token,
        user: user.into(),
    }))
}

#[derive(Deserialize)]
struct RefreshRequest {
    refresh_token: String,
}

async fn refresh(
    State(state): State<AppState>,
    Json(req): Json<RefreshRequest>,
) -> Result<Json<TokenResponse>, ApiError> {
    let user_id = auth::redeem_refresh_token(&state.pool, &req.refresh_token).await?;
    let user = users::get_by_id(&state.pool, user_id)
        .await?
        .filter(|u| u.is_active)
        .ok_or(ApiError::Unauthorized)?;

    let (access_token, refresh_token) = auth::issue_token_pair(&state, &user).await?;
    Ok(Json(TokenResponse {
        access_token,
        refresh_token,
        user: user.into(),
    }))
}

#[derive(Deserialize)]
struct LogoutRequest {
    refresh_token: String,
}

async fn logout(
    State(state): State<AppState>,
    Json(req): Json<LogoutRequest>,
) -> Result<(), ApiError> {
    // Best-effort: an already-expired/invalid token still "succeeds" from
    // the client's point of view, since the end state (not logged in with
    // that token) is the same either way.
    let _ = auth::redeem_refresh_token(&state.pool, &req.refresh_token).await;
    Ok(())
}

#[derive(Deserialize)]
struct PasswordResetRequest {
    email: String,
}

/// Always responds 202 regardless of whether `email` has an account, is
/// SSO-only, or is deactivated -- see `auth::send_password_reset_email` for
/// why. Responds 404 instead if no SMTP mailer is configured at all, since
/// self-service reset is then a deployment-wide unavailable feature, not a
/// per-request secret.
async fn request_password_reset(
    State(state): State<AppState>,
    Json(req): Json<PasswordResetRequest>,
) -> Result<StatusCode, ApiError> {
    let mailer = state.mailer.as_deref().ok_or_else(|| {
        ApiError::NotFound("password reset is not enabled on this deployment".into())
    })?;
    auth::send_password_reset_email(&state.pool, mailer, &req.email).await?;
    Ok(StatusCode::ACCEPTED)
}

#[derive(Deserialize)]
struct PasswordResetConfirm {
    token: String,
    new_password: String,
}

async fn confirm_password_reset(
    State(state): State<AppState>,
    Json(req): Json<PasswordResetConfirm>,
) -> Result<(), ApiError> {
    if req.new_password.len() < 12 {
        return Err(ApiError::BadRequest(
            "password must be at least 12 characters".into(),
        ));
    }

    let user_id = auth::redeem_password_reset_token(&state.pool, &req.token).await?;
    let password_hash = auth::hash_password(&req.new_password)?;
    users::set_password_hash(&state.pool, user_id, &password_hash).await?;
    // A password reset should also sign out anyone using the old (possibly
    // compromised) password -- same reasoning as deactivating a user.
    auth::revoke_all_sessions(&state.pool, user_id).await?;
    Ok(())
}
