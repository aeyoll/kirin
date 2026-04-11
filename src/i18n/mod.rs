use axum::http::Uri;
use std::collections::HashMap;
use std::sync::Arc;
use toml::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    En,
    Fr,
    Ja,
}

impl Locale {
    pub fn as_str(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::Fr => "fr",
            Locale::Ja => "ja",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Catalog {
    en: HashMap<String, String>,
    fr: HashMap<String, String>,
    ja: HashMap<String, String>,
}

const EN_SRC: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/locales/en.toml"));
const FR_SRC: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/locales/fr.toml"));
const JA_SRC: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/locales/ja.toml"));

impl Catalog {
    pub fn embedded() -> anyhow::Result<Arc<Self>> {
        let en = flatten_toml("", &EN_SRC.parse::<Value>()?)?;
        let fr = flatten_toml("", &FR_SRC.parse::<Value>()?)?;
        let ja = flatten_toml("", &JA_SRC.parse::<Value>()?)?;
        Ok(Arc::new(Self { en, fr, ja }))
    }

    pub fn get(&self, locale: Locale, key: &str) -> String {
        let primary = match locale {
            Locale::En => &self.en,
            Locale::Fr => &self.fr,
            Locale::Ja => &self.ja,
        };
        if let Some(v) = primary.get(key) {
            return v.clone();
        }
        if locale != Locale::En {
            if let Some(v) = self.en.get(key) {
                tracing::debug!(key, locale = ?locale, "i18n fallback to en");
                return v.clone();
            }
        }
        key.to_string()
    }

    pub fn map_for_locale(&self, locale: Locale) -> HashMap<String, String> {
        match locale {
            Locale::En => self.en.clone(),
            Locale::Fr => self
                .en
                .keys()
                .map(|k| (k.clone(), self.get(Locale::Fr, k)))
                .collect(),
            Locale::Ja => self
                .en
                .keys()
                .map(|k| (k.clone(), self.get(Locale::Ja, k)))
                .collect(),
        }
    }
}

fn flatten_toml(prefix: &str, v: &Value) -> anyhow::Result<HashMap<String, String>> {
    let mut out = HashMap::new();
    match v {
        Value::Table(t) => {
            for (k, child) in t {
                let p = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                match child {
                    Value::String(s) => {
                        out.insert(p, s.clone());
                    }
                    Value::Table(_) => {
                        out.extend(flatten_toml(&p, child)?);
                    }
                    _ => {}
                }
            }
        }
        Value::String(s) if !prefix.is_empty() => {
            out.insert(prefix.to_string(), s.clone());
        }
        _ => {}
    }
    Ok(out)
}

pub fn resolve_locale(
    cookie_raw: Option<&str>,
    accept_language: Option<&str>,
    default: &str,
) -> Locale {
    if let Some(c) = cookie_raw {
        if c == "en" {
            return Locale::En;
        }
        if c == "fr" {
            return Locale::Fr;
        }
        if c == "ja" {
            return Locale::Ja;
        }
    }
    if let Some(header) = accept_language {
        for tag in header.split(',') {
            let primary = tag
                .trim()
                .split(';')
                .next()
                .unwrap_or("")
                .split('-')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            if primary == "fr" {
                return Locale::Fr;
            }
            if primary == "en" {
                return Locale::En;
            }
            if primary == "ja" {
                return Locale::Ja;
            }
        }
    }
    match default {
        "fr" => Locale::Fr,
        "ja" => Locale::Ja,
        _ => Locale::En,
    }
}

pub fn safe_redirect_target(referer: &str, public_base: &str, root_path: &str) -> String {
    let Ok(ref_url) = referer.parse::<Uri>() else {
        return join_base_path(public_base, root_path);
    };
    let Ok(base_url) = public_base.parse::<Uri>() else {
        return join_base_path(public_base, root_path);
    };
    let (Some(r), Some(b)) = (ref_url.authority(), base_url.authority()) else {
        return join_base_path(public_base, root_path);
    };
    if r.as_str() == b.as_str() {
        referer.to_string()
    } else {
        join_base_path(public_base, root_path)
    }
}

fn join_base_path(public_base: &str, path: &str) -> String {
    let mut b = public_base.trim_end_matches('/').to_string();
    if !path.starts_with('/') {
        b.push('/');
    }
    b.push_str(path);
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fr_falls_back_to_en_for_missing_key() {
        let mut en = HashMap::new();
        en.insert("only.en".into(), "English only".into());
        let cat = Catalog {
            en,
            fr: HashMap::new(),
            ja: HashMap::new(),
        };
        assert_eq!(cat.get(Locale::Fr, "only.en"), "English only");
    }

    #[test]
    fn ja_falls_back_to_en_for_missing_key() {
        let mut en = HashMap::new();
        en.insert("only.en".into(), "English only".into());
        let cat = Catalog {
            en,
            fr: HashMap::new(),
            ja: HashMap::new(),
        };
        assert_eq!(cat.get(Locale::Ja, "only.en"), "English only");
    }

    #[test]
    fn embedded_catalog_loads() {
        let cat = Catalog::embedded().unwrap();
        assert_eq!(cat.get(Locale::En, "index.upload_heading"), "Upload a file");
        assert_eq!(
            cat.get(Locale::Fr, "index.upload_heading"),
            "Envoyer un fichier"
        );
        assert_eq!(
            cat.get(Locale::Ja, "index.upload_heading"),
            "ファイルをアップロード"
        );
    }

    #[test]
    fn resolve_default_when_no_cookie_no_header() {
        assert_eq!(resolve_locale(None, None, "en"), Locale::En);
        assert_eq!(resolve_locale(None, None, "fr"), Locale::Fr);
        assert_eq!(resolve_locale(None, None, "ja"), Locale::Ja);
    }

    #[test]
    fn resolve_fr_from_accept_language() {
        assert_eq!(
            resolve_locale(None, Some("fr-FR, en;q=0.9"), "en"),
            Locale::Fr
        );
    }

    #[test]
    fn resolve_ja_from_accept_language() {
        assert_eq!(
            resolve_locale(None, Some("ja-JP, en;q=0.9"), "en"),
            Locale::Ja
        );
    }

    #[test]
    fn cookie_overrides_accept_language() {
        assert_eq!(resolve_locale(Some("fr"), Some("en-US"), "en"), Locale::Fr);
    }

    #[test]
    fn cookie_ja_overrides_accept_language() {
        assert_eq!(resolve_locale(Some("ja"), Some("en-US"), "en"), Locale::Ja);
    }

    #[test]
    fn invalid_cookie_ignored() {
        assert_eq!(resolve_locale(Some("de"), Some("en-US"), "en"), Locale::En);
    }

    #[test]
    fn same_origin_referer_returns_referer() {
        assert_eq!(
            safe_redirect_target("http://localhost:8080/foo", "http://localhost:8080/", "/"),
            "http://localhost:8080/foo"
        );
    }

    #[test]
    fn cross_origin_referer_returns_root() {
        assert_eq!(
            safe_redirect_target("https://evil.example/foo", "http://localhost:8080/", "/"),
            "http://localhost:8080/"
        );
    }

    #[test]
    fn invalid_referer_returns_root() {
        assert_eq!(
            safe_redirect_target("not-a-url", "http://localhost:8080/", "/"),
            "http://localhost:8080/"
        );
    }
}
