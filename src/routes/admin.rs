use crate::error::AppError;
use crate::password::verify_admin_password_hex;
use crate::routes::common::{challenge_admin_ip, human_size, valid_link_id};
use crate::routes::locale::{request_locale, tr_value};
use crate::state::AppState;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::Deserialize;
use serde::Serialize;
use std::net::SocketAddr;

#[derive(Serialize)]
struct AdminRow {
    link_id: String,
    name: String,
    size: String,
    expires: String,
}

pub async fn admin_get(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Response, AppError> {
    let ip = addr.ip().to_string();
    if !challenge_admin_ip(&state.cfg.admin.allowed_admin_ips, &ip) {
        return Err(AppError::Forbidden);
    }
    let loc = request_locale(&state.cfg, &headers, &jar);
    let tr = tr_value(&state.i18n, loc);
    if state.cfg.admin.password_sha256_hex.is_empty() {
        let html = state
            .minijinja()
            .get_template("admin_login.html")
            .map_err(|_| AppError::Internal)?
            .render(minijinja::context! {
                message => state.i18n.get(loc, "admin.disabled_message"),
                locale => loc.as_str(),
                tr => tr.clone(),
            })
            .map_err(|_| AppError::Internal)?;
        return Ok((StatusCode::OK, axum::response::Html(html)).into_response());
    }
    let valid = jar
        .get("jfr_admin")
        .map(|c| state.verify_admin_session(c.value()))
        .unwrap_or(false);
    if !valid {
        let html = state
            .minijinja()
            .get_template("admin_login.html")
            .map_err(|_| AppError::Internal)?
            .render(minijinja::context! {
                message => Option::<String>::None,
                locale => loc.as_str(),
                tr => tr.clone(),
            })
            .map_err(|_| AppError::Internal)?;
        return Ok((StatusCode::OK, axum::response::Html(html)).into_response());
    }
    let ids = state.storage.list_link_ids().await?;
    let mut rows: Vec<AdminRow> = Vec::new();
    for id in ids {
        if let Some(m) = state.storage.read_meta(&id).await? {
            let exp = match m.expires_at_unix {
                Some(t) => chrono::DateTime::from_timestamp(t, 0)
                    .map(|d| d.format("%Y-%m-%d %H:%M UTC").to_string())
                    .unwrap_or_else(|| state.i18n.get(loc, "admin.expires_unknown")),
                None => state.i18n.get(loc, "admin.expires_never"),
            };
            rows.push(AdminRow {
                link_id: m.link_id,
                name: m.original_name,
                size: human_size(m.size),
                expires: exp,
            });
        }
    }
    let rows_val = minijinja::Value::from_serialize(&rows);
    let ctx = minijinja::context! {
        rows => rows_val,
        locale => loc.as_str(),
        tr => tr,
    };
    let html = state
        .minijinja()
        .get_template("admin.html")
        .map_err(|_| AppError::Internal)?
        .render(ctx)
        .map_err(|_| AppError::Internal)?;
    Ok((StatusCode::OK, axum::response::Html(html)).into_response())
}

#[derive(Deserialize)]
pub struct AdminLoginForm {
    pub admin_password: String,
}

pub async fn admin_login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    axum::Form(form): axum::Form<AdminLoginForm>,
) -> Result<Response, AppError> {
    let ip = addr.ip().to_string();
    if !challenge_admin_ip(&state.cfg.admin.allowed_admin_ips, &ip) {
        return Err(AppError::Forbidden);
    }
    let loc = request_locale(&state.cfg, &headers, &jar);
    let tr = tr_value(&state.i18n, loc);
    if !verify_admin_password_hex(&state.cfg.admin.password_sha256_hex, &form.admin_password) {
        let html = state
            .minijinja()
            .get_template("admin_login.html")
            .map_err(|_| AppError::Internal)?
            .render(minijinja::context! {
                message => state.i18n.get(loc, "admin.invalid_password"),
                locale => loc.as_str(),
                tr => tr,
            })
            .map_err(|_| AppError::Internal)?;
        return Ok((StatusCode::UNAUTHORIZED, axum::response::Html(html)).into_response());
    }
    let exp = chrono::Utc::now().timestamp() + 12 * 3600;
    let token = state.sign_admin_session(exp);
    let cookie = Cookie::build(("jfr_admin", token))
        .path("/admin")
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(cookie::time::Duration::hours(12))
        .build();
    Ok((jar.add(cookie), Redirect::to("/admin")).into_response())
}

pub async fn admin_logout(jar: CookieJar) -> impl IntoResponse {
    let jar = jar.remove(Cookie::from("jfr_admin"));
    (jar, Redirect::to("/admin"))
}

#[derive(Deserialize)]
pub struct AdminDeleteForm {
    pub link_id: String,
}

pub async fn admin_delete(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    axum::Form(form): axum::Form<AdminDeleteForm>,
) -> Result<Response, AppError> {
    let ip = addr.ip().to_string();
    if !challenge_admin_ip(&state.cfg.admin.allowed_admin_ips, &ip) {
        return Err(AppError::Forbidden);
    }
    let valid = jar
        .get("jfr_admin")
        .map(|c| state.verify_admin_session(c.value()))
        .unwrap_or(false);
    if !valid {
        return Err(AppError::Forbidden);
    }
    if !valid_link_id(&form.link_id) {
        return Err(AppError::BadRequest("bad id".into()));
    }
    state.storage.delete_link(&form.link_id).await?;
    Ok(Redirect::to("/admin").into_response())
}
