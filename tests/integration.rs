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
default_locale = "en"
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
    assert!(res.status().is_success(), "upload status {}", res.status());
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
        gone_html.contains("File not found") || gone_html.contains("Fichier introuvable"),
        "expected HTML unavailable page, got: {}",
        &gone_html[..gone_html.len().min(200)]
    );
}

fn config_toml_large_multipart(data_dir: &std::path::Path, public_base: &str) -> String {
    format!(
        r#"
[server]
bind = "127.0.0.1:0"
public_base_url = "{public_base}"
data_dir = "{}"
max_body_mb = 64

[limits]
max_upload_bytes = 50000000
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
default_locale = "en"
"#,
        data_dir.display()
    )
}

#[tokio::test]
async fn multipart_upload_accepts_payload_larger_than_axum_default_limit() {
    let tmp = TempDir::new().unwrap();
    let data = tmp.path().join("data");
    std::fs::create_dir_all(&data).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let base = format!("http://127.0.0.1:{port}/");

    let cfg_path = tmp.path().join("config.toml");
    std::fs::write(&cfg_path, config_toml_large_multipart(&data, &base)).unwrap();
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
        .timeout(Duration::from_secs(120))
        .build()
        .unwrap();

    const SIZE: usize = 3 * 1024 * 1024;
    let upload_body: Vec<u8> = (0..SIZE).map(|i| (i % 251) as u8).collect();
    let file_part = reqwest::multipart::Part::bytes(upload_body.clone())
        .file_name("big.bin")
        .mime_str("application/octet-stream")
        .unwrap();
    let form = reqwest::multipart::Form::new()
        .text("time", "hour")
        .part("file", file_part);

    let res = client
        .post(format!("{base}upload"))
        .multipart(form)
        .send()
        .await
        .expect("upload request");
    assert!(res.status().is_success(), "upload status {}", res.status());
    let html = res.text().await.expect("upload body");

    let re = Regex::new(r"f/([A-Za-z0-9_-]+)\?d=1").unwrap();
    let link_id = re
        .captures(&html)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
        .expect("link id in upload result");

    let dl_url = format!("{base}f/{link_id}?d=1");
    let dl = client.get(&dl_url).send().await.expect("download");
    assert!(dl.status().is_success(), "download {}", dl.status());
    assert_eq!(dl.bytes().await.unwrap().as_ref(), upload_body.as_slice());
}

#[tokio::test]
async fn upload_complete_get_without_token_hides_delete_link() {
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

    let file_part = reqwest::multipart::Part::bytes(b"x".to_vec())
        .file_name("t.txt")
        .mime_str("text/plain")
        .unwrap();
    let form = reqwest::multipart::Form::new()
        .text("time", "hour")
        .part("file", file_part);

    let res = client
        .post(format!("{base}upload"))
        .multipart(form)
        .send()
        .await
        .expect("upload request");
    assert!(res.status().is_success(), "upload status {}", res.status());
    let html = res.text().await.expect("upload body");

    let re = Regex::new(r"f/([A-Za-z0-9_-]+)\?d=1").unwrap();
    let link_id = re
        .captures(&html)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
        .expect("link id in upload result");

    let complete_url = format!("{base}upload/complete/{link_id}");
    let page = client
        .get(&complete_url)
        .send()
        .await
        .expect("complete get");
    assert!(page.status().is_success());
    let body = page.text().await.expect("complete body");

    assert!(
        !body.contains("Remove file"),
        "delete row should be hidden without v token (link_id={link_id})"
    );
}

#[tokio::test]
async fn download_wrong_password_returns_403_html() {
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

    let upload_body = b"secret-payload";
    let file_part = reqwest::multipart::Part::bytes(upload_body.to_vec())
        .file_name("locked.txt")
        .mime_str("text/plain")
        .unwrap();
    let form = reqwest::multipart::Form::new()
        .text("time", "hour")
        .text("key", "correct-horse")
        .part("file", file_part);

    let res = client
        .post(format!("{base}upload"))
        .multipart(form)
        .send()
        .await
        .expect("upload");
    assert!(res.status().is_success(), "upload {}", res.status());
    let html = res.text().await.expect("upload body");

    let re = Regex::new(r"f/([A-Za-z0-9_-]+)\?d=1").unwrap();
    let link_id = re
        .captures(&html)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
        .expect("link id in upload result");

    let bad_form = reqwest::multipart::Form::new().text("key", "wrong");
    let denied = client
        .post(format!("{base}f/{link_id}?d=1"))
        .multipart(bad_form)
        .send()
        .await
        .expect("download with bad password");
    assert_eq!(
        denied.status(),
        reqwest::StatusCode::FORBIDDEN,
        "wrong password should be 403"
    );
    let body = denied.text().await.expect("403 body");
    assert!(
        body.contains("Wrong password") || body.contains("Mot de passe incorrect"),
        "expected HTML forbidden page, got: {}",
        &body[..body.len().min(300)]
    );
    assert!(
        (body.contains("Try again") || body.contains("Réessayer"))
            && body.contains(&format!("/f/{link_id}")),
        "expected retry link to download page"
    );
}

#[tokio::test]
async fn post_locale_sets_cookie_and_redirects_same_origin() {
    let tmp = TempDir::new().unwrap();
    let data = tmp.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
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
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let res = client
        .post(format!("{base}locale"))
        .header("Referer", format!("{base}admin"))
        .form(&[("locale", "fr")])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);
    let loc = res
        .headers()
        .get(reqwest::header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(loc, format!("{base}admin"));
    let set_cookie = res.headers().get_all(reqwest::header::SET_COOKIE);
    let joined: String = set_cookie.iter().filter_map(|h| h.to_str().ok()).collect();
    assert!(
        joined.contains("kirin_locale=fr"),
        "set-cookie headers: {:?}",
        joined
    );
}

#[tokio::test]
async fn index_french_when_cookie_fr() {
    let tmp = TempDir::new().unwrap();
    let data = tmp.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
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
    let client = reqwest::Client::new();
    let body = client
        .get(&base)
        .header(reqwest::header::COOKIE, "kirin_locale=fr")
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains("lang=\"fr\""));
    assert!(body.contains("Envoyer un fichier"));
}

#[tokio::test]
async fn index_japanese_when_cookie_ja() {
    let tmp = TempDir::new().unwrap();
    let data = tmp.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
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
    let client = reqwest::Client::new();
    let body = client
        .get(&base)
        .header(reqwest::header::COOKIE, "kirin_locale=ja")
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains("lang=\"ja\""));
    assert!(body.contains("ファイルをアップロード"));
}

#[tokio::test]
async fn default_locale_fr_without_cookie_or_accept_language() {
    let tmp = TempDir::new().unwrap();
    let data = tmp.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let base = format!("http://127.0.0.1:{port}/");
    let mut toml = config_toml(&data, &base);
    toml = toml.replace("default_locale = \"en\"", "default_locale = \"fr\"");
    let cfg_path = tmp.path().join("config.toml");
    std::fs::write(&cfg_path, toml).unwrap();
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
    let client = reqwest::Client::new();
    let body = client
        .get(&base)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains("lang=\"fr\""));
}

#[tokio::test]
async fn default_locale_ja_without_cookie_or_accept_language() {
    let tmp = TempDir::new().unwrap();
    let data = tmp.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let base = format!("http://127.0.0.1:{port}/");
    let mut toml = config_toml(&data, &base);
    toml = toml.replace("default_locale = \"en\"", "default_locale = \"ja\"");
    let cfg_path = tmp.path().join("config.toml");
    std::fs::write(&cfg_path, toml).unwrap();
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
    let client = reqwest::Client::new();
    let body = client
        .get(&base)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains("lang=\"ja\""));
}

#[tokio::test]
async fn post_locale_sets_cookie_for_ja() {
    let tmp = TempDir::new().unwrap();
    let data = tmp.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
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
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let res = client
        .post(format!("{base}locale"))
        .header("Referer", format!("{base}admin"))
        .form(&[("locale", "ja")])
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::SEE_OTHER);
    let set_cookie = res.headers().get_all(reqwest::header::SET_COOKIE);
    let joined: String = set_cookie.iter().filter_map(|h| h.to_str().ok()).collect();
    assert!(
        joined.contains("kirin_locale=ja"),
        "set-cookie headers: {:?}",
        joined
    );
}
