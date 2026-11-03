use crate::core::{self, Challenge};
use axum::{
    extract::{ConnectInfo, DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use hmac::{Hmac, Mac};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha256;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use subtle::ConstantTimeEq;
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub website: crate::website::WebsitePolicy,
    pub database: String,
    pub token_key_file: String,
    pub public_listen: SocketAddr,
    pub internal_listen: SocketAddr,
    pub difficulty_bits: u8,
    pub challenge_ttl_seconds: i64,
    pub token_ttl_seconds: i64,
    pub retention_seconds: i64,
    pub requests_per_minute: u32,
    pub max_active_challenges: u32,
}
impl Config {
    pub fn validate(&self) -> Result<(), String> {
        if !self.website.valid()
            || !(1..=24).contains(&self.difficulty_bits)
            || !(10..=600).contains(&self.challenge_ttl_seconds)
            || !(10..=300).contains(&self.token_ttl_seconds)
            || !(300..=86400).contains(&self.retention_seconds)
            || self.requests_per_minute == 0
            || self.max_active_challenges == 0
            || !self.internal_listen.ip().is_loopback()
        {
            return Err("invalid configuration; internal API must bind to loopback".into());
        }
        Ok(())
    }
}
pub struct App {
    pub db: Mutex<Connection>,
    pub config: Config,
    key: Vec<u8>,
    limits: Mutex<HashMap<String, (i64, u32)>>,
}
pub type Shared = Arc<App>;
#[derive(Debug)]
pub struct Error(pub StatusCode, pub &'static str);
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (self.0,Json(json!({"error":{"code":self.1,"retryable": self.0 == StatusCode::TOO_MANY_REQUESTS || self.0 == StatusCode::SERVICE_UNAVAILABLE}}))).into_response()
    }
}
pub(crate) fn bad(code: &'static str) -> Error {
    Error(StatusCode::BAD_REQUEST, code)
}
pub(crate) fn db_error(_: rusqlite::Error) -> Error {
    Error(StatusCode::SERVICE_UNAVAILABLE, "SERVICE_UNAVAILABLE")
}
impl App {
    pub fn open(config: Config) -> Result<Shared, Box<dyn std::error::Error>> {
        config.validate()?;
        let raw = std::fs::read_to_string(&config.token_key_file)?;
        let key = core::decode(raw.trim(), 32)
            .ok_or("token key must be canonical base64url, 32 bytes")?;
        let db = Connection::open(&config.database)?;
        db.busy_timeout(std::time::Duration::from_secs(2))?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
        CREATE TABLE IF NOT EXISTS schema_migrations(version INTEGER PRIMARY KEY);
        INSERT OR IGNORE INTO schema_migrations VALUES(1);
        CREATE TABLE IF NOT EXISTS integrations(id TEXT PRIMARY KEY,name TEXT NOT NULL,secret_hash TEXT NOT NULL, previous_hash TEXT, previous_until INTEGER NOT NULL DEFAULT 0,active INTEGER NOT NULL DEFAULT 1);
        CREATE TABLE IF NOT EXISTS history_policies(integration_id TEXT PRIMARY KEY REFERENCES integrations(id));
        CREATE TABLE IF NOT EXISTS attempt_history(integration_id TEXT NOT NULL REFERENCES integrations(id),source TEXT NOT NULL,minute INTEGER NOT NULL,attempts INTEGER NOT NULL DEFAULT 0,failures INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(integration_id,source,minute));
        CREATE INDEX IF NOT EXISTS attempt_history_expiry ON attempt_history(minute);
        CREATE TABLE IF NOT EXISTS behavior_policies(integration_id TEXT PRIMARY KEY REFERENCES integrations(id));
        CREATE TABLE IF NOT EXISTS browser_policies(integration_id TEXT PRIMARY KEY REFERENCES integrations(id));
        CREATE TABLE IF NOT EXISTS actions(integration_id TEXT NOT NULL REFERENCES integrations(id),action TEXT NOT NULL, PRIMARY KEY(integration_id,action));
        CREATE TABLE IF NOT EXISTS challenges(id TEXT PRIMARY KEY,integration_id TEXT NOT NULL REFERENCES integrations(id),action TEXT NOT NULL,context TEXT NOT NULL,salt TEXT NOT NULL,difficulty INTEGER NOT NULL,expires INTEGER NOT NULL,solved_at INTEGER,token_hash TEXT UNIQUE,token_expires INTEGER,consumed_at INTEGER,request_id TEXT, UNIQUE(integration_id,action,context));
        CREATE INDEX IF NOT EXISTS challenge_expiry ON challenges(expires);
        CREATE INDEX IF NOT EXISTS challenge_integration ON challenges(integration_id,expires);")?;
        crate::website::migrate(&db)?;
        crate::visual::migrate(&db)?;
        Ok(Arc::new(Self {
            db: Mutex::new(db),
            config,
            key,
            limits: Mutex::new(HashMap::new()),
        }))
    }
    pub(crate) fn pseudonym(&self, domain: &[u8], integration: &str, value: &str) -> String {
        let mut h = Hmac::<Sha256>::new_from_slice(&self.key).unwrap();
        h.update(domain);
        h.update(integration.as_bytes());
        h.update(&[0]);
        h.update(value.as_bytes());
        B64.encode(h.finalize().into_bytes())
    }
    fn token(&self, id: &str) -> String {
        let mut h = Hmac::<Sha256>::new_from_slice(&self.key).unwrap();
        h.update(b"gateveriq:verification:v1");
        h.update(&core::decode(id, 32).unwrap());
        B64.encode(h.finalize().into_bytes())
    }
    pub fn create_integration(
        &self,
        name: &str,
        actions: &[String],
    ) -> Result<(String, String), Box<dyn std::error::Error>> {
        if name.is_empty()
            || name.len() > 128
            || actions.is_empty()
            || !actions.iter().all(|x| valid(x))
        {
            return Err("invalid name or actions".into());
        }
        let id = core::random();
        let secret = core::random();
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction()?;
        tx.execute(
            "INSERT INTO integrations(id,name,secret_hash) VALUES(?1,?2,?3)",
            params![id, name, core::hash(secret.as_bytes())],
        )?;
        for action in actions {
            tx.execute("INSERT INTO actions VALUES(?1,?2)", params![id, action])?;
        }
        tx.commit()?;
        Ok((id, secret))
    }
    pub(crate) fn auth(&self, headers: &HeaderMap, id: &str) -> Result<(), Error> {
        let token = headers
            .get("authorization")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .filter(|s| core::decode(s, 32).is_some())
            .ok_or(Error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED"))?;
        let db = self.db.lock().unwrap();
        let row:Option<(String,Option<String>,i64)>=db.query_row("SELECT secret_hash,previous_hash,previous_until FROM integrations WHERE id=?1 AND active=1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db_error)?;
        let (current, old, until) = row.ok_or(Error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED"))?;
        let h = core::hash(token.as_bytes());
        if bool::from(h.as_bytes().ct_eq(current.as_bytes()))
            || (until > now() && old.is_some_and(|v| bool::from(h.as_bytes().ct_eq(v.as_bytes()))))
        {
            Ok(())
        } else {
            Err(Error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED"))
        }
    }
    pub(crate) fn limit(&self, key: String) -> Result<(), Error> {
        let t = now() / 60;
        let mut limits = self.limits.lock().unwrap();
        limits.retain(|_, v| v.0 == t);
        if limits.len() >= 10000 && !limits.contains_key(&key) {
            return Err(Error(StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED"));
        }
        let v = limits.entry(key).or_insert((t, 0));
        if v.1 >= self.config.requests_per_minute {
            return Err(Error(StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED"));
        }
        v.1 += 1;
        Ok(())
    }
    pub fn cleanup(&self) -> Result<usize, rusqlite::Error> {
        crate::website::cleanup(self)?;
        self.db.lock().unwrap().execute("DELETE FROM attempt_history WHERE rowid IN (SELECT rowid FROM attempt_history WHERE minute < ?1 LIMIT 500)", [now() / 60 - self.config.website.history_retention_seconds / 60])?;
        self.db.lock().unwrap().execute("DELETE FROM challenges WHERE id IN (SELECT id FROM challenges WHERE MAX(expires,COALESCE(token_expires,0),COALESCE(consumed_at,0)) < ?1 LIMIT 500)",[now()-self.config.retention_seconds])
    }
}
fn valid(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.:".contains(&b))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prepare {
    pub integration_id: String,
    pub action: String,
    pub context_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserSignals {
    pub webdriver: bool,
    pub cookies_enabled: bool,
    pub languages_count: u8,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BehaviorSignals {
    pub challenge_id: String,
    pub elapsed_ms: u32,
    pub activation_ms: u32,
    pub trusted_activations: u16,
    pub pointer_events: u16,
    pub keyboard_events: u16,
    pub focus_changes: u16,
}
impl BehaviorSignals {
    fn acceptable(&self, challenge_id: &str, ttl_ms: u32) -> bool {
        self.challenge_id == challenge_id
            && self.elapsed_ms >= 300
            && self.elapsed_ms <= ttl_ms
            && self.activation_ms >= 300
            && self.activation_ms <= self.elapsed_ms
            && (1..=20).contains(&self.trusted_activations)
            && self.pointer_events <= 4096
            && self.keyboard_events <= 4096
            && self.focus_changes <= 4096
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Solution {
    pub challenge_id: String,
    pub nonce: String,
    #[serde(default)]
    pub browser: Option<BrowserSignals>,
    #[serde(default)]
    pub behavior: Option<BehaviorSignals>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Consume {
    pub integration_id: String,
    pub verification_token: String,
    pub action: String,
    pub context_id: String,
    pub request_id: String,
}
fn challenge(row: &rusqlite::Row) -> rusqlite::Result<Challenge> {
    Ok(Challenge {
        challenge_id: row.get(0)?,
        algorithm: "sha256-pow-v1".into(),
        salt: row.get(1)?,
        difficulty_bits: row.get(2)?,
        expires_at: row.get(3)?,
    })
}
pub fn prepare(app: &Shared, headers: &HeaderMap, p: Prepare) -> Result<Challenge, Error> {
    app.auth(headers, &p.integration_id)?;
    if !valid(&p.action) || !valid(&p.context_id) {
        return Err(bad("INVALID_REQUEST"));
    }
    app.limit(format!("integration:{}", p.integration_id))?;
    let mut db = app.db.lock().unwrap();
    let tx = db.transaction().map_err(db_error)?;
    let allowed: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM actions WHERE integration_id=?1 AND action=?2)",
            params![p.integration_id, p.action],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    if !allowed {
        return Err(Error(StatusCode::FORBIDDEN, "ACTION_NOT_ALLOWED"));
    }
    let old=tx.query_row("SELECT id,salt,difficulty,expires FROM challenges WHERE integration_id=?1 AND action=?2 AND context=?3",params![p.integration_id,p.action,p.context_id],challenge).optional().map_err(db_error)?;
    if let Some(c) = old {
        return if c.expires_at > now() {
            Ok(c)
        } else {
            Err(bad("CHALLENGE_EXPIRED"))
        };
    }
    let count: u32 = tx
        .query_row(
            "SELECT COUNT(*) FROM challenges WHERE integration_id=?1 AND expires>?2",
            params![p.integration_id, now()],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    if count >= app.config.max_active_challenges {
        return Err(Error(StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED"));
    }
    let c = Challenge {
        challenge_id: core::random(),
        algorithm: "sha256-pow-v1".into(),
        salt: core::random(),
        difficulty_bits: app.config.difficulty_bits,
        expires_at: now() + app.config.challenge_ttl_seconds,
    };
    tx.execute("INSERT INTO challenges(id,integration_id,action,context,salt,difficulty,expires) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![c.challenge_id,p.integration_id,p.action,p.context_id,c.salt,c.difficulty_bits,c.expires_at]).map_err(db_error)?;
    tx.commit().map_err(db_error)?;
    Ok(c)
}
pub fn solution(app: &Shared, p: Solution) -> Result<Value, Error> {
    solution_with_origin(app, p, None)
}
pub(crate) fn solution_with_origin(
    app: &Shared,
    p: Solution,
    origin: Option<&str>,
) -> Result<Value, Error> {
    if core::decode(&p.challenge_id, 32).is_none() || core::decode(&p.nonce, 8).is_none() {
        return Err(bad("INVALID_REQUEST"));
    }
    let mut db = app.db.lock().unwrap();
    let tx = db.transaction().map_err(db_error)?;
    let c = tx
        .query_row(
            "SELECT id,salt,difficulty,expires FROM challenges WHERE id=?1",
            [&p.challenge_id],
            challenge,
        )
        .optional()
        .map_err(db_error)?
        .ok_or(bad("CHALLENGE_NOT_FOUND"))?;
    crate::website::gate(&tx, app, &c.challenge_id, origin)?;
    let browser_required: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM browser_policies p JOIN challenges c ON c.integration_id=p.integration_id WHERE c.id=?1)",
        [&c.challenge_id], |r| r.get(0)).map_err(db_error)?;
    let visual_passed = crate::visual::passed(&tx, &c.challenge_id)?;
    if browser_required && !visual_passed {
        let signals = p
            .browser
            .as_ref()
            .ok_or(Error(StatusCode::FORBIDDEN, "BROWSER_SIGNALS_REQUIRED"))?;
        if signals.webdriver || !signals.cookies_enabled || signals.languages_count == 0 {
            return Err(Error(StatusCode::FORBIDDEN, "AUTOMATION_SUSPECTED"));
        }
    }
    let behavior_required: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM behavior_policies p JOIN challenges c ON c.integration_id=p.integration_id WHERE c.id=?1)",
        [&c.challenge_id], |r| r.get(0)).map_err(db_error)?;
    if behavior_required && !visual_passed {
        let signals = p
            .behavior
            .as_ref()
            .ok_or(Error(StatusCode::FORBIDDEN, "BEHAVIOR_SIGNALS_REQUIRED"))?;
        if !signals.acceptable(
            &c.challenge_id,
            app.config.challenge_ttl_seconds as u32 * 1000,
        ) {
            return Err(Error(StatusCode::FORBIDDEN, "BEHAVIOR_REJECTED"));
        }
    }
    let (solved, expires, consumed): (Option<i64>, Option<i64>, Option<i64>) = tx
        .query_row(
            "SELECT solved_at,token_expires,consumed_at FROM challenges WHERE id=?1",
            [&c.challenge_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(db_error)?;
    let t = now();
    if c.expires_at <= t {
        return Err(bad("CHALLENGE_EXPIRED"));
    }
    if !core::verify(&c, &p.nonce) {
        return Err(bad("INVALID_SOLUTION"));
    }
    if consumed.is_some() {
        return Err(bad("INVALID_TOKEN"));
    }
    let token = app.token(&c.challenge_id);
    let expiry = expires.unwrap_or(t + app.config.token_ttl_seconds);
    if expiry <= t {
        return Err(bad("TOKEN_EXPIRED"));
    }
    if solved.is_none() {
        tx.execute(
            "UPDATE challenges SET solved_at=?1,token_hash=?2,token_expires=?3 WHERE id=?4",
            params![t, core::hash(token.as_bytes()), expiry, c.challenge_id],
        )
        .map_err(db_error)?;
    }
    tx.commit().map_err(db_error)?;
    Ok(json!({"verification_token":token,"expires_at":expiry}))
}
pub fn consume(app: &Shared, headers: &HeaderMap, p: Consume) -> Result<Value, Error> {
    app.auth(headers, &p.integration_id)?;
    if !valid(&p.action)
        || !valid(&p.context_id)
        || !valid(&p.request_id)
        || core::decode(&p.verification_token, 32).is_none()
    {
        return Err(bad("INVALID_REQUEST"));
    }
    app.limit(format!("consume:{}", p.integration_id))?;
    let mut db = app.db.lock().unwrap();
    let tx = db.transaction().map_err(db_error)?;
    type Row = (String, String, String, i64, Option<i64>, Option<String>);
    let row:Option<Row>=tx.query_row("SELECT integration_id,action,context,token_expires,consumed_at,request_id FROM challenges WHERE token_hash=?1",[core::hash(p.verification_token.as_bytes())],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional().map_err(db_error)?;
    let (integration, action, context, expires, consumed, request) =
        row.ok_or(bad("INVALID_TOKEN"))?;
    if integration != p.integration_id {
        return Err(bad("INVALID_TOKEN"));
    }
    if action != p.action || context != p.context_id {
        return Err(bad("CONTEXT_MISMATCH"));
    }
    if let Some(t) = consumed {
        if request.as_deref() != Some(&p.request_id) {
            return Err(bad("TOKEN_ALREADY_CONSUMED"));
        }
        if now() >= t + app.config.retention_seconds {
            return Err(bad("TOKEN_EXPIRED"));
        }
    } else {
        if expires <= now() {
            return Err(bad("TOKEN_EXPIRED"));
        }
        tx.execute(
            "UPDATE challenges SET consumed_at=?1,request_id=?2 WHERE token_hash=?3",
            params![
                now(),
                p.request_id,
                core::hash(p.verification_token.as_bytes())
            ],
        )
        .map_err(db_error)?;
    }
    tx.commit().map_err(db_error)?;
    Ok(json!({"valid":true,"action":action,"context_id":context}))
}
/// Evaluate persistent per-integration / transport-source history before issuing tokens.
/// Forwarded headers and client-supplied addresses are deliberately not trusted.
pub fn solution_from(app: &Shared, source: std::net::IpAddr, p: Solution) -> Result<Value, Error> {
    solution_from_origin(app, source, p, None)
}
pub(crate) fn solution_from_origin(
    app: &Shared,
    source: std::net::IpAddr,
    p: Solution,
    origin: Option<&str>,
) -> Result<Value, Error> {
    let mut mac = Hmac::<Sha256>::new_from_slice(&app.key).unwrap();
    mac.update(b"gateveriq:source:v1:");
    mac.update(source.to_canonical().to_string().as_bytes());
    let source_key = B64.encode(mac.finalize().into_bytes());
    let minute = now() / 60;
    let integration: Option<String> = {
        let mut db = app.db.lock().unwrap();
        let tx = db.transaction().map_err(db_error)?;
        let integration: Option<String> = tx.query_row(
            "SELECT c.integration_id FROM challenges c JOIN history_policies p ON p.integration_id=c.integration_id WHERE c.id=?1",
            [&p.challenge_id], |r| r.get(0)).optional().map_err(db_error)?;
        if let Some(id) = integration.as_ref() {
            let (attempts, failures): (i64, i64) = tx.query_row(
                "SELECT COALESCE(SUM(CASE WHEN minute=?3 THEN attempts ELSE 0 END),0),COALESCE(SUM(failures),0) FROM attempt_history WHERE integration_id=?1 AND source=?2 AND minute BETWEEN ?3-9 AND ?3",
                params![id, source_key, minute], |r| Ok((r.get(0)?,r.get(1)?))).map_err(db_error)?;
            if failures >= i64::from(app.config.website.failure_limit) {
                return Err(Error(
                    StatusCode::TOO_MANY_REQUESTS,
                    "ATTEMPT_HISTORY_BLOCKED",
                ));
            }
            if attempts >= i64::from(app.config.website.attempts_per_minute) {
                return Err(Error(StatusCode::TOO_MANY_REQUESTS, "ATTEMPT_RATE_LIMITED"));
            }
            // Reserve before verification so concurrent requests cannot bypass the limit.
            tx.execute("INSERT INTO attempt_history(integration_id,source,minute,attempts) VALUES(?1,?2,?3,1) ON CONFLICT(integration_id,source,minute) DO UPDATE SET attempts=attempts+1",
                params![id, source_key, minute]).map_err(db_error)?;
        }
        tx.commit().map_err(db_error)?;
        integration
    };
    let result = solution_with_origin(app, p, origin);
    if let (Some(id), Err(error)) = (integration, &result) {
        // Count client verification failures, never infrastructure errors.
        if error.0.is_client_error() && error.0 != StatusCode::TOO_MANY_REQUESTS {
            app.db.lock().unwrap().execute(
                "UPDATE attempt_history SET failures=failures+1 WHERE integration_id=?1 AND source=?2 AND minute=?3",
                params![id, source_key, minute]).map_err(db_error)?;
        }
    }
    result
}
async fn issue(
    State(a): State<Shared>,
    ConnectInfo(ip): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Json(p): Json<Prepare>,
) -> Result<Json<Challenge>, Error> {
    a.limit(format!("internal:{}", ip.ip()))?;
    prepare(&a, &h, p).map(Json)
}
async fn submit(
    State(a): State<Shared>,
    ConnectInfo(ip): ConnectInfo<SocketAddr>,
    Json(p): Json<Solution>,
) -> Result<Json<Value>, Error> {
    a.limit(format!("ip:{}", ip.ip()))?;
    a.limit(format!("challenge:{}", p.challenge_id))?;
    solution_from(&a, ip.ip(), p).map(Json)
}
async fn check(
    State(a): State<Shared>,
    ConnectInfo(ip): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Json(p): Json<Consume>,
) -> Result<Json<Value>, Error> {
    a.limit(format!("internal:{}", ip.ip()))?;
    consume(&a, &h, p).map(Json)
}
pub fn routers(a: Shared) -> (Router, Router) {
    let public = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/public/v1/solutions", post(submit))
        .route(
            "/widget.js",
            get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, "text/javascript")],
                    include_str!("../web/widget.js"),
                )
            }),
        )
        .route(
            "/worker.js",
            get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, "text/javascript")],
                    include_str!("../web/worker.js"),
                )
            }),
        )
        .layer(DefaultBodyLimit::max(4096))
        .with_state(a.clone());
    let internal = Router::new()
        .route("/internal/v1/challenges", post(issue))
        .route("/internal/v1/verifications/consume", post(check))
        .layer(DefaultBodyLimit::max(4096))
        .with_state(a.clone());
    (
        crate::website::public_routes(public, a.clone()),
        crate::website::internal_routes(internal, a),
    )
}
pub async fn serve(a: Shared) -> Result<(), Box<dyn std::error::Error>> {
    let public = tokio::net::TcpListener::bind(a.config.public_listen).await?;
    let internal = tokio::net::TcpListener::bind(a.config.internal_listen).await?;
    let cleanup = a.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            if cleanup.cleanup().is_err() {
                eprintln!("cleanup failed");
            }
        }
    });
    let (p, i) = routers(a);
    tokio::try_join!(
        axum::serve(
            public,
            p.into_make_service_with_connect_info::<SocketAddr>()
        ),
        axum::serve(
            internal,
            i.into_make_service_with_connect_info::<SocketAddr>()
        )
    )?;
    Ok(())
}
