use crate::routes::locale::{request_locale, tr_value};
use crate::state::AppState;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Html;
use axum_extra::extract::cookie::CookieJar;
use serde::Serialize;

#[derive(Serialize)]
struct AvailOpt {
    key: String,
    label: String,
}

pub async fn index_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Html<String>, axum::response::Response> {
    let cfg = &state.cfg;
    let loc = request_locale(cfg, &headers, &jar);
    let tr = tr_value(&state.i18n, loc);
    let keys = [
        "minute",
        "hour",
        "day",
        "week",
        "fortnight",
        "month",
        "quarter",
        "year",
        "none",
    ];
    let mut opts = Vec::new();
    for k in keys {
        if cfg.availability_enabled(k) {
            opts.push(AvailOpt {
                key: k.to_string(),
                label: state.i18n.get(loc, &format!("avail.{k}")),
            });
        }
    }
    let availability_options = minijinja::Value::from_serialize(&opts);
    let ctx = minijinja::context! {
        title => cfg.ui.title.clone(),
        organisation => cfg.ui.organisation.clone(),
        upload_error => Option::<String>::None,
        availability_options => availability_options,
        default_availability => cfg.availabilities.default.clone(),
        one_time_enabled => cfg.features.one_time_download,
        one_time_preselected => cfg.features.one_time_download_preselected,
        download_password_optional => cfg.features.download_password_requirement == "optional",
        show_upload_password_field => !cfg.upload_auth.passwords.is_empty(),
        locale => loc.as_str(),
        tr => tr,
    };
    let html = state
        .minijinja()
        .get_template("index.html")
        .map_err(|_| {
            axum::response::Response::builder()
                .status(500)
                .body("template missing".into())
                .unwrap()
        })?
        .render(ctx)
        .map_err(|_| {
            axum::response::Response::builder()
                .status(500)
                .body("internal error".into())
                .unwrap()
        })?;
    Ok(Html(html))
}
