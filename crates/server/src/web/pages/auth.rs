use askama::Template;
use axum::{
    Form, Router,
    extract::{Query, State},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use servcat_db::repositories::org_settings;

use crate::{
    auth,
    state::AppState,
    web::{
        render::{WebError, html},
        session,
    },
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/login", get(show_login).post(submit_login))
        .route("/login/sso/complete", get(sso_complete))
        .route("/logout", post(logout))
        .route(
            "/forgot-password",
            get(show_forgot_password).post(submit_forgot_password),
        )
        .route(
            "/reset-password",
            get(show_reset_password).post(submit_reset_password),
        )
}

#[derive(Template)]
#[template(path = "login.html")]
struct LoginTemplate {
    app_name: String,
    logo_url: Option<String>,
    sso_enabled: bool,
    /// Only true when a `Mailer` is configured -- self-service password
    /// reset needs somewhere to actually send the link.
    reset_enabled: bool,
    invalid_credentials: bool,
    reset_success: bool,
    next: String,
}

#[derive(Deserialize)]
struct LoginQuery {
    next: Option<String>,
    #[serde(default)]
    error: bool,
    #[serde(default)]
    reset: bool,
}

/// Only a same-site path is ever accepted as a post-login destination --
/// anything else (an absolute URL, a scheme-relative `//host/...`) is
/// treated the same as if `next` had been absent, so a crafted login link
/// can't be used to bounce a signed-in browser off-site.
fn safe_next(next: Option<String>) -> String {
    match next {
        Some(n) if n.starts_with('/') && !n.starts_with("//") => n,
        _ => "/".to_string(),
    }
}

async fn show_login(
    State(state): State<AppState>,
    Query(query): Query<LoginQuery>,
    jar: CookieJar,
) -> Result<Response, WebError> {
    let next = safe_next(query.next);
    if session::peek_user(&state, &jar).await.is_some() {
        return Ok(Redirect::to(&next).into_response());
    }

    let settings = org_settings::get(&state.pool).await?;

    Ok(html(LoginTemplate {
        app_name: settings.app_name,
        logo_url: settings.logo_url,
        sso_enabled: state.sso.is_some(),
        reset_enabled: state.mailer.is_some(),
        invalid_credentials: query.error,
        reset_success: query.reset,
        next,
    }))
}

#[derive(Deserialize)]
struct LoginForm {
    email: String,
    password: String,
    next: Option<String>,
}

async fn submit_login(State(state): State<AppState>, Form(form): Form<LoginForm>) -> Response {
    let next = safe_next(form.next);
    match auth::authenticate_local(&state.pool, &form.email, &form.password).await {
        Ok(user) => match auth::issue_token_pair(&state, &user).await {
            Ok((access_token, refresh_token)) => {
                let mut res = Redirect::to(&next).into_response();
                session::set_session_cookies(&state, &mut res, &access_token, &refresh_token);
                res
            }
            Err(_) => Redirect::to(&format!("/login?error=1&next={next}")).into_response(),
        },
        Err(_) => Redirect::to(&format!("/login?error=1&next={next}")).into_response(),
    }
}

#[derive(Deserialize)]
struct SsoCompleteQuery {
    code: String,
    next: Option<String>,
}

/// The web UI's counterpart to the JSON API's `POST /api/auth/sso/token`:
/// same one-time handoff code, redeemed the same way, just finishing with a
/// cookie + redirect instead of a token-pair response body. This is what
/// `SSO_FRONTEND_REDIRECT_URL` should point at for a browser-based deployment.
async fn sso_complete(
    State(state): State<AppState>,
    Query(query): Query<SsoCompleteQuery>,
) -> Response {
    let next = safe_next(query.next);
    let Some(sso) = state.sso.as_ref() else {
        return Redirect::to("/login").into_response();
    };
    let Ok(user_id) = sso.redeem_handoff_code(&query.code) else {
        return Redirect::to("/login?error=1").into_response();
    };
    let user = match servcat_db::repositories::users::get_by_id(&state.pool, user_id).await {
        Ok(Some(user)) if user.is_active => user,
        _ => return Redirect::to("/login?error=1").into_response(),
    };
    match auth::issue_token_pair(&state, &user).await {
        Ok((access_token, refresh_token)) => {
            let mut res = Redirect::to(&next).into_response();
            session::set_session_cookies(&state, &mut res, &access_token, &refresh_token);
            res
        }
        Err(_) => Redirect::to("/login?error=1").into_response(),
    }
}

async fn logout(State(state): State<AppState>, jar: CookieJar) -> Response {
    if let Some(cookie) = jar.get(session::REFRESH_COOKIE) {
        let _ = auth::redeem_refresh_token(&state.pool, cookie.value()).await;
    }
    let mut res = Redirect::to("/login").into_response();
    session::clear_session_cookies(&mut res);
    res
}

#[derive(Template)]
#[template(path = "forgot_password.html")]
struct ForgotPasswordTemplate {
    app_name: String,
    logo_url: Option<String>,
    submitted: bool,
}

/// Redirects to `/login` instead of rendering when no `Mailer` is
/// configured, same as `reset_enabled` hiding the link on the login page --
/// self-service reset genuinely isn't available on this deployment, not
/// just unadvertised.
async fn show_forgot_password(State(state): State<AppState>) -> Result<Response, WebError> {
    if state.mailer.is_none() {
        return Ok(Redirect::to("/login").into_response());
    }

    let settings = org_settings::get(&state.pool).await?;
    Ok(html(ForgotPasswordTemplate {
        app_name: settings.app_name,
        logo_url: settings.logo_url,
        submitted: false,
    }))
}

#[derive(Deserialize)]
struct ForgotPasswordForm {
    email: String,
}

async fn submit_forgot_password(
    State(state): State<AppState>,
    Form(form): Form<ForgotPasswordForm>,
) -> Result<Response, WebError> {
    let Some(mailer) = state.mailer.as_deref() else {
        return Ok(Redirect::to("/login").into_response());
    };
    auth::send_password_reset_email(&state.pool, mailer, &form.email).await?;

    let settings = org_settings::get(&state.pool).await?;
    Ok(html(ForgotPasswordTemplate {
        app_name: settings.app_name,
        logo_url: settings.logo_url,
        submitted: true,
    }))
}

#[derive(Template)]
#[template(path = "reset_password.html")]
struct ResetPasswordTemplate {
    app_name: String,
    logo_url: Option<String>,
    invalid_token: bool,
    token: String,
    error: Option<String>,
}

#[derive(Deserialize)]
struct ResetPasswordQuery {
    token: Option<String>,
}

/// Doesn't validate the token yet (that only happens, and single-use-burns
/// it, on `submit_reset_password`) -- an empty/missing query param is the
/// only thing checked here, just enough to show a sane page for a bare
/// `/reset-password` visit instead of an empty form.
async fn show_reset_password(
    State(state): State<AppState>,
    Query(query): Query<ResetPasswordQuery>,
) -> Result<Response, WebError> {
    let settings = org_settings::get(&state.pool).await?;
    let token = query.token.unwrap_or_default();
    Ok(html(ResetPasswordTemplate {
        app_name: settings.app_name,
        logo_url: settings.logo_url,
        invalid_token: token.is_empty(),
        token,
        error: None,
    }))
}

#[derive(Deserialize)]
struct ResetPasswordForm {
    token: String,
    new_password: String,
}

async fn submit_reset_password(
    State(state): State<AppState>,
    Form(form): Form<ResetPasswordForm>,
) -> Result<Response, WebError> {
    let settings = org_settings::get(&state.pool).await?;

    if form.new_password.len() < 12 {
        return Ok(html(ResetPasswordTemplate {
            app_name: settings.app_name,
            logo_url: settings.logo_url,
            invalid_token: false,
            token: form.token,
            error: Some("Password must be at least 12 characters.".to_string()),
        }));
    }

    let user_id = match auth::redeem_password_reset_token(&state.pool, &form.token).await {
        Ok(user_id) => user_id,
        Err(_) => {
            return Ok(html(ResetPasswordTemplate {
                app_name: settings.app_name,
                logo_url: settings.logo_url,
                invalid_token: true,
                token: form.token,
                error: None,
            }));
        }
    };

    let password_hash = auth::hash_password(&form.new_password)?;
    servcat_db::repositories::users::set_password_hash(&state.pool, user_id, &password_hash)
        .await?;
    // Same reasoning as deactivating a user: a password reset should also
    // sign out anyone still using the old (possibly compromised) password.
    auth::revoke_all_sessions(&state.pool, user_id).await?;

    Ok(Redirect::to("/login?reset=1").into_response())
}
