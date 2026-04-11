use crate::state::AppState;
use axum::extract::State;
use axum::response::Html;
use serde::Serialize;

#[derive(Serialize)]
struct AvailOpt {
    key: String,
    label: String,
}

pub async fn index_get(State(state): State<AppState>) -> Result<Html<String>, axum::response::Response> {
    let cfg = &state.cfg;
    let labels = [
        ("minute", "One minute"),
        ("hour", "One hour"),
        ("day", "One day"),
        ("week", "One week"),
        ("fortnight", "Two weeks"),
        ("month", "One month"),
        ("quarter", "Three months"),
        ("year", "One year"),
        ("none", "Unlimited"),
    ];
    let mut opts = Vec::new();
    for (k, lab) in labels {
        if cfg.availability_enabled(k) {
            opts.push(AvailOpt {
                key: k.to_string(),
                label: lab.to_string(),
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
    };
    let html = state
        .minijinja()
        .get_template("index.html")
        .map_err(|_| axum::response::Response::builder()
            .status(500)
            .body("template missing".into())
            .unwrap())?
        .render(ctx)
        .map_err(|e| axum::response::Response::builder()
            .status(500)
            .body(e.to_string().into())
            .unwrap())?;
    Ok(Html(html))
}
