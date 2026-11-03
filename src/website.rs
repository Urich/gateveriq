//! Website attempts are created and reported exclusively by an authenticated backend.
use crate::{
    core,
    service::{self, bad, db_error, Error, Prepare, Shared, Solution},
};
use axum::{
    extract::{ConnectInfo, DefaultBodyLimit, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::SocketAddr;

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct WebsitePolicy {
    pub history_retention_seconds: i64,
    pub attempts_per_minute: u32,
    pub failure_limit: u32,
    pub account_failure_limit: u32,
}
impl Default for WebsitePolicy {
    fn default() -> Self {
        Self {
            history_retention_seconds: 86400,
            attempts_per_minute: 20,
            failure_limit: 5,
            account_failure_limit: 5,
        }
    }
}
impl WebsitePolicy {
    pub fn valid(&self) -> bool {
        (600..=86400).contains(&self.history_retention_seconds)
            && (1..=10000).contains(&self.attempts_per_minute)
            && (1..=10000).contains(&self.failure_limit)
            && (1..=10000).contains(&self.account_failure_limit)
    }
}
pub fn migrate(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS website_origins(integration_id TEXT NOT NULL REFERENCES integrations(id),origin TEXT NOT NULL,PRIMARY KEY(integration_id,origin));
    CREATE TABLE IF NOT EXISTS website_attempts(challenge_id TEXT PRIMARY KEY REFERENCES challenges(id) ON DELETE CASCADE,account_hash TEXT NOT NULL,origin TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS login_outcomes(challenge_id TEXT PRIMARY KEY,integration_id TEXT NOT NULL REFERENCES integrations(id),account_hash TEXT NOT NULL,success INTEGER NOT NULL,created INTEGER NOT NULL);
    CREATE INDEX IF NOT EXISTS login_account_history ON login_outcomes(integration_id,account_hash,created);
    CREATE INDEX IF NOT EXISTS login_outcome_expiry ON login_outcomes(created);")
}
pub fn cleanup(app: &service::App) -> rusqlite::Result<()> {
    app.db.lock().unwrap().execute("DELETE FROM login_outcomes WHERE rowid IN (SELECT rowid FROM login_outcomes WHERE created<=?1 LIMIT 500)", [service::now()-app.config.website.history_retention_seconds])?;
    Ok(())
}
pub fn validate_origin(value: &str) -> Result<(), &'static str> {
    let u = reqwest::Url::parse(value).map_err(|_| "invalid origin")?;
    if !matches!(u.scheme(), "http" | "https")
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
        || u.origin().ascii_serialization() != value
    {
        return Err(
            "origin must be canonical scheme://host[:port], without path or trailing slash",
        );
    }
    if u.scheme() == "http" && !matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")) {
        return Err("non-local website origins must use HTTPS");
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebsitePrepare {
    pub integration_id: String,
    pub action: String,
    pub context_id: String,
    pub account_id: String,
    pub origin: String,
}

pub fn prepare(app: &Shared, headers: &HeaderMap, p: WebsitePrepare) -> Result<Value, Error> {
    app.auth(headers, &p.integration_id)?;
    if core::decode(&p.context_id, 32).is_none()
        || p.account_id.is_empty()
        || p.account_id.len() > 128
        || p.account_id.chars().any(char::is_control)
        || validate_origin(&p.origin).is_err()
    {
        return Err(bad("INVALID_REQUEST"));
    }
    let account = app.pseudonym(b"gateveriq:account:v1:", &p.integration_id, &p.account_id);
    let needs_visual = {
        let db = app.db.lock().unwrap();
        let allowed: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM website_origins WHERE integration_id=?1 AND origin=?2)", params![p.integration_id,p.origin], |r| r.get(0)).map_err(db_error)?;
        if !allowed {
            return Err(Error(StatusCode::FORBIDDEN, "ORIGIN_NOT_ALLOWED"));
        }
        let failures: i64 = db.query_row("SELECT COUNT(*) FROM login_outcomes WHERE integration_id=?1 AND account_hash=?2 AND action=?4 AND success=0 AND created>?3", params![p.integration_id,account,service::now()-app.config.website.history_retention_seconds,p.action], |r| r.get(0)).map_err(db_error)?;
        failures >= i64::from(app.config.website.account_failure_limit)
    };
    let c = service::prepare(
        app,
        headers,
        Prepare {
            integration_id: p.integration_id,
            action: p.action,
            context_id: p.context_id,
        },
    )?;
    let mut db = app.db.lock().unwrap();
    let tx = db.transaction().map_err(db_error)?;
    tx.execute(
        "INSERT OR IGNORE INTO website_attempts VALUES(?1,?2,?3)",
        params![c.challenge_id, account, p.origin],
    )
    .map_err(db_error)?;
    let matches: bool = tx
        .query_row(
            "SELECT account_hash=?2 AND origin=?3 FROM website_attempts WHERE challenge_id=?1",
            params![c.challenge_id, account, p.origin],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    if !matches {
        return Err(bad("CONTEXT_MISMATCH"));
    }
    let pending_visual = crate::visual::pending(&tx, &c.challenge_id)?;
    let always: bool = tx
        .query_row(
            "SELECT visual_policy='always' FROM challenges WHERE id=?1",
            [&c.challenge_id],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    tx.commit().map_err(db_error)?;
    drop(db);
    if always || needs_visual || pending_visual {
        return crate::visual::offer(app, &c.challenge_id, &p.origin);
    }
    Ok(json!({"decision":"verify", "challenge":c}))
}

pub(crate) fn binding(
    tx: &Transaction<'_>,
    id: &str,
    origin: Option<&str>,
) -> Result<Option<(String, String)>, Error> {
    let enabled: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM challenges c JOIN website_origins o ON o.integration_id=c.integration_id WHERE c.id=?1)", [id], |r| r.get(0)).map_err(db_error)?;
    if !enabled {
        return Ok(None);
    }
    let origin = origin.ok_or(Error(StatusCode::FORBIDDEN, "ORIGIN_NOT_ALLOWED"))?;
    let row: Option<(String,String)> = tx.query_row("SELECT c.integration_id,w.account_hash FROM challenges c JOIN website_attempts w ON w.challenge_id=c.id JOIN website_origins o ON o.integration_id=c.integration_id AND o.origin=w.origin WHERE c.id=?1 AND w.origin=?2",params![id,origin], |r| Ok((r.get(0)?,r.get(1)?))).optional().map_err(db_error)?;
    let bound = row.ok_or(Error(StatusCode::FORBIDDEN, "ORIGIN_NOT_ALLOWED"))?;
    Ok(Some(bound))
}

/// Called inside the token-issuance transaction, also for legacy endpoints.
pub(crate) fn gate(
    tx: &Transaction<'_>,
    app: &Shared,
    id: &str,
    origin: Option<&str>,
) -> Result<(), Error> {
    let (always, action): (bool, String) = tx
        .query_row(
            "SELECT visual_policy='always',action FROM challenges WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(db_error)?;
    let bound = binding(tx, id, origin)?;
    if always && !crate::visual::passed(tx, id)? {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "ADDITIONAL_VERIFICATION_REQUIRED",
        ));
    }
    let Some((integration, account)) = bound else {
        return Ok(());
    };
    if crate::visual::passed(tx, id)? {
        return Ok(());
    }
    if crate::visual::pending(tx, id)? {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "ADDITIONAL_VERIFICATION_REQUIRED",
        ));
    }
    let failures:i64 = tx.query_row("SELECT COUNT(*) FROM login_outcomes WHERE integration_id=?1 AND account_hash=?2 AND action=?4 AND success=0 AND created>?3",params![integration,account,service::now()-app.config.website.history_retention_seconds,action], |r| r.get(0)).map_err(db_error)?;
    if failures >= i64::from(app.config.website.account_failure_limit) {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "ADDITIONAL_VERIFICATION_REQUIRED",
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoginOutcome {
    pub integration_id: String,
    pub action: String,
    pub context_id: String,
    pub request_id: String,
    pub success: bool,
}
pub fn outcome(app: &Shared, headers: &HeaderMap, p: LoginOutcome) -> Result<Value, Error> {
    app.auth(headers, &p.integration_id)?;
    app.limit(format!("outcome:{}", p.integration_id))?;
    let mut db = app.db.lock().unwrap();
    let tx = db.transaction().map_err(db_error)?;
    let row:Option<(String,String)> = tx.query_row("SELECT c.id,w.account_hash FROM challenges c JOIN website_attempts w ON w.challenge_id=c.id WHERE c.integration_id=?1 AND c.action=?2 AND c.context=?3 AND c.request_id=?4 AND c.consumed_at>?5",params![p.integration_id,p.action,p.context_id,p.request_id,service::now()-app.config.retention_seconds],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(db_error)?;
    let (id, account) = row.ok_or(bad("VERIFICATION_NOT_CONSUMED"))?;
    tx.execute(
        "INSERT OR IGNORE INTO login_outcomes(challenge_id,integration_id,account_hash,success,created,action) VALUES(?1,?2,?3,?4,?5,?6)",
        params![id, p.integration_id, account, p.success, service::now(), p.action],
    )
    .map_err(db_error)?;
    let existing: bool = tx
        .query_row(
            "SELECT success FROM login_outcomes WHERE challenge_id=?1",
            [id],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    if existing != p.success {
        return Err(bad("OUTCOME_CONFLICT"));
    }
    tx.commit().map_err(db_error)?;
    Ok(json!({"recorded":true}))
}
async fn issue(
    State(a): State<Shared>,
    ConnectInfo(ip): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Json(p): Json<WebsitePrepare>,
) -> Result<Json<Value>, Error> {
    a.limit(format!("internal:{}", ip.ip()))?;
    prepare(&a, &h, p).map(Json)
}
async fn report(
    State(a): State<Shared>,
    ConnectInfo(ip): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Json(p): Json<LoginOutcome>,
) -> Result<Json<Value>, Error> {
    a.limit(format!("internal:{}", ip.ip()))?;
    outcome(&a, &h, p).map(Json)
}
pub(crate) fn cors(mut response: Response, origin: &str) -> Response {
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    if let Ok(value) = HeaderValue::from_str(origin) {
        response
            .headers_mut()
            .insert("access-control-allow-origin", value);
    }
    response
        .headers_mut()
        .insert("vary", HeaderValue::from_static("Origin"));
    response
}
pub(crate) async fn preflight(State(a): State<Shared>, h: HeaderMap) -> Response {
    let origin = h.get("origin").and_then(|v| v.to_str().ok()).unwrap_or("");
    let allowed: Result<bool, _> = a.db.lock().unwrap().query_row(
        "SELECT EXISTS(SELECT 1 FROM website_origins WHERE origin=?1)",
        [origin],
        |r| r.get(0),
    );
    if !matches!(allowed, Ok(true)) {
        return Error(StatusCode::FORBIDDEN, "ORIGIN_NOT_ALLOWED").into_response();
    }
    let mut response = cors(StatusCode::NO_CONTENT.into_response(), origin);
    response.headers_mut().insert(
        "access-control-allow-methods",
        HeaderValue::from_static("POST"),
    );
    response.headers_mut().insert(
        "access-control-allow-headers",
        HeaderValue::from_static("Content-Type"),
    );
    response
}
async fn submit(
    State(a): State<Shared>,
    ConnectInfo(ip): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Json(p): Json<Solution>,
) -> Response {
    let origin = h.get("origin").and_then(|v| v.to_str().ok()).unwrap_or("");
    let challenge_id = p.challenge_id.clone();
    let result = (|| {
        a.limit(format!("ip:{}", ip.ip()))?;
        a.limit(format!("challenge:{}", p.challenge_id))?;
        // Gate before recording history so unrelated origins cannot poison it.
        {
            let mut db = a.db.lock().unwrap();
            let tx = db.transaction().map_err(db_error)?;
            binding(&tx, &p.challenge_id, Some(origin))?;
        }
        service::solution_from_origin(&a, ip.ip(), p, Some(origin))
    })();
    let response = match result {
        Ok(value) => Json(value).into_response(),
        Err(Error(
            _,
            "ADDITIONAL_VERIFICATION_REQUIRED"
            | "ATTEMPT_HISTORY_BLOCKED"
            | "AUTOMATION_SUSPECTED"
            | "BEHAVIOR_REJECTED",
        )) => match crate::visual::offer(&a, &challenge_id, origin) {
            Ok(v) => (StatusCode::FORBIDDEN, Json(v)).into_response(),
            Err(e) => e.into_response(),
        },
        Err(e) => e.into_response(),
    };
    cors(response, origin)
}
pub fn public_routes(router: Router, app: Shared) -> Router {
    router.merge(
        Router::new()
            .route("/public/v1/web/solutions", post(submit).options(preflight))
            .route(
                "/public/v1/web/visual/solutions",
                post(crate::visual::submit).options(preflight),
            )
            .layer(DefaultBodyLimit::max(4096))
            .with_state(app),
    )
}
pub fn internal_routes(router: Router, app: Shared) -> Router {
    router.merge(
        Router::new()
            .route("/internal/v1/web/attempts", post(issue))
            .route("/internal/v1/web/outcomes", post(report))
            .layer(DefaultBodyLimit::max(4096))
            .with_state(app),
    )
}
