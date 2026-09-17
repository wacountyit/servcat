//! Bridges the JSON API's bearer-JWT auth (see `crate::auth`) to a normal
//! browser session for the server-rendered web UI: an access token and a
//! refresh token, both minted by the exact same `crate::auth` functions the
//! JSON API uses, just carried in httponly cookies instead of a bearer
//! header/response body. `require_session` is the only place that reads or
//! writes those cookies; everything downstream just extracts `WebUser`.

use axum::{
    extract::{FromRequestParts, Request, State},
    http::{HeaderValue, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use servcat_db::repositories::users;
use servcat_model::User;

use crate::{auth, state::AppState};

pub const ACCESS_COOKIE: &str = "sc_at";
pub const REFRESH_COOKIE: &str = "sc_rt";

/// The signed-in user for a web-UI page handler. Only resolvable on routes
/// behind the `require_session` middleware, which is what actually
/// populates the request extension this reads.
#[derive(Clone)]
pub struct WebUser(pub User);

impl<S> FromRequestParts<S> for WebUser
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<User>()
            .cloned()
            .map(WebUser)
            .ok_or_else(|| Redirect::to("/login").into_response())
    }
}

/// Validates the access-token cookie, transparently minting a fresh token
/// pair from the refresh-token cookie if it's missing/expired, and redirects
/// to `/login` (preserving the original path as `?next=`) if neither is
/// valid. On success the resolved `User` is stashed in the request
/// extensions for `WebUser`/page handlers to read.
pub async fn require_session(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let jar = CookieJar::from_headers(req.headers());

    if let Some(cookie) = jar.get(ACCESS_COOKIE)
        && let Some(user) = valid_user_for_access_token(&state, cookie.value()).await
    {
        req.extensions_mut().insert(user);
        return next.run(req).await;
    }

    if let Some(cookie) = jar.get(REFRESH_COOKIE)
        && let Ok(user_id) = auth::redeem_refresh_token(&state.pool, cookie.value()).await
        && let Ok(Some(user)) = users::get_by_id(&state.pool, user_id).await
        && user.is_active
        && let Ok((access_token, refresh_token)) = auth::issue_token_pair(&state, &user).await
    {
        req.extensions_mut().insert(user);
        let mut res = next.run(req).await;
        set_session_cookies(&state, &mut res, &access_token, &refresh_token);
        return res;
    }

    let next_path = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/");
    Redirect::to(&format!("/login?next={}", urlencoding_encode(next_path))).into_response()
}

async fn valid_user_for_access_token(state: &AppState, token: &str) -> Option<User> {
    let (user_id, _role) = auth::verify_access_token(&state.config, token).ok()?;
    let user = users::get_by_id(&state.pool, user_id).await.ok()??;
    user.is_active.then_some(user)
}

/// Best-effort lookup used by pages (like `/login`) that want to know
/// whether a visitor is already signed in without redirecting/erroring if
/// not -- unlike `require_session`, this never mints a fresh token pair, so
/// it never needs to write cookies.
pub async fn peek_user(state: &AppState, jar: &CookieJar) -> Option<User> {
    let cookie = jar.get(ACCESS_COOKIE)?;
    valid_user_for_access_token(state, cookie.value()).await
}

/// Sets the access/refresh cookies for a freshly-authenticated session.
pub fn set_session_cookies(
    state: &AppState,
    res: &mut Response,
    access_token: &str,
    refresh_token: &str,
) {
    let access_max_age = state.config.access_token_ttl.as_secs() as i64;
    let refresh_max_age = state.config.refresh_token_ttl.as_secs() as i64;
    push_cookie(res, ACCESS_COOKIE, access_token, access_max_age);
    push_cookie(res, REFRESH_COOKIE, refresh_token, refresh_max_age);
}

/// Clears both session cookies (used on logout).
pub fn clear_session_cookies(res: &mut Response) {
    push_cookie(res, ACCESS_COOKIE, "", 0);
    push_cookie(res, REFRESH_COOKIE, "", 0);
}

fn push_cookie(res: &mut Response, name: &'static str, value: &str, max_age_secs: i64) {
    let mut cookie = Cookie::new(name, value.to_string());
    cookie.set_http_only(true);
    cookie.set_path("/");
    cookie.set_same_site(SameSite::Lax);
    cookie.set_max_age(Some(time::Duration::seconds(max_age_secs.max(0))));
    if let Ok(value) = HeaderValue::from_str(&cookie.to_string()) {
        res.headers_mut().append(header::SET_COOKIE, value);
    }
}

/// Minimal percent-encoding for the `next` redirect query param -- only path
/// characters ever appear here (this server's own URIs), so a small manual
/// encoder avoids pulling in a whole crate for it.
fn urlencoding_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b'/'
            | b'?'
            | b'=' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
