use minijinja::Environment;
use std::sync::Arc;

pub struct TemplateEngine {
    inner: Arc<Environment<'static>>,
}

impl TemplateEngine {
    pub fn embedded() -> anyhow::Result<Self> {
        let mut env = Environment::new();
        const INDEX: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/index.html"));
        const DOWNLOAD: &str =
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/download.html"));
        const DELETE: &str =
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/delete_confirm.html"));
        const ADMIN: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/admin.html"));
        const ADMIN_LOGIN: &str =
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/admin_login.html"));
        const UPLOAD_RESULT: &str =
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/upload_result.html"));
        env.add_template("index.html", INDEX)?;
        env.add_template("download.html", DOWNLOAD)?;
        env.add_template("delete_confirm.html", DELETE)?;
        env.add_template("admin.html", ADMIN)?;
        env.add_template("admin_login.html", ADMIN_LOGIN)?;
        env.add_template("upload_result.html", UPLOAD_RESULT)?;
        Ok(Self {
            inner: Arc::new(env),
        })
    }

    pub fn env(&self) -> &Environment<'static> {
        &self.inner
    }
}
