use crate::config::AppConfig;
use crate::i18n::{resolve_locale, safe_redirect_target, Catalog, Locale};
use crate::state::AppState;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect};
use axum::Form;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::Deserialize;

pub const LOCALE_COOKIE_NAME: &str = "kirin_locale";

#[derive(Deserialize)]
pub struct LocaleForm {
    pub locale: String,
}

pub fn request_locale(cfg: &AppConfig, headers: &HeaderMap, jar: &CookieJar) -> Locale {
    let cookie = jar
        .get(LOCALE_COOKIE_NAME)
        .map(|c| c.value().to_string());
    let accept = headers
        .get(axum::http::header::ACCEPT_LANGUAGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    resolve_locale(
        cookie.as_deref(),
        accept.as_deref(),
        &cfg.ui.default_locale,
    )
}

pub fn tr_value(catalog: &Catalog, locale: Locale) -> minijinja::Value {
    minijinja::Value::from_serialize(catalog.map_for_locale(locale))
}

pub async fn locale_post(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<LocaleForm>,
) -> impl IntoResponse {
    let base = state.cfg.public_base_url_normalized();
    let root_url = base.clone();
    let referer = headers
        .get(axum::http::header::REFERER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if form.locale == "en" || form.locale == "fr" {
        let cookie = Cookie::build((LOCALE_COOKIE_NAME, form.locale.clone()))
            .path("/")
            .same_site(SameSite::Lax)
            .max_age(cookie::time::Duration::days(365))
            .http_only(true)
            .build();
        let jar = jar.add(cookie);
        let target = if referer.is_empty() {
            root_url
        } else {
            safe_redirect_target(referer, &base, "/")
        };
        (jar, Redirect::to(&target)).into_response()
    } else {
        (jar, Redirect::to(&root_url)).into_response()
    }
}
