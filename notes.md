Mevcut kodunu bu temel üzerine dönüştürelim:

- **Rust / Axum:** Google ile giriş, oturum yönetimi, kullanıcı API’si.
- **PostgreSQL:** Kullanıcılar, oturumlar ve kalıcı görev kayıtları.
- **Redis + BullMQ:** Zamanlanmış işler.
- **Docker Compose:** PostgreSQL, Redis ve BullMQ worker.
- **Local:** Sadece Rust; `cargo run`.
- **Migration:** Rust başlarken otomatik uygulanacak.

**Önemli:** BullMQ, Node.js üzerinde çalışır. Bu yüzden Docker’da küçük bir Node worker bulunacak. Bu, ikinci bir oyun backend’i olmayacak; oyun kuralları Rust’ta kalacak.

Aşağıdaki başlangıçta savaş motorunu yazmıyoruz. Google Login’i ve **PostgreSQL → BullMQ → Rust → tamamlanınca kuyruktan silme** altyapısını kuruyoruz.

> Dosya sistemine erişimim olmadığı için dosyaları doğrudan değiştiremiyorum. Aşağıdakiler mevcut projenin yerine/üzerine koyabileceğin dosyalardır. Burada derleme veya çalıştırma testi yapmadım.

---

# 1. Dosya yapısı

```text
rust-axum-full/
├── Cargo.toml
├── .env.example
├── .gitignore
├── compose.yaml
├── migrations/
│   └── 202609080001_initial.sql
├── src/
│   ├── main.rs
│   ├── config.rs
│   ├── state.rs
│   ├── error.rs
│   ├── security.rs
│   ├── middleware.rs
│   ├── auth/
│   │   ├── mod.rs
│   │   ├── handlers.rs
│   │   └── repository.rs
│   └── jobs/
│       ├── mod.rs
│       └── handlers.rs
└── services/
    └── bullmq/
        ├── Dockerfile
        ├── package.json
        └── src/
            └── index.mjs
```

Eski `routes/auth.rs`, JWT middleware’i ve sabit `admin/123456` girişi bu yapıda kullanılmayacak.

Tarayıcı tabanlı oyun için başlangıçta JWT yerine **PostgreSQL’de tutulan, iptal edilebilir güvenli oturum cookie’si** kullanıyoruz. Google token’larını frontend’e vermiyoruz.

---

# 2. Cargo.toml

```toml
[package]
name = "rust-axum-full"
version = "0.1.0"
edition = "2024"

[dependencies]
anyhow = "1"
axum = { version = "0.8", features = ["macros"] }
axum-extra = { version = "0.10", features = ["cookie"] }

tokio = { version = "1", features = ["full"] }

serde = { version = "1", features = ["derive"] }
serde_json = "1"

dotenvy = "0.15"

sqlx = { version = "0.8", default-features = false, features = [
    "runtime-tokio-rustls",
    "postgres",
    "uuid",
    "chrono",
    "migrate",
    "macros"
] }

# Bu kod 3.x API'sine göre yazılmıştır.
openidconnect = { version = "3.5", default-features = false, features = [
    "reqwest",
    "rustls-tls"
] }

uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }

rand = "0.8"
base64 = "0.22"
sha2 = "0.10"
subtle = "2"
time = "0.3"

tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
```

`Cargo.lock` dosyasını oluşturulduktan sonra Git’e ekle. Böylece bağımlılık sürümleri ekipte ve CI’da aynı kalır.

---

# 3. Environment

## `.env.example`

```env
# Rust:
# Docker worker'ın host üzerindeki Rust'a ulaşması için 0.0.0.0.
# Geliştirme ortamında firewall ayarlarını buna göre tut.
HOST=0.0.0.0
PORT=8080

APP_ORIGIN=http://localhost:8080
COOKIE_SECURE=false

RUST_LOG=info,sqlx=warn

# Docker PostgreSQL:
POSTGRES_DB=umaykut
POSTGRES_USER=umaykut
POSTGRES_PASSWORD=local_dev_change_me

# Host'ta çalışan Rust için:
DATABASE_URL=postgresql://umaykut:local_dev_change_me@localhost:5432/umaykut

# Google Cloud Console:
GOOGLE_CLIENT_ID=replace_me
GOOGLE_CLIENT_SECRET=replace_me

# Rust ve BullMQ worker arasında ortak secret.
# Çalıştırmadan önce rastgele bir değerle değiştir.
INTERNAL_WORKER_SECRET=replace_with_a_random_secret_at_least_32_characters
```

Google callback adresini ayrıca environment’ta tekrar tutmuyoruz. `APP_ORIGIN` üzerinden üretilecek:

```text
http://localhost:8080/auth/google/callback
```

## `.gitignore`

```gitignore
/target
.env
.env.*
!.env.example

node_modules
.DS_Store
```

---

# 4. Docker Compose

## `compose.yaml`

```yaml
name: umaykut-dev

services:
  postgres:
    image: postgres:17-alpine
    restart: unless-stopped
    environment:
      POSTGRES_DB: ${POSTGRES_DB:?POSTGRES_DB is required}
      POSTGRES_USER: ${POSTGRES_USER:?POSTGRES_USER is required}
      POSTGRES_PASSWORD: ${POSTGRES_PASSWORD:?POSTGRES_PASSWORD is required}
    ports:
      - "127.0.0.1:5432:5432"
    volumes:
      - postgres_data:/var/lib/postgresql/data
    healthcheck:
      test:
        [
          "CMD-SHELL",
          "pg_isready -U \"$${POSTGRES_USER}\" -d \"$${POSTGRES_DB}\""
        ]
      interval: 5s
      timeout: 5s
      retries: 10

  redis:
    image: redis:7.4-alpine
    restart: unless-stopped
    command:
      - redis-server
      - --appendonly
      - "yes"
      - --appendfsync
      - everysec
      - --maxmemory-policy
      - noeviction
    ports:
      - "127.0.0.1:6379:6379"
    volumes:
      - redis_data:/data
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 5s
      timeout: 3s
      retries: 10

  bullmq:
    build:
      context: ./services/bullmq
    restart: unless-stopped
    init: true
    environment:
      NODE_ENV: production

      PGHOST: postgres
      PGPORT: "5432"
      PGDATABASE: ${POSTGRES_DB}
      PGUSER: ${POSTGRES_USER}
      PGPASSWORD: ${POSTGRES_PASSWORD}

      REDIS_HOST: redis
      REDIS_PORT: "6379"

      RUST_API_URL: http://host.docker.internal:${PORT:-8080}
      INTERNAL_WORKER_SECRET: ${INTERNAL_WORKER_SECRET:?INTERNAL_WORKER_SECRET is required}

    extra_hosts:
      - "host.docker.internal:host-gateway"

    depends_on:
      postgres:
        condition: service_healthy
      redis:
        condition: service_healthy

    stop_grace_period: 30s

volumes:
  postgres_data:
  redis_data:
```

**Rust servisi bilerek Compose’a eklenmedi.**

PostgreSQL ve Redis portları yalnızca localhost’a açılıyor. BullMQ’nun dışarıya açık bir HTTP portu bulunmuyor.

---

# 5. Migration

## `migrations/202609080001_initial.sql`

```sql
CREATE TABLE users (
    id UUID PRIMARY KEY,
    google_sub TEXT NOT NULL UNIQUE,
    email TEXT NOT NULL,
    email_verified BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Kimlik eşleştirmesi email üzerinden değil, Google'ın sabit sub değeriyle yapılır.
-- Email'e bilerek UNIQUE koymuyoruz.


CREATE TABLE oauth_login_flows (
    state_hash TEXT PRIMARY KEY,
    browser_token_hash TEXT NOT NULL,
    nonce TEXT NOT NULL,
    pkce_verifier TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX oauth_login_flows_expires_at_idx
    ON oauth_login_flows (expires_at);


CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX sessions_user_id_idx
    ON sessions (user_id);

CREATE INDEX sessions_expires_at_idx
    ON sessions (expires_at);


-- Aynı zamanda transactional outbox görevi görür.
-- İleride savaş kaydı ile bu tabloya yapılacak INSERT aynı transaction'da olacak.
CREATE TABLE scheduled_jobs (
    id UUID PRIMARY KEY,
    kind TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    run_at TIMESTAMPTZ NOT NULL,

    last_enqueued_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT scheduled_jobs_payload_object
        CHECK (jsonb_typeof(payload) = 'object')
);

CREATE INDEX scheduled_jobs_pending_idx
    ON scheduled_jobs (last_enqueued_at, run_at)
    WHERE completed_at IS NULL;
```

Burada tamamlanan **BullMQ job’ı silinir**, PostgreSQL’deki kayıt kalır. Bu kayıt daha sonra:

- Aynı savaşın iki kere sonuçlanmasını engellemek
- Destek ve hata araştırması
- Savaş raporları
- İşlem takibi

için kullanılacak.

---

# 6. Rust yapılandırması

## `src/config.rs`

```rust
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
```

## `src/state.rs`

```rust
use crate::config::Config;
use openidconnect::core::CoreClient;
use sqlx::PgPool;
use std::sync::Arc;

pub struct AppState {
    pub config: Config,
    pub db: PgPool,
    pub google: CoreClient,
}

pub type SharedState = Arc<AppState>;
```

## `src/error.rs`

```rust
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

pub enum AppError {
    Unauthorized,
    Forbidden,
    NotFound,
    BadRequest(&'static str),
    Internal(anyhow::Error),
}

impl From<sqlx::Error> for AppError {
    fn from(error: sqlx::Error) -> Self {
        Self::Internal(error.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "Authentication required or invalid authentication",
            ),
            Self::Forbidden => (StatusCode::FORBIDDEN, "Forbidden"),
            Self::NotFound => (StatusCode::NOT_FOUND, "Resource not found"),
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            Self::Internal(error) => {
                tracing::error!(error = %error, "Internal request error");

                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error",
                )
            }
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}
```

## `src/security.rs`

```rust
use axum_extra::extract::cookie::{Cookie, SameSite};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};
use time::Duration;

pub fn random_token() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);

    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn hash_token(value: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(value.as_bytes()))
}

pub fn auth_cookie(
    name: &'static str,
    value: String,
    secure: bool,
    lifetime: Duration,
) -> Cookie<'static> {
    Cookie::build((name, value))
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
        .max_age(lifetime)
        .build()
}

pub fn removal_cookie(name: &'static str, secure: bool) -> Cookie<'static> {
    Cookie::build((name, ""))
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
        .build()
}
```

## `src/middleware.rs`

Google callback URL’sindeki authorization code’u loglamamak için tam URI yerine yalnızca path loglanıyor.

```rust
use axum::{
    extract::Request,
    middleware::Next,
    response::Response,
};
use std::time::Instant;

pub async fn request_logger(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let started = Instant::now();

    let response = next.run(request).await;

    tracing::info!(
        %method,
        %path,
        status = response.status().as_u16(),
        duration_ms = started.elapsed().as_millis() as u64,
        "HTTP request"
    );

    response
}
```

---

# 7. Google Login repository

## `src/auth/mod.rs`

```rust
pub mod handlers;
pub mod repository;
```

## `src/auth/repository.rs`

```rust
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(FromRow)]
pub struct LoginFlow {
    pub nonce: String,
    pub pkce_verifier: String,
}

#[derive(Serialize, FromRow)]
pub struct CurrentUser {
    pub id: Uuid,
    pub email: String,
    pub email_verified: bool,
    pub created_at: DateTime<Utc>,
}

pub async fn create_login_flow(
    db: &PgPool,
    state_hash: &str,
    browser_token_hash: &str,
    nonce: &str,
    pkce_verifier: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO oauth_login_flows (
            state_hash,
            browser_token_hash,
            nonce,
            pkce_verifier,
            expires_at
        )
        VALUES ($1, $2, $3, $4, NOW() + INTERVAL '10 minutes')
        "#,
    )
    .bind(state_hash)
    .bind(browser_token_hash)
    .bind(nonce)
    .bind(pkce_verifier)
    .execute(db)
    .await?;

    Ok(())
}

pub async fn consume_login_flow(
    db: &PgPool,
    state_hash: &str,
    browser_token_hash: &str,
) -> Result<Option<LoginFlow>, sqlx::Error> {
    sqlx::query_as::<_, LoginFlow>(
        r#"
        DELETE FROM oauth_login_flows
        WHERE state_hash = $1
          AND browser_token_hash = $2
          AND expires_at > NOW()
        RETURNING nonce, pkce_verifier
        "#,
    )
    .bind(state_hash)
    .bind(browser_token_hash)
    .fetch_optional(db)
    .await
}

pub async fn create_session(
    db: &PgPool,
    google_sub: &str,
    email: &str,
    token_hash: &str,
    previous_token_hash: Option<&str>,
) -> Result<(), sqlx::Error> {
    let mut transaction = db.begin().await?;

    let user_id: Uuid = sqlx::query_scalar(
        r#"
        INSERT INTO users (
            id,
            google_sub,
            email,
            email_verified
        )
        VALUES ($1, $2, $3, TRUE)
        ON CONFLICT (google_sub)
        DO UPDATE SET
            email = EXCLUDED.email,
            email_verified = EXCLUDED.email_verified,
            updated_at = NOW()
        RETURNING id
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(google_sub)
    .bind(email)
    .fetch_one(&mut *transaction)
    .await?;

    // Aynı tarayıcıdaki önceki oturumu iptal et.
    if let Some(previous_token_hash) = previous_token_hash {
        sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
            .bind(previous_token_hash)
            .execute(&mut *transaction)
            .await?;
    }

    sqlx::query(
        r#"
        INSERT INTO sessions (
            token_hash,
            user_id,
            expires_at
        )
        VALUES ($1, $2, NOW() + INTERVAL '7 days')
        "#,
    )
    .bind(token_hash)
    .bind(user_id)
    .execute(&mut *transaction)
    .await?;

    transaction.commit().await
}

pub async fn find_session_user(
    db: &PgPool,
    token_hash: &str,
) -> Result<Option<CurrentUser>, sqlx::Error> {
    sqlx::query_as::<_, CurrentUser>(
        r#"
        SELECT
            u.id,
            u.email,
            u.email_verified,
            u.created_at
        FROM sessions s
        JOIN users u ON u.id = s.user_id
        WHERE s.token_hash = $1
          AND s.expires_at > NOW()
        "#,
    )
    .bind(token_hash)
    .fetch_optional(db)
    .await
}

pub async fn delete_session(
    db: &PgPool,
    token_hash: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
        .bind(token_hash)
        .execute(db)
        .await?;

    Ok(())
}

pub async fn cleanup_expired(db: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM oauth_login_flows WHERE expires_at <= NOW()")
        .execute(db)
        .await?;

    sqlx::query("DELETE FROM sessions WHERE expires_at <= NOW()")
        .execute(db)
        .await?;

    Ok(())
}
```

---

# 8. Google Login endpoint’leri

## `src/auth/handlers.rs`

```rust
use super::repository;

use crate::{
    error::AppError,
    security::{auth_cookie, hash_token, random_token, removal_cookie},
    state::SharedState,
};

use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::Redirect,
};

use axum_extra::extract::CookieJar;

use openidconnect::{
    AccessTokenHash,
    AuthenticationFlow,
    AuthorizationCode,
    CsrfToken,
    Nonce,
    OAuth2TokenResponse,
    PkceCodeChallenge,
    PkceCodeVerifier,
    Scope,
    TokenResponse,
    core::CoreAuthenticationFlow,
    reqwest::async_http_client,
};

use serde::Deserialize;
use time::Duration;

#[derive(Deserialize)]
pub struct GoogleCallback {
    pub code: String,
    pub state: String,
}

pub async fn google_login(
    State(state): State<SharedState>,
    jar: CookieJar,
) -> Result<(CookieJar, Redirect), AppError> {
    let (pkce_challenge, pkce_verifier) =
        PkceCodeChallenge::new_random_sha256();

    let (authorization_url, csrf_token, nonce) = state
        .google
        .authorize_url(
            AuthenticationFlow::<CoreAuthenticationFlow>::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .add_scope(Scope::new("email".to_owned()))
        .set_pkce_challenge(pkce_challenge)
        .url();

    let browser_token = random_token();

    repository::create_login_flow(
        &state.db,
        &hash_token(csrf_token.secret()),
        &hash_token(&browser_token),
        nonce.secret(),
        pkce_verifier.secret(),
    )
    .await?;

    let jar = jar.add(auth_cookie(
        state.config.oauth_cookie_name(),
        browser_token,
        state.config.cookie_secure,
        Duration::minutes(10),
    ));

    Ok((jar, Redirect::temporary(authorization_url.as_str())))
}

pub async fn google_callback(
    State(state): State<SharedState>,
    jar: CookieJar,
    Query(query): Query<GoogleCallback>,
) -> Result<(CookieJar, Redirect), AppError> {
    let browser_token = jar
        .get(state.config.oauth_cookie_name())
        .ok_or(AppError::Unauthorized)?
        .value()
        .to_owned();

    // Tek kullanımlık state + tarayıcı bağı kontrolü.
    let flow = repository::consume_login_flow(
        &state.db,
        &hash_token(&query.state),
        &hash_token(&browser_token),
    )
    .await?
    .ok_or(AppError::Unauthorized)?;

    let token_response = state
        .google
        .exchange_code(AuthorizationCode::new(query.code))
        .set_pkce_verifier(PkceCodeVerifier::new(flow.pkce_verifier))
        .request_async(async_http_client)
        .await
        .map_err(|_| AppError::Unauthorized)?;

    let id_token = token_response
        .extra_fields()
        .id_token()
        .ok_or(AppError::Unauthorized)?;

    let verifier = state.google.id_token_verifier();
    let expected_nonce = Nonce::new(flow.nonce);

    // İmza, issuer, audience, expiration ve nonce doğrulanır.
    let claims = id_token
        .claims(&verifier, &expected_nonce)
        .map_err(|_| AppError::Unauthorized)?;

    if let Some(expected_access_token_hash) = claims.access_token_hash() {
        let signing_algorithm = id_token
            .signing_alg()
            .map_err(|_| AppError::Unauthorized)?;

        let actual_access_token_hash = AccessTokenHash::from_token(
            token_response.access_token(),
            &signing_algorithm,
        )
        .map_err(|_| AppError::Unauthorized)?;

        if actual_access_token_hash != *expected_access_token_hash {
            return Err(AppError::Unauthorized);
        }
    }

    if claims.email_verified() != Some(true) {
        return Err(AppError::Unauthorized);
    }

    let email = claims
        .email()
        .ok_or(AppError::Unauthorized)?
        .as_str()
        .to_owned();

    let google_sub = claims.subject().as_str().to_owned();

    let session_token = random_token();

    let previous_token_hash = jar
        .get(state.config.session_cookie_name())
        .map(|cookie| hash_token(cookie.value()));

    repository::create_session(
        &state.db,
        &google_sub,
        &email,
        &hash_token(&session_token),
        previous_token_hash.as_deref(),
    )
    .await?;

    let jar = jar
        .remove(removal_cookie(
            state.config.oauth_cookie_name(),
            state.config.cookie_secure,
        ))
        .add(auth_cookie(
            state.config.session_cookie_name(),
            session_token,
            state.config.cookie_secure,
            Duration::days(7),
        ));

    Ok((jar, Redirect::to("/auth/me")))
}

pub async fn me(
    State(state): State<SharedState>,
    jar: CookieJar,
) -> Result<Json<repository::CurrentUser>, AppError> {
    let session_token = jar
        .get(state.config.session_cookie_name())
        .ok_or(AppError::Unauthorized)?
        .value();

    let user = repository::find_session_user(
        &state.db,
        &hash_token(session_token),
    )
    .await?
    .ok_or(AppError::Unauthorized)?;

    Ok(Json(user))
}

pub async fn logout(
    State(state): State<SharedState>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<(CookieJar, StatusCode), AppError> {
    // Cookie tabanlı, durum değiştiren endpoint için Origin kontrolü.
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());

    if origin != Some(state.config.app_origin.as_str()) {
        return Err(AppError::Forbidden);
    }

    if let Some(cookie) = jar.get(state.config.session_cookie_name()) {
        repository::delete_session(
            &state.db,
            &hash_token(cookie.value()),
        )
        .await?;
    }

    let jar = jar.remove(removal_cookie(
        state.config.session_cookie_name(),
        state.config.cookie_secure,
    ));

    Ok((jar, StatusCode::NO_CONTENT))
}
```

Burada:

- Google `sub` değeriyle kullanıcı bulunur.
- İlk girişte kayıt otomatik oluşur.
- Email ve doğrulama bilgisi PostgreSQL’e kaydedilir.
- Google access token ve refresh token saklanmaz.
- Oturum token’ının kendisi değil, hash’i PostgreSQL’de tutulur.

---

# 9. Rust job endpoint’i

Bu endpoint yalnızca Docker’daki worker tarafından çağrılacak.

Şimdilik sadece `system.ping` test işini işler. Gerçek `battle.resolve.v1` mantığını daha sonra buraya bağlayacağız; bilinmeyen işi başarılı kabul etmiyoruz.

## `src/jobs/mod.rs`

```rust
pub mod handlers;
```

## `src/jobs/handlers.rs`

```rust
use crate::{
    error::AppError,
    state::SharedState,
};

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::FromRow;
use subtle::ConstantTimeEq;
use uuid::Uuid;

#[derive(FromRow)]
struct ScheduledJob {
    kind: String,
    payload: Value,
    completed_at: Option<DateTime<Utc>>,
    is_due: bool,
}

pub async fn execute(
    State(state): State<SharedState>,
    Path(job_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    let supplied_secret = headers
        .get("x-worker-secret")
        .and_then(|value| value.to_str().ok())
        .ok_or(AppError::Unauthorized)?;

    let authorized: bool = supplied_secret
        .as_bytes()
        .ct_eq(state.config.internal_worker_secret.as_bytes())
        .into();

    if !authorized {
        return Err(AppError::Unauthorized);
    }

    let mut transaction = state.db.begin().await?;

    let job = sqlx::query_as::<_, ScheduledJob>(
        r#"
        SELECT
            kind,
            payload,
            completed_at,
            run_at <= NOW() AS is_due
        FROM scheduled_jobs
        WHERE id = $1
        FOR UPDATE
        "#,
    )
    .bind(job_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;

    // BullMQ aynı işi tekrar teslim edebilir.
    // Tamamlanan işin etkileri ikinci kez uygulanmaz.
    if job.completed_at.is_some() {
        transaction.commit().await?;
        return Ok(StatusCode::NO_CONTENT);
    }

    if !job.is_due {
        return Err(AppError::BadRequest("Job is not due yet"));
    }

    match job.kind.as_str() {
        "system.ping" => {
            tracing::info!(
                %job_id,
                payload = %job.payload,
                "Scheduled test job executed"
            );
        }

        // Sonraki aşama:
        //
        // "battle.resolve.v1" => {
        //     battle_service::resolve(
        //         &mut transaction,
        //         &job.payload,
        //     ).await?;
        // }
        //
        // Savaş sonucu ve completed_at aynı transaction'da yazılmalı.

        _ => {
            return Err(AppError::BadRequest("Unsupported job kind"));
        }
    }

    sqlx::query(
        r#"
        UPDATE scheduled_jobs
        SET completed_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(job_id)
    .execute(&mut *transaction)
    .await?;

    transaction.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}
```

**Kritik ayrıntı:** Savaş hesabını Node’a taşımıyoruz. Worker yalnızca “bu işin zamanı geldi” diyerek Rust’ı çağıracak. Rust veritabanından gerçek veriyi okuyacak.

---

# 10. main.rs

## `src/main.rs`

```rust
mod auth;
mod config;
mod error;
mod jobs;
mod middleware;
mod security;
mod state;

use axum::{
    Json,
    Router,
    extract::State,
    middleware::from_fn,
    routing::{get, post},
};

use config::Config;
use error::AppError;

use openidconnect::{
    ClientId,
    ClientSecret,
    IssuerUrl,
    RedirectUrl,
    core::{CoreClient, CoreProviderMetadata},
    reqwest::async_http_client,
};

use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use state::{AppState, SharedState};

use std::{sync::Arc, time::Duration};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = Config::from_env()?;

    let db = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&config.database_url)
        .await?;

    // Migration dosyaları binary içine gömülür.
    // Ayrı sqlx CLI kurmak zorunlu değildir.
    sqlx::migrate!("./migrations")
        .run(&db)
        .await?;

    tracing::info!("Database migrations applied");

    let google_metadata = CoreProviderMetadata::discover_async(
        IssuerUrl::new("https://accounts.google.com".to_owned())?,
        async_http_client,
    )
    .await?;

    let google = CoreClient::from_provider_metadata(
        google_metadata,
        ClientId::new(config.google_client_id.clone()),
        Some(ClientSecret::new(config.google_client_secret.clone())),
    )
    .set_redirect_uri(
        RedirectUrl::new(config.google_redirect_url())?,
    );

    let bind_address = format!("{}:{}", config.host, config.port);

    let state: SharedState = Arc::new(AppState {
        config,
        db,
        google,
    });

    let app = Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .route("/auth/google", get(auth::handlers::google_login))
        .route(
            "/auth/google/callback",
            get(auth::handlers::google_callback),
        )
        .route("/auth/me", get(auth::handlers::me))
        .route("/auth/logout", post(auth::handlers::logout))
        .route(
            "/internal/jobs/{job_id}/execute",
            post(jobs::handlers::execute),
        )
        .layer(from_fn(middleware::request_logger))
        .with_state(state.clone());

    let cleanup_db = state.db.clone();

    let cleanup_task = tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(Duration::from_secs(15 * 60));

        loop {
            interval.tick().await;

            if let Err(error) =
                auth::repository::cleanup_expired(&cleanup_db).await
            {
                tracing::error!(
                    %error,
                    "Expired authentication records cleanup failed"
                );
            }
        }
    });

    let listener = TcpListener::bind(&bind_address).await?;

    tracing::info!(
        address = %bind_address,
        "Rust API started"
    );

    let result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await;

    cleanup_task.abort();
    state.db.close().await;

    result?;

    Ok(())
}

async fn live() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn ready(
    State(state): State<SharedState>,
) -> Result<Json<Value>, AppError> {
    sqlx::query("SELECT 1")
        .execute(&state.db)
        .await?;

    Ok(Json(json!({
        "status": "ok",
        "postgres": "connected"
    })))
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::terminate(),
        )
        .expect("Failed to install terminate handler")
        .recv()
        .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("Shutdown requested");
}
```

Google metadata/JWKS başlangıçta alınır; bu nedenle ilk çalıştırmada internet gerekir. Uzun süre çalışan production sürümünde anahtar yenileme/caching politikasını ayrıca eklemek gerekir.

---

# 11. BullMQ servisi

BullMQ’nun Redis formatına Rust’tan elle veri yazmıyoruz.

Akış:

```text
Rust / PostgreSQL transaction
            │
            ▼
     scheduled_jobs
            │
            ▼
     Node dispatcher
            │
            ▼
    BullMQ delayed job
            │
       zamanı gelince
            ▼
    Rust internal endpoint
            │
            ▼
  Transaction ile işi tamamla
            │
            ▼
  BullMQ job'ını otomatik sil
```

## `services/bullmq/package.json`

```json
{
  "name": "umaykut-bullmq",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {
    "start": "node src/index.mjs"
  },
  "dependencies": {
    "bullmq": "^5.58.0",
    "pg": "^8.16.0"
  }
}
```

## `services/bullmq/Dockerfile`

```dockerfile
FROM node:22-alpine

WORKDIR /app

ENV NODE_ENV=production

COPY package*.json ./

RUN if [ -f package-lock.json ]; then \
      npm ci --omit=dev; \
    else \
      npm install --omit=dev; \
    fi

COPY --chown=node:node src ./src

USER node

CMD ["node", "src/index.mjs"]
```

İlk kurulumda lock dosyası olmadan çalışır. Tekrarlanabilir build için sonrasında `package-lock.json` üretip Git’e ekleyerek Dockerfile’ı yalnızca `npm ci` kullanacak hale getir.

## `services/bullmq/src/index.mjs`

```javascript
import { Queue, Worker } from "bullmq";
import pg from "pg";
import { setTimeout as sleep } from "node:timers/promises";

const { Pool } = pg;

function required(name) {
  const value = process.env[name];

  if (!value) {
    throw new Error(`${name} is required`);
  }

  return value;
}

const config = {
  redisHost: required("REDIS_HOST"),
  redisPort: Number(process.env.REDIS_PORT ?? 6379),
  rustApiUrl: required("RUST_API_URL").replace(/\/$/, ""),
  workerSecret: required("INTERNAL_WORKER_SECRET"),
};

const connection = {
  host: config.redisHost,
  port: config.redisPort,
};

const database = new Pool({
  max: 3,
  connectionTimeoutMillis: 5000,
});

database.on("error", (error) => {
  console.error("PostgreSQL pool error:", error.message);
});

const queueName = "game-jobs";

const queue = new Queue(queueName, {
  connection,
});

queue.on("error", (error) => {
  console.error("Queue error:", error.message);
});

async function executeOnRust(job) {
  const response = await fetch(
    `${config.rustApiUrl}/internal/jobs/${job.data.taskId}/execute`,
    {
      method: "POST",
      headers: {
        "x-worker-secret": config.workerSecret,
      },
      signal: AbortSignal.timeout(15_000),
    },
  );

  if (!response.ok) {
    // Response içeriğini loglamıyoruz; ileride hassas veri içerebilir.
    throw new Error(`Rust job execution returned HTTP ${response.status}`);
  }
}

const worker = new Worker(
  queueName,
  executeOnRust,
  {
    connection,
    concurrency: 5,
  },
);

worker.on("completed", (job) => {
  console.info(`Job completed: ${job.id}`);
});

worker.on("failed", (job, error) => {
  console.error(
    `Job attempt failed: ${job?.id}; ${error.message}`,
  );
});

worker.on("error", (error) => {
  console.error("Worker error:", error.message);
});

async function dispatchPendingJobs() {
  const client = await database.connect();

  try {
    await client.query("BEGIN");

    // Aynı servisten birden fazla instance çalışırsa aynı kayıt için
    // dispatcher yarışını SKIP LOCKED ile önlüyoruz.
    //
    // Tamamlanmamış işleri 30 saniyede bir yeniden kontrol ediyoruz:
    // Redis'te kaybolmuş bir job varsa tekrar oluşturulabilir.
    const result = await client.query(`
      SELECT id, kind, run_at
      FROM scheduled_jobs
      WHERE completed_at IS NULL
        AND (
          last_enqueued_at IS NULL
          OR last_enqueued_at < NOW() - INTERVAL '30 seconds'
        )
      ORDER BY last_enqueued_at ASC NULLS FIRST, run_at ASC
      LIMIT 25
      FOR UPDATE SKIP LOCKED
    `);

    for (const task of result.rows) {
      const delay = Math.max(
        0,
        new Date(task.run_at).getTime() - Date.now(),
      );

      await queue.add(
        task.kind,
        {
          taskId: task.id,
        },
        {
          // UUID içinde BullMQ jobId için yasak olan ":" bulunmaz.
          jobId: task.id,

          delay,

          attempts: 100,
          backoff: {
            type: "fixed",
            delay: 5000,
          },

          // Kullanıcının istediği davranış:
          removeOnComplete: true,

          // Hataları sessizce kaybetme.
          removeOnFail: false,
        },
      );

      await client.query(
        `
        UPDATE scheduled_jobs
        SET last_enqueued_at = NOW()
        WHERE id = $1
        `,
        [task.id],
      );
    }

    await client.query("COMMIT");
  } catch (error) {
    await client.query("ROLLBACK").catch(() => {});
    throw error;
  } finally {
    client.release();
  }
}

let stopping = false;

async function dispatchLoop() {
  while (!stopping) {
    try {
      await dispatchPendingJobs();
    } catch (error) {
      // İlk açılışta Rust migration'ı henüz çalışmadıysa bekle.
      if (error.code === "42P01") {
        console.info("Waiting for Rust database migrations...");
      } else {
        console.error("Dispatcher error:", error.message);
      }
    }

    await sleep(1000);
  }
}

const dispatcher = dispatchLoop();

async function shutdown(signal) {
  if (stopping) {
    return;
  }

  console.info(`Shutting down: ${signal}`);
  stopping = true;

  try {
    await dispatcher;
    await worker.close();
    await queue.close();
    await database.end();
  } catch (error) {
    console.error("Shutdown error:", error.message);
    process.exitCode = 1;
  }
}

process.once("SIGTERM", () => {
  void shutdown("SIGTERM");
});

process.once("SIGINT", () => {
  void shutdown("SIGINT");
});

console.info("BullMQ dispatcher and worker started");
```

### Bu tasarım neden böyle?

PostgreSQL ile Redis arasında ortak transaction yok. Bu nedenle teslimat **en az bir kez** gerçekleşebilir.

Örneğin:

1. Rust işi tamamladı.
2. Worker cevabı alamadan kapandı.
3. BullMQ tekrar denedi.

Rust `completed_at` kontrolü sayesinde işlemi tekrar uygulamadan başarılı döner. Böylece ileride aynı savaş için iki defa ödül verilmez.

`jobId` tek başına yeterli değildir; çünkü tamamlanan job Redis’ten silinmektedir.

---

# 12. Google Cloud ayarları

Google Cloud Console’da:

1. Proje oluştur.
2. OAuth consent screen’i yapılandır.
3. Uygulama test durumundaysa kendi Google hesabını test kullanıcısı yap.
4. **OAuth Client ID → Web application** oluştur.
5. Authorized redirect URI ekle:

```text
http://localhost:8080/auth/google/callback
```

6. Client ID ve secret’ı `.env` içine koy.

Bu server-side yönlendirme akışında ayrıca JavaScript SDK kurmuyoruz.

---

# 13. İlk çalıştırma

## 13.1 Environment dosyasını oluştur

```bash
cp .env.example .env
```

Rastgele worker secret üret:

```bash
openssl rand -hex 32
```

Çıktıyı `.env` içindeki `INTERNAL_WORKER_SECRET` değerine yapıştır.

Google bilgilerini de doldur.

## 13.2 Altyapıyı başlat

```bash
docker compose up -d --build
```

İlk açılışta worker şu mesajı verebilir:

```text
Waiting for Rust database migrations...
```

Normaldir; migration’ı Rust uygulayacak.

## 13.3 Rust’ı başlat

```bash
cargo run
```

Rust:

1. PostgreSQL’e bağlanır.
2. Migration’ları uygular.
3. Google OIDC yapılandırmasını alır.
4. API’yi başlatır.

## 13.4 Health kontrolü

```bash
curl http://localhost:8080/health/ready
```

Beklenen:

```json
{
  "status": "ok",
  "postgres": "connected"
}
```

## 13.5 Google ile giriş

Tarayıcıda:

```text
http://localhost:8080/auth/google
```

Başarılı girişten sonra:

```text
http://localhost:8080/auth/me
```

Örnek cevap:

```json
{
  "id": "9f027b61-7dbb-4bbc-a920-6321667ba2ca",
  "email": "oyuncu@example.com",
  "email_verified": true,
  "created_at": "2026-09-08T12:00:00Z"
}
```

---

# 14. BullMQ zamanlama testi

Şimdilik dışarıya açık bir “istediğin job’ı oluştur” endpoint’i koymuyoruz. Test için PostgreSQL’e 15 saniye sonra çalışacak bir iş ekle:

```bash
docker compose exec -T postgres \
  psql -U umaykut -d umaykut <<'SQL'
INSERT INTO scheduled_jobs (
    id,
    kind,
    payload,
    run_at
)
VALUES (
    gen_random_uuid(),
    'system.ping',
    '{"message": "BullMQ scheduling works"}',
    NOW() + INTERVAL '15 seconds'
)
RETURNING id, run_at;
SQL
```

Worker logları:

```bash
docker compose logs -f bullmq
```

İş tamamlanınca:

```text
Job completed: ...
```

PostgreSQL kontrolü:

```bash
docker compose exec postgres \
  psql -U umaykut -d umaykut \
  -c "SELECT id, kind, run_at, completed_at FROM scheduled_jobs ORDER BY created_at DESC;"
```

`completed_at` dolmuş olmalı.

**BullMQ tarafında tamamlanan iş `removeOnComplete: true` nedeniyle silinir.**

> BullMQ gerçek zamanlı/hard-deadline bir zamanlayıcı değildir. İş, hedef zamandan önce çalıştırılmaz; sistem yükü, Rust’ın kapalı olması veya retry nedeniyle daha geç tamamlanabilir. Savaş kuralları Redis’in çalışma anına değil PostgreSQL’deki `run_at` değerine dayanmalı.

---

# 15. Sonraki günlerde çalıştırma

Docker açıksa ve container’ları elle durdurmadıysan:

```bash
cargo run
```

yeterlidir.

`restart: unless-stopped` Docker yeniden başladığında servisleri geri getirir. Ancak:

- Docker Engine / Docker Desktop çalışıyor olmalı.
- `docker compose down` yaptıysan container’lar silinir; yeniden `up -d` gerekir.
- Elle durdurduğun container otomatik başlamaz.

Loglar:

```bash
docker compose logs -f bullmq
```

Altyapı durumu:

```bash
docker compose ps
```

**Veriyi silen komut:**

```bash
docker compose down -v
```

Bunu normal geliştirme döngüsünde kullanma; PostgreSQL ve Redis volume’larını siler.

---

## Bu aşamanın sınırı

Bu temel şunları sağlar:

| Özellik | Durum |
|---|---|
| Rust’ın `cargo run` ile çalışması | Hazır |
| PostgreSQL’in Docker’da olması | Hazır |
| Otomatik migration | Hazır |
| Google ile giriş ve otomatik kullanıcı kaydı | Hazır |
| PostgreSQL tabanlı oturum | Hazır |
| Redis + BullMQ worker | Hazır |
| Zamanlanmış test işi | Hazır |
| Başarılı job’ın kuyruktan silinmesi | Hazır |
| Tekrarlı teslimatta DB işleminin korunması | Temeli hazır |
| Gerçek savaş hesabı | Sonraki aşama |
| Frontend | Bu kapsamda yok |

**Production öncesinde** ayrıca login rate limit, Google anahtar yenileme, kalıcı hata alan job’ların yeniden deneme/uyarı yönetimi, HTTPS ve yedekleme eklenmeli. Bu sürümde Rust uzun süre kapalı kalıp retry hakkı tükenirse iş `failed` olarak korunur; Rust’ın yeniden açılması tek başına o failed job’ı tekrar başlatmaz.

Bir sonraki geliştirme, bu yapıyı değiştirmeden **`battles` migration’ı + Rust savaş başlatma use-case’i + aynı transaction’da `scheduled_jobs` kaydı + `battle.resolve.v1` handler’ı** eklemek olacaktır.