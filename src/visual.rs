//! Experimental local illustration-matching challenge. The small static corpus is
//! a visual prototype, not a robust classifier-resistant CAPTCHA.
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
use base64::{engine::general_purpose::STANDARD, Engine};
use rand::{seq::SliceRandom, Rng};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{net::SocketAddr, sync::OnceLock};

pub fn migrate(db: &Connection) -> rusqlite::Result<()> {
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
fn new_task(expires: i64, previous_reference: Option<&str>, remaining: u32) -> (Value, u32) {
    let mut rng = rand::thread_rng();
    let bank = illustration_bank();
    let eligible: Vec<usize> = (0..bank.len())
        .filter(|k| {
            !bank[*k]
                .iter()
                .any(|image| Some(image.as_str()) == previous_reference)
        })
        .collect();
    let target = *eligible
        .choose(&mut rng)
        .expect("multiple illustration categories");
    let correct_count = rng.gen_range(3..=4);
    let mut variants: Vec<usize> = (0..4).collect();
    variants.shuffle(&mut rng);
    // Use distinct illustrations within a board, rather than duplicate pictures.
    let mut choices: Vec<(usize, usize)> = variants[..correct_count]
        .iter()
        .map(|v| (target, *v))
        .collect();
    let mut others: Vec<(usize, usize)> = (0..bank.len())
        .filter(|k| *k != target)
        .flat_map(|k| (0..4).map(move |v| (k, v)))
        .collect();
    others.shuffle(&mut rng);
    choices.extend_from_slice(&others[..16 - correct_count]);
    choices.shuffle(&mut rng);
    let answer: u32 = choices
        .iter()
        .enumerate()
        .filter(|(_, (kind, _))| *kind == target)
        .map(|(i, _)| 1u32 << i)
        .sum();
    let task_id = core::random();
    let reference = &bank[target][variants[3]];
    let images: Vec<&String> = choices
        .iter()
        .map(|(kind, variant)| &bank[*kind][*variant])
        .collect();
    let payload = json!({"task_id":task_id,"expires_at":expires,
            "instruction_key":"match_reference",
            "instruction":"Select all illustrations showing the same type of object as the reference. Colors and poses may differ.",
            "reference":reference,"images":images,"max_attempts":remaining});
    (payload, answer)
}

pub fn offer(app: &Shared, id: &str, origin: &str) -> Result<Value, Error> {
    let mut db = app.db.lock().unwrap();
    let tx = db.transaction().map_err(db_error)?;
    let (integration, account) = crate::website::binding(&tx, id, Some(origin))?
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
        let previous:Option<String> = tx.query_row("SELECT v.payload FROM visual_tasks v JOIN website_attempts w ON w.challenge_id=v.challenge_id JOIN challenges c ON c.id=v.challenge_id WHERE w.account_hash=?1 AND c.integration_id=?2 ORDER BY v.rowid DESC LIMIT 1",params![account,integration],|r|r.get(0)).optional().map_err(db_error)?;
        let previous: Option<Value> = previous
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|_| Error(StatusCode::SERVICE_UNAVAILABLE, "SERVICE_UNAVAILABLE"))?;
        let previous_reference = previous.as_ref().and_then(|v| v["reference"].as_str());
        let (payload, answer) = new_task(expires, previous_reference, 3);
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
    pub selected: Vec<u8>,
}
pub fn solve(app: &Shared, origin: &str, p: VisualSolution) -> Result<Value, Error> {
    if core::decode(&p.task_id, 32).is_none() || p.selected.len() > 16 {
        return Err(bad("INVALID_REQUEST"));
    }
    let mut mask = 0u32;
    for index in &p.selected {
        if *index >= 16 || mask & (1 << index) != 0 {
            return Err(bad("INVALID_REQUEST"));
        }
        mask |= 1 << index;
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
        if answer != mask {
            if !was_passed {
                if attempts + 1 < 3 {
                    let previous: String = tx
                        .query_row(
                            "SELECT payload FROM visual_tasks WHERE challenge_id=?1",
                            [&p.challenge_id],
                            |r| r.get(0),
                        )
                        .map_err(db_error)?;
                    let previous: Value = serde_json::from_str(&previous).map_err(|_| {
                        Error(StatusCode::SERVICE_UNAVAILABLE, "SERVICE_UNAVAILABLE")
                    })?;
                    let (replacement, answer) =
                        new_task(expires, previous["reference"].as_str(), 3 - attempts - 1);
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

/// Runtime uses pre-sized local tiles; no image decoding on a request path.
/// Category identifiers and filenames are never included in task metadata.
fn illustration_bank() -> &'static Vec<Vec<String>> {
    static BANK: OnceLock<Vec<Vec<String>>> = OnceLock::new();
    BANK.get_or_init(|| {
        let categories: [[&[u8]; 4]; 6] = [
            [
                include_bytes!("../assets/visual/tiles/cats-0.png"),
                include_bytes!("../assets/visual/tiles/cats-1.png"),
                include_bytes!("../assets/visual/tiles/cats-2.png"),
                include_bytes!("../assets/visual/tiles/cats-3.png"),
            ],
            [
                include_bytes!("../assets/visual/tiles/dogs-0.png"),
                include_bytes!("../assets/visual/tiles/dogs-1.png"),
                include_bytes!("../assets/visual/tiles/dogs-2.png"),
                include_bytes!("../assets/visual/tiles/dogs-3.png"),
            ],
            [
                include_bytes!("../assets/visual/tiles/bicycles-0.png"),
                include_bytes!("../assets/visual/tiles/bicycles-1.png"),
                include_bytes!("../assets/visual/tiles/bicycles-2.png"),
                include_bytes!("../assets/visual/tiles/bicycles-3.png"),
            ],
            [
                include_bytes!("../assets/visual/tiles/cars-0.png"),
                include_bytes!("../assets/visual/tiles/cars-1.png"),
                include_bytes!("../assets/visual/tiles/cars-2.png"),
                include_bytes!("../assets/visual/tiles/cars-3.png"),
            ],
            [
                include_bytes!("../assets/visual/tiles/flowers-0.png"),
                include_bytes!("../assets/visual/tiles/flowers-1.png"),
                include_bytes!("../assets/visual/tiles/flowers-2.png"),
                include_bytes!("../assets/visual/tiles/flowers-3.png"),
            ],
            [
                include_bytes!("../assets/visual/tiles/trees-0.png"),
                include_bytes!("../assets/visual/tiles/trees-1.png"),
                include_bytes!("../assets/visual/tiles/trees-2.png"),
                include_bytes!("../assets/visual/tiles/trees-3.png"),
            ],
        ];
        categories
            .iter()
            .map(|tiles| {
                tiles
                    .iter()
                    .map(|bytes| format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
                    .collect()
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacement_uses_another_object_category() {
        for category in illustration_bank() {
            let previous = &category[0];
            let (next, _) = new_task(service::now() + 120, Some(previous), 2);
            assert!(!category
                .iter()
                .any(|image| image == next["reference"].as_str().unwrap()));
            assert_eq!(next["max_attempts"], 2);
        }
    }
    #[test]
    fn illustration_assets_are_valid_and_distinct() {
        let bank = illustration_bank();
        assert_eq!(bank.len(), 6);
        let mut unique = std::collections::HashSet::new();
        for category in bank {
            assert_eq!(category.len(), 4);
            for data in category {
                assert!(unique.insert(data));
                let bytes = STANDARD
                    .decode(data.strip_prefix("data:image/png;base64,").unwrap())
                    .unwrap();
                let image = image::load_from_memory(&bytes).unwrap();
                assert_eq!((image.width(), image.height()), (160, 160));
            }
        }
    }
}
