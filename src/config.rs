use anyhow::{Context, ensure};
use std::env;

#[derive(Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub database_url: String,

    pub app_origin: String,
    pub cookie_secure: bool,

    pub google_client_id: String,
    pub google_client_secret: String,

    pub internal_worker_secret: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let internal_worker_secret = required("INTERNAL_WORKER_SECRET")?;

        ensure!(
            internal_worker_secret.len() >= 32
                && !internal_worker_secret.starts_with("replace_"),
            "INTERNAL_WORKER_SECRET must be a random secret of at least 32 characters"
        );

        let app_origin = required("APP_ORIGIN")?
            .trim_end_matches('/')
            .to_owned();

        let cookie_secure = env::var("COOKIE_SECURE")
            .unwrap_or_else(|_| "false".to_owned())
            .parse::<bool>()
            .context("COOKIE_SECURE must be true or false")?;

        ensure!(
            !cookie_secure || app_origin.starts_with("https://"),
            "Secure cookies require an HTTPS APP_ORIGIN"
        );

        Ok(Self {
            host: env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_owned()),
            port: env::var("PORT")
                .unwrap_or_else(|_| "8080".to_owned())
                .parse()
                .context("Invalid PORT")?,

            database_url: required("DATABASE_URL")?,
            app_origin,
            cookie_secure,

            google_client_id: required("GOOGLE_CLIENT_ID")?,
            google_client_secret: required("GOOGLE_CLIENT_SECRET")?,

            internal_worker_secret,
        })
    }

    pub fn google_redirect_url(&self) -> String {
        format!("{}/auth/google/callback", self.app_origin)
    }

    pub fn session_cookie_name(&self) -> &'static str {
        if self.cookie_secure {
            "__Host-session"
        } else {
            "session"
        }
    }

    pub fn oauth_cookie_name(&self) -> &'static str {
        if self.cookie_secure {
            "__Host-oidc"
        } else {
            "oidc"
        }
    }
}

fn required(name: &str) -> anyhow::Result<String> {
    let value = env::var(name).with_context(|| format!("{name} is missing"))?;

    ensure!(!value.trim().is_empty(), "{name} cannot be empty");

    Ok(value)
}