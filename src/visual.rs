//! Expiring slider puzzles with server-side coordinates and a fixed answer budget.
use crate::puzzle;
use crate::{
    core,
    service::{self, bad, db_error, Error, Shared, Solution},
};
use axum::{
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::SocketAddr;

pub fn init_schema(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS visual_tasks(challenge_id TEXT PRIMARY KEY REFERENCES challenges(id) ON DELETE CASCADE,task_id TEXT UNIQUE NOT NULL,answer INTEGER NOT NULL,payload TEXT NOT NULL,expires INTEGER NOT NULL,attempts INTEGER NOT NULL DEFAULT 0,passed INTEGER NOT NULL DEFAULT 0);")
}
pub(crate) fn passed(tx: &Transaction<'_>, id: &str) -> Result<bool, Error> {
    tx.query_row("SELECT EXISTS(SELECT 1 FROM visual_tasks WHERE challenge_id=?1 AND passed=1 AND expires>?2)",params![id,service::now()],|r|r.get(0)).map_err(db_error)
}
pub(crate) fn pending(tx: &Transaction<'_>, id: &str) -> Result<bool, Error> {
    tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM visual_tasks WHERE challenge_id=?1 AND passed=0)",
        [id],
        |r| r.get(0),
    )
    .map_err(db_error)
}
pub fn offer(app: &Shared, id: &str, origin: &str) -> Result<Value, Error> {
    let mut db = app.db.lock().unwrap();
    let tx = db.transaction().map_err(db_error)?;
    crate::website::binding(&tx, id, Some(origin))?
        .ok_or(Error(StatusCode::FORBIDDEN, "ORIGIN_NOT_ALLOWED"))?;
    let c = tx
        .query_row(
            "SELECT id,salt,difficulty,expires FROM challenges WHERE id=?1",
            [id],
            |r| {
                Ok(core::Challenge {
                    challenge_id: r.get(0)?,
                    algorithm: "sha256-pow-v1".into(),
                    salt: r.get(1)?,
                    difficulty_bits: r.get(2)?,
                    expires_at: r.get(3)?,
                })
            },
        )
        .map_err(db_error)?;
    if c.expires_at <= service::now() {
        return Err(bad("CHALLENGE_EXPIRED"));
    }
    let existing: Option<(String, i64, i64)> = tx
        .query_row(
            "SELECT payload,expires,attempts FROM visual_tasks WHERE challenge_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(db_error)?;
    let visual = if let Some((payload, expires, attempts)) = existing {
        if expires <= service::now() {
            return Err(bad("VISUAL_EXPIRED"));
        }
        if attempts >= 3 {
            return Err(Error(
                StatusCode::TOO_MANY_REQUESTS,
                "VISUAL_ATTEMPTS_EXHAUSTED",
            ));
        }
        serde_json::from_str::<Value>(&payload)
            .map_err(|_| Error(StatusCode::SERVICE_UNAVAILABLE, "SERVICE_UNAVAILABLE"))?
    } else {
        let expires = c.expires_at.min(service::now() + 120);
        let (payload, answer) = puzzle::new_task(expires, 3);
        let task_id = payload["task_id"].as_str().unwrap();
        tx.execute("INSERT INTO visual_tasks(challenge_id,task_id,answer,payload,expires) VALUES(?1,?2,?3,?4,?5)",params![id,task_id,answer,payload.to_string(),expires]).map_err(db_error)?;
        payload
    };
    tx.commit().map_err(db_error)?;
    Ok(
        json!({"decision":"additional_verification_required","reason":"risk_policy","challenge":c,"visual":visual,"verification_token":null}),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisualSolution {
    pub challenge_id: String,
    pub task_id: String,
    pub nonce: String,
    pub position: u32,
}
pub fn solve(app: &Shared, origin: &str, p: VisualSolution) -> Result<Value, Error> {
    if core::decode(&p.task_id, 32).is_none() || p.position > puzzle::MAX_POSITION {
        return Err(bad("INVALID_REQUEST"));
    }
    {
        let mut db = app.db.lock().unwrap();
        let tx = db.transaction().map_err(db_error)?;
        crate::website::binding(&tx, &p.challenge_id, Some(origin))?
            .ok_or(Error(StatusCode::FORBIDDEN, "ORIGIN_NOT_ALLOWED"))?;
        let c = tx
            .query_row(
                "SELECT id,salt,difficulty,expires FROM challenges WHERE id=?1",
                [&p.challenge_id],
                |r| {
                    Ok(core::Challenge {
                        challenge_id: r.get(0)?,
                        algorithm: "sha256-pow-v1".into(),
                        salt: r.get(1)?,
                        difficulty_bits: r.get(2)?,
                        expires_at: r.get(3)?,
                    })
                },
            )
            .map_err(db_error)?;
        if c.expires_at <= service::now() {
            return Err(bad("CHALLENGE_EXPIRED"));
        }
        if !core::verify(&c, &p.nonce) {
            return Err(bad("INVALID_SOLUTION"));
        }
        let row:Option<(u32,i64,u32,bool)>=tx.query_row("SELECT answer,expires,attempts,passed FROM visual_tasks WHERE challenge_id=?1 AND task_id=?2",params![p.challenge_id,p.task_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(db_error)?;
        let (answer, expires, attempts, was_passed) = row.ok_or(bad("VISUAL_NOT_FOUND"))?;
        if expires <= service::now() {
            return Err(bad("VISUAL_EXPIRED"));
        }
        if attempts >= 3 && !was_passed {
            return Err(Error(
                StatusCode::TOO_MANY_REQUESTS,
                "VISUAL_ATTEMPTS_EXHAUSTED",
            ));
        }
        if answer.abs_diff(p.position) > puzzle::TOLERANCE {
            if !was_passed {
                if attempts + 1 < 3 {
                    let (replacement, answer) = puzzle::new_task(expires, 3 - attempts - 1);
                    tx.execute("UPDATE visual_tasks SET attempts=attempts+1,task_id=?2,answer=?3,payload=?4 WHERE challenge_id=?1",
                        params![p.challenge_id,replacement["task_id"].as_str().unwrap(),answer,replacement.to_string()]).map_err(db_error)?;
                } else {
                    tx.execute(
                        "UPDATE visual_tasks SET attempts=attempts+1 WHERE challenge_id=?1",
                        [&p.challenge_id],
                    )
                    .map_err(db_error)?;
                }
            }
            tx.commit().map_err(db_error)?;
            return Err(bad("INVALID_VISUAL_SOLUTION"));
        }
        tx.execute(
            "UPDATE visual_tasks SET passed=1 WHERE challenge_id=?1",
            [&p.challenge_id],
        )
        .map_err(db_error)?;
        tx.commit().map_err(db_error)?;
    }
    service::solution_with_origin(
        app,
        Solution {
            challenge_id: p.challenge_id,
            nonce: p.nonce,
            browser: None,
            behavior: None,
        },
        Some(origin),
    )
}
pub(crate) async fn submit(
    State(a): State<Shared>,
    ConnectInfo(ip): ConnectInfo<SocketAddr>,
    h: HeaderMap,
    Json(p): Json<VisualSolution>,
) -> Response {
    let origin = h.get("origin").and_then(|v| v.to_str().ok()).unwrap_or("");
    let p_id = p.challenge_id.clone();
    let result = (|| {
        a.limit(format!("ip:{}", ip.ip()))?;
        a.limit(format!("visual:{}", p.challenge_id))?;
        solve(&a, origin, p)
    })();
    crate::website::cors(
        match result {
            Ok(v) => Json(v).into_response(),
            Err(Error(_, "INVALID_VISUAL_SOLUTION")) => match offer(&a,&p_id,origin) {
                Ok(offered) => (StatusCode::BAD_REQUEST,Json(json!({"error":{"code":"INVALID_VISUAL_SOLUTION","retryable":false},"visual":offered["visual"]}))).into_response(),
                Err(e) => e.into_response(),
            },
            Err(e) => e.into_response(),
        },
        origin,
    )
}
