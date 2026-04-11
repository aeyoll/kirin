//! Upload, download, and delete flow against a live local server.

use kirin::app::create_app;
use kirin::config::AppConfig;
use regex::Regex;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::net::TcpListener;

fn config_toml(data_dir: &std::path::Path, public_base: &str) -> String {
    format!(
        r#"
[server]
bind = "127.0.0.1:0"
public_base_url = "{public_base}"
data_dir = "{}"
max_body_mb = 32

[limits]
max_upload_bytes = 1048576
link_id_length = 8

[upload_auth]
passwords = []

[availabilities]
minute = true
hour = true
day = true
week = true
fortnight = true
month = true
quarter = false
year = false
none = false
default = "hour"

[features]
one_time_download = true
one_time_download_preselected = false
preview = true
download_password_requirement = "optional"

[admin]
password_sha256_hex = ""
session_signing_key_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
allowed_admin_ips = []

[ui]
title = "Test"
organisation = ""
"#,
        data_dir.display()
    )
}

#[tokio::test]
async fn upload_download_delete_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let data = tmp.path().join("data");
    std::fs::create_dir_all(&data).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let port = addr.port();
    let base = format!("http://127.0.0.1:{port}/");

    let cfg_path = tmp.path().join("config.toml");
    std::fs::write(&cfg_path, config_toml(&data, &base)).unwrap();
    let cfg = Arc::new(AppConfig::load_path(&cfg_path).unwrap());
    let app = create_app(cfg).unwrap();

    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .expect("server");
    });

    tokio::time::sleep(Duration::from_millis(80)).await;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();

    let upload_body = b"roundtrip-bytes";
    let file_part = reqwest::multipart::Part::bytes(upload_body.to_vec())
        .file_name("note.txt")
        .mime_str("text/plain")
        .unwrap();
    let form = reqwest::multipart::Form::new()
        .text("time", "hour")
        .part("file", file_part);

    let upload_url = format!("{base}upload");
    let res = client
        .post(&upload_url)
        .multipart(form)
        .send()
        .await
        .expect("upload request");
    assert!(
        res.status().is_success(),
        "upload status {}",
        res.status()
    );
    let html = res.text().await.expect("upload body");

    let re = Regex::new(r"f/([A-Za-z0-9_-]+)\?d=([A-Za-z0-9_-]+)").unwrap();
    let mut link_id: Option<String> = None;
    let mut delete_code: Option<String> = None;
    for cap in re.captures_iter(&html) {
        let id = cap.get(1).unwrap().as_str().to_string();
        let d = cap.get(2).unwrap().as_str();
        if d == "1" {
            link_id = Some(id);
        } else {
            delete_code = Some(d.to_string());
        }
    }
    let link_id = link_id.expect("link id in upload result");
    let delete_code = delete_code.expect("delete code in upload result");

    let dl_url = format!("{base}f/{link_id}?d=1");
    let dl = client.get(&dl_url).send().await.expect("download");
    assert!(dl.status().is_success(), "download {}", dl.status());
    assert_eq!(dl.bytes().await.unwrap().as_ref(), upload_body);

    let del_url = format!("{base}f/{link_id}?d={delete_code}");
    let del_form = reqwest::multipart::Form::new().text("confirm", "1");
    let del = client
        .post(&del_url)
        .multipart(del_form)
        .send()
        .await
        .expect("delete");
    assert!(del.status().is_success(), "delete {}", del.status());

    let gone = client.get(&dl_url).send().await.expect("after delete");
    assert_eq!(gone.status(), reqwest::StatusCode::NOT_FOUND);
    let gone_html = gone.text().await.expect("after delete body");
    assert!(
        gone_html.contains("File not found"),
        "expected HTML unavailable page, got: {}",
        &gone_html[..gone_html.len().min(200)]
    );
}
