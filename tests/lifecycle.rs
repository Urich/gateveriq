use axum::http::{HeaderMap, HeaderValue, StatusCode};
use gateveriq::{core, service::*};
fn setup() -> (tempfile::TempDir, std::sync::Arc<App>, String, HeaderMap) {
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("key");
    std::fs::write(&key, core::random()).unwrap();
    let c = Config {
        website: Default::default(),
        database: dir.path().join("db").to_string_lossy().into(),
        token_key_file: key.to_string_lossy().into(),
        public_listen: "127.0.0.1:0".parse().unwrap(),
        internal_listen: "127.0.0.1:0".parse().unwrap(),
        difficulty_bits: 4,
        challenge_ttl_seconds: 120,
        token_ttl_seconds: 60,
        retention_seconds: 300,
        requests_per_minute: 1000,
        max_active_challenges: 10,
    };
    let a = App::open(c).unwrap();
    let (id, secret) = a.create_integration("test", &["register".into()]).unwrap();
    let mut h = HeaderMap::new();
    h.insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {secret}")).unwrap(),
    );
    (dir, a, id, h)
}
fn issue(a: &std::sync::Arc<App>, id: &str, h: &HeaderMap, ctx: &str) -> core::Challenge {
    prepare(
        a,
        h,
        Prepare {
            integration_id: id.into(),
            action: "register".into(),
            context_id: ctx.into(),
        },
    )
    .unwrap()
}
fn request(id: &str, t: &str, ctx: &str, r: &str) -> Consume {
    Consume {
        integration_id: id.into(),
        verification_token: t.into(),
        action: "register".into(),
        context_id: ctx.into(),
        request_id: r.into(),
    }
}
fn token(a: &std::sync::Arc<App>, c: &core::Challenge) -> String {
    let nonce = core::solve(c).unwrap();
    solution(
        a,
        Solution {
            behavior: None,
            browser: None,
            challenge_id: c.challenge_id.clone(),
            nonce,
        },
    )
    .unwrap()["verification_token"]
        .as_str()
        .unwrap()
        .into()
}
#[test]
fn lifecycle_and_retry() {
    let (_d, a, id, h) = setup();
    let c = issue(&a, &id, &h, "ctx");
    assert_eq!(c.challenge_id, issue(&a, &id, &h, "ctx").challenge_id);
    let t = token(&a, &c);
    assert_eq!(t, token(&a, &c));
    assert_eq!(
        consume(&a, &h, request(&id, &t, "wrong", "r"))
            .unwrap_err()
            .1,
        "CONTEXT_MISMATCH"
    );
    assert!(consume(&a, &h, request(&id, &t, "ctx", "r")).is_ok());
    assert!(consume(&a, &h, request(&id, &t, "ctx", "r")).is_ok());
    assert_eq!(
        consume(&a, &h, request(&id, &t, "ctx", "other"))
            .unwrap_err()
            .1,
        "TOKEN_ALREADY_CONSUMED"
    );
}
#[test]
fn expiry() {
    let (_d, a, id, h) = setup();
    let c = issue(&a, &id, &h, "ctx");
    let nonce = core::solve(&c).unwrap();
    a.db.lock()
        .unwrap()
        .execute("UPDATE challenges SET expires=0", [])
        .unwrap();
    assert_eq!(
        solution(
            &a,
            Solution {
                behavior: None,
                browser: None,
                challenge_id: c.challenge_id,
                nonce
            }
        )
        .unwrap_err()
        .1,
        "CHALLENGE_EXPIRED"
    );
}
#[test]
fn token_expiry_and_restart() {
    let (_d, a, id, h) = setup();
    let c = issue(&a, &id, &h, "ctx");
    let t = token(&a, &c);
    let b = App::open(a.config.clone()).unwrap();
    assert_eq!(t, token(&b, &c));
    b.db.lock()
        .unwrap()
        .execute("UPDATE challenges SET token_expires=0", [])
        .unwrap();
    assert_eq!(
        consume(&b, &h, request(&id, &t, "ctx", "r")).unwrap_err().1,
        "TOKEN_EXPIRED"
    );
}
#[test]
fn auth_and_isolation() {
    let (_d, a, id, h) = setup();
    assert_eq!(
        prepare(
            &a,
            &HeaderMap::new(),
            Prepare {
                integration_id: id.clone(),
                action: "register".into(),
                context_id: "ctx".into()
            }
        )
        .unwrap_err()
        .0,
        StatusCode::UNAUTHORIZED
    );
    let c = issue(&a, &id, &h, "ctx");
    let t = token(&a, &c);
    let (other, secret) = a.create_integration("other", &["register".into()]).unwrap();
    let mut h2 = HeaderMap::new();
    h2.insert("authorization", format!("Bearer {secret}").parse().unwrap());
    assert_eq!(
        consume(&a, &h2, request(&other, &t, "ctx", "r"))
            .unwrap_err()
            .1,
        "INVALID_TOKEN"
    );
}
#[test]
fn concurrent_consumption() {
    let (_d, a, id, h) = setup();
    let c = issue(&a, &id, &h, "ctx");
    let t = token(&a, &c);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|i| {
            let (a, id, h, t, b) = (a.clone(), id.clone(), h.clone(), t.clone(), barrier.clone());
            std::thread::spawn(move || {
                b.wait();
                consume(&a, &h, request(&id, &t, "ctx", &format!("r{i}"))).is_ok()
            })
        })
        .collect();
    assert_eq!(
        threads
            .into_iter()
            .filter_map(|t| t.join().ok())
            .filter(|v| *v)
            .count(),
        1
    );
}
#[test]
fn malformed_and_invalid_proof() {
    let (_d, a, id, h) = setup();
    let mut c = issue(&a, &id, &h, "ctx");
    assert!(!core::verify(&c, "bad"));
    c.difficulty_bits = 25;
    assert!(core::solve(&c).is_err());
    assert_eq!(
        solution(
            &a,
            Solution {
                behavior: None,
                browser: None,
                challenge_id: c.challenge_id,
                nonce: "bad".into()
            }
        )
        .unwrap_err()
        .1,
        "INVALID_REQUEST"
    );
}
#[tokio::test]
async fn routes_are_separate_and_body_is_bounded() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let (_d, a, _, _) = setup();
    let (public, _) = routers(a);
    let r = public
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/internal/v1/challenges")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    let r = public
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/public/v1/solutions")
                .header("content-type", "application/json")
                .extension(axum::extract::ConnectInfo(
                    "127.0.0.1:12345".parse::<std::net::SocketAddr>().unwrap(),
                ))
                .body(Body::from("x".repeat(5000)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[test]
fn browser_test_vector() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
    let c = core::Challenge {
        challenge_id: B64.encode([0u8; 32]),
        salt: B64.encode([1u8; 32]),
        algorithm: "sha256-pow-v1".into(),
        difficulty_bits: 8,
        expires_at: now() + 120,
    };
    assert!(core::verify(&c, "TwAAAAAAAAA"));
    assert_eq!(core::solve(&c).unwrap(), "TwAAAAAAAAA");
}

#[test]
fn browser_policy_gates_token_issuance() {
    let (_d, a, id, h) = setup();
    a.db.lock()
        .unwrap()
        .execute("INSERT INTO browser_policies VALUES(?1)", [&id])
        .unwrap();
    let c = issue(&a, &id, &h, "browser");
    let submit = |browser| {
        solution(
            &a,
            Solution {
                behavior: None,
                challenge_id: c.challenge_id.clone(),
                nonce: core::solve(&c).unwrap(),
                browser,
            },
        )
    };
    assert_eq!(submit(None).unwrap_err().1, "BROWSER_SIGNALS_REQUIRED");
    for (webdriver, cookies_enabled, languages_count) in
        [(true, true, 1), (false, false, 1), (false, true, 0)]
    {
        assert_eq!(
            submit(Some(BrowserSignals {
                webdriver,
                cookies_enabled,
                languages_count
            }))
            .unwrap_err()
            .1,
            "AUTOMATION_SUSPECTED"
        );
    }
    let issued: i64 =
        a.db.lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM challenges WHERE token_hash IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
    assert_eq!(issued, 0);
    let result = submit(Some(BrowserSignals {
        webdriver: false,
        cookies_enabled: true,
        languages_count: 1,
    }))
    .unwrap();
    assert_eq!(
        consume(
            &a,
            &h,
            request(
                &id,
                result["verification_token"].as_str().unwrap(),
                "browser",
                "login"
            )
        )
        .unwrap()["valid"],
        true
    );
}

#[test]
fn behavior_policy_requires_bound_plausible_interaction() {
    let (_d, a, id, h) = setup();
    a.db.lock()
        .unwrap()
        .execute("INSERT INTO behavior_policies VALUES(?1)", [&id])
        .unwrap();
    let c = issue(&a, &id, &h, "behavior");
    let submit = |behavior| {
        solution(
            &a,
            Solution {
                challenge_id: c.challenge_id.clone(),
                nonce: core::solve(&c).unwrap(),
                browser: None,
                behavior,
            },
        )
    };
    let report = || BehaviorSignals {
        challenge_id: c.challenge_id.clone(),
        elapsed_ms: 1200,
        activation_ms: 1000,
        trusted_activations: 1,
        pointer_events: 0,
        keyboard_events: 1,
        focus_changes: 1,
    };
    assert_eq!(submit(None).unwrap_err().1, "BEHAVIOR_SIGNALS_REQUIRED");
    for case in 0..6 {
        let mut b = report();
        match case {
            0 => b.challenge_id = core::random(),
            1 => b.activation_ms = 0,
            2 => b.activation_ms = 1300,
            3 => b.elapsed_ms = 600001,
            4 => b.trusted_activations = 0,
            _ => b.pointer_events = 4097,
        }
        assert_eq!(submit(Some(b)).unwrap_err().1, "BEHAVIOR_REJECTED");
    }
    let issued: i64 =
        a.db.lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM challenges WHERE token_hash IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
    assert_eq!(issued, 0);
    let result = submit(Some(report())).unwrap();
    assert_eq!(
        consume(
            &a,
            &h,
            request(
                &id,
                result["verification_token"].as_str().unwrap(),
                "behavior",
                "login"
            )
        )
        .unwrap()["valid"],
        true
    );
}

#[test]
fn persistent_failure_history_blocks_and_expires() {
    let (_d, a, id, h) = setup();
    a.db.lock()
        .unwrap()
        .execute("INSERT INTO history_policies VALUES(?1)", [&id])
        .unwrap();
    a.db.lock()
        .unwrap()
        .execute("INSERT INTO browser_policies VALUES(?1)", [&id])
        .unwrap();
    let c = issue(&a, &id, &h, "history");
    let make = || Solution {
        challenge_id: c.challenge_id.clone(),
        nonce: core::solve(&c).unwrap(),
        browser: None,
        behavior: None,
    };
    let ip = "192.0.2.1".parse().unwrap();
    for _ in 0..5 {
        assert_eq!(
            solution_from(&a, ip, make()).unwrap_err().1,
            "BROWSER_SIGNALS_REQUIRED"
        );
    }
    let reopened = App::open(a.config.clone()).unwrap();
    assert_eq!(
        solution_from(&reopened, ip, make()).unwrap_err().1,
        "ATTEMPT_HISTORY_BLOCKED"
    );
    // Another source and another integration do not inherit the block.
    assert_eq!(
        solution_from(&a, "192.0.2.2".parse().unwrap(), make())
            .unwrap_err()
            .1,
        "BROWSER_SIGNALS_REQUIRED"
    );
    let (other, _) = a.create_integration("other", &["register".into()]).unwrap();
    a.db.lock()
        .unwrap()
        .execute(
            "UPDATE challenges SET integration_id=?1 WHERE id=?2",
            rusqlite::params![other, c.challenge_id],
        )
        .unwrap();
    assert!(solution_from(&a, ip, make()).is_ok());
    a.db.lock()
        .unwrap()
        .execute(
            "UPDATE challenges SET integration_id=?1 WHERE id=?2",
            rusqlite::params![id, c.challenge_id],
        )
        .unwrap();
    let (attempts, failures): (i64, i64) =
        a.db.lock()
            .unwrap()
            .query_row(
                "SELECT SUM(attempts),SUM(failures) FROM attempt_history",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
    assert_eq!((attempts, failures), (6, 6));
    let raw_ips: i64 =
        a.db.lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM attempt_history WHERE source LIKE '%192.0.%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
    assert_eq!(raw_ips, 0);
    a.db.lock()
        .unwrap()
        .execute("UPDATE attempt_history SET minute=minute-11", [])
        .unwrap();
    assert_eq!(
        solution_from(&a, ip, make()).unwrap_err().1,
        "BROWSER_SIGNALS_REQUIRED"
    );
    a.db.lock()
        .unwrap()
        .execute(
            "UPDATE attempt_history SET minute=minute-1441 WHERE minute < ?1",
            [now() / 60 - 10],
        )
        .unwrap();
    a.cleanup().unwrap();
    let remaining: i64 =
        a.db.lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM attempt_history", [], |r| r.get(0))
            .unwrap();
    assert_eq!(remaining, 1);
}

#[test]
fn attempt_rate_counts_success_and_reserves_concurrently() {
    let (_d, a, id, h) = setup();
    a.db.lock()
        .unwrap()
        .execute("INSERT INTO history_policies VALUES(?1)", [&id])
        .unwrap();
    let c = issue(&a, &id, &h, "rate");
    let nonce = core::solve(&c).unwrap();
    let start = std::sync::Arc::new(std::sync::Barrier::new(25));
    let handles: Vec<_> = (0..25)
        .map(|_| {
            let a = a.clone();
            let c = c.clone();
            let nonce = nonce.clone();
            let start = start.clone();
            std::thread::spawn(move || {
                start.wait();
                solution_from(
                    &a,
                    "192.0.2.3".parse().unwrap(),
                    Solution {
                        challenge_id: c.challenge_id,
                        nonce,
                        browser: None,
                        behavior: None,
                    },
                )
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 20);
    assert!(results
        .iter()
        .filter_map(|r| r.as_ref().err())
        .all(|e| e.1 == "ATTEMPT_RATE_LIMITED"));
    let failures: i64 =
        a.db.lock()
            .unwrap()
            .query_row("SELECT SUM(failures) FROM attempt_history", [], |r| {
                r.get(0)
            })
            .unwrap();
    assert_eq!(failures, 0);
}

#[tokio::test]
async fn http_history_uses_peer_instead_of_forwarded_header() {
    use axum::{body::Body, extract::ConnectInfo, http::Request};
    use tower::ServiceExt;
    let (_d, a, id, h) = setup();
    a.db.lock()
        .unwrap()
        .execute("INSERT INTO history_policies VALUES(?1)", [&id])
        .unwrap();
    a.db.lock()
        .unwrap()
        .execute("INSERT INTO browser_policies VALUES(?1)", [&id])
        .unwrap();
    let c = issue(&a, &id, &h, "http-history");
    let (public, _) = routers(a);
    let body = serde_json::json!({"challenge_id":c.challenge_id,"nonce":core::solve(&c).unwrap()})
        .to_string();
    for i in 0..6 {
        let response = public
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/public/v1/solutions")
                    .header("content-type", "application/json")
                    .header("x-forwarded-for", format!("192.0.2.{}", i + 1))
                    .extension(ConnectInfo(
                        "127.0.0.1:12345".parse::<std::net::SocketAddr>().unwrap(),
                    ))
                    .body(Body::from(body.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            if i < 5 {
                StatusCode::FORBIDDEN
            } else {
                StatusCode::TOO_MANY_REQUESTS
            }
        );
    }
}

fn website_setup() -> (tempfile::TempDir, std::sync::Arc<App>, String, HeaderMap) {
    let (dir, a, id, h) = setup();
    let mut config = a.config.clone();
    config.website.account_failure_limit = 1;
    let a = App::open(config).unwrap();
    for table in ["browser_policies", "behavior_policies", "history_policies"] {
        a.db.lock()
            .unwrap()
            .execute(&format!("INSERT INTO {table} VALUES(?1)"), [&id])
            .unwrap();
    }
    a.db.lock()
        .unwrap()
        .execute(
            "INSERT INTO website_origins VALUES(?1,'https://site.example')",
            [&id],
        )
        .unwrap();
    (dir, a, id, h)
}
fn web_prepare(
    a: &std::sync::Arc<App>,
    h: &HeaderMap,
    id: &str,
    context: &str,
    account: &str,
) -> Result<serde_json::Value, Error> {
    gateveriq::website::prepare(
        a,
        h,
        gateveriq::website::WebsitePrepare {
            integration_id: id.into(),
            action: "register".into(),
            context_id: context.into(),
            account_id: account.into(),
            origin: "https://site.example".into(),
        },
    )
}
async fn web_submit(
    a: std::sync::Arc<App>,
    c: &core::Challenge,
    origin: Option<&str>,
    path: &str,
) -> (StatusCode, serde_json::Value) {
    use axum::{
        body::{to_bytes, Body},
        extract::ConnectInfo,
        http::Request,
    };
    use tower::ServiceExt;
    let (public, _) = routers(a);
    let mut req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .extension(ConnectInfo(
            "192.0.2.10:1234".parse::<std::net::SocketAddr>().unwrap(),
        ));
    if let Some(origin) = origin {
        req = req.header("origin", origin);
    }
    let body = serde_json::json!({"challenge_id":c.challenge_id,"nonce":core::solve(c).unwrap(),
        "browser":{"webdriver":false,"cookies_enabled":true,"languages_count":1},
        "behavior":{"challenge_id":c.challenge_id,"elapsed_ms":1000,"activation_ms":500,"trusted_activations":1,"pointer_events":0,"keyboard_events":1,"focus_changes":1}});
    let response = public
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1048576).await.unwrap()).unwrap();
    (status, value)
}
#[tokio::test]
async fn website_binding_outcome_and_step_up() {
    use gateveriq::website::{outcome, LoginOutcome};
    let (_dir, a, id, h) = website_setup();
    let ctx = core::random();
    let p = web_prepare(&a, &h, &id, &ctx, "opaque-account-1").unwrap();
    assert_eq!(
        p,
        web_prepare(&a, &h, &id, &ctx, "opaque-account-1").unwrap()
    );
    assert_eq!(
        web_prepare(&a, &h, &id, &ctx, "another-account")
            .unwrap_err()
            .1,
        "CONTEXT_MISMATCH"
    );
    let c: core::Challenge = serde_json::from_value(p["challenge"].clone()).unwrap();
    let report = |success| LoginOutcome {
        integration_id: id.clone(),
        action: "register".into(),
        context_id: ctx.clone(),
        request_id: "login-request".into(),
        success,
    };
    assert_eq!(
        outcome(&a, &h, report(false)).unwrap_err().1,
        "VERIFICATION_NOT_CONSUMED"
    );
    let (status, result) = web_submit(
        a.clone(),
        &c,
        Some("https://site.example"),
        "/public/v1/web/solutions",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let token = result["verification_token"].as_str().unwrap();
    assert_eq!(
        consume(&a, &h, request(&id, token, "wrong", "login-request"))
            .unwrap_err()
            .1,
        "CONTEXT_MISMATCH"
    );
    consume(&a, &h, request(&id, token, &ctx, "login-request")).unwrap();
    outcome(&a, &h, report(false)).unwrap();
    outcome(&a, &h, report(false)).unwrap();
    assert_eq!(
        outcome(&a, &h, report(true)).unwrap_err().1,
        "OUTCOME_CONFLICT"
    );
    let reopened = App::open(a.config.clone()).unwrap();
    let risk = web_prepare(&reopened, &h, &id, &core::random(), "opaque-account-1").unwrap();
    assert_eq!(risk["decision"], "additional_verification_required");
    assert!(risk["verification_token"].is_null());
    assert_eq!(
        web_prepare(&a, &h, &id, &core::random(), "another-account").unwrap()["decision"],
        "verify"
    );
    let raw: i64 =
        a.db.lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM login_outcomes WHERE account_hash='opaque-account-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
    assert_eq!(raw, 0);
    a.db.lock()
        .unwrap()
        .execute("UPDATE login_outcomes SET created=created-86401", [])
        .unwrap();
    assert_eq!(
        web_prepare(&a, &h, &id, &core::random(), "opaque-account-1").unwrap()["decision"],
        "verify"
    );
    a.cleanup().unwrap();
    let count: i64 =
        a.db.lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM login_outcomes", [], |r| r.get(0))
            .unwrap();
    assert_eq!(count, 0);
}
#[tokio::test]
async fn website_cannot_bypass_origin_or_use_unbound_challenge() {
    let (_dir, a, id, h) = website_setup();
    let ctx = core::random();
    let c: core::Challenge = serde_json::from_value(
        web_prepare(&a, &h, &id, &ctx, "account").unwrap()["challenge"].clone(),
    )
    .unwrap();
    for (origin, path) in [
        (None, "/public/v1/web/solutions"),
        (Some("https://evil.example"), "/public/v1/web/solutions"),
        (Some("https://site.example"), "/public/v1/solutions"),
    ] {
        let (status, result) = web_submit(a.clone(), &c, origin, path).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(result["error"]["code"], "ORIGIN_NOT_ALLOWED");
    }
    let unbound = issue(&a, &id, &h, &core::random());
    assert_eq!(
        web_submit(
            a.clone(),
            &unbound,
            Some("https://site.example"),
            "/public/v1/web/solutions"
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let bad = HeaderMap::new();
    assert_eq!(
        web_prepare(&a, &bad, &id, &core::random(), "account")
            .unwrap_err()
            .0,
        StatusCode::UNAUTHORIZED
    );
}
#[test]
fn website_origin_and_policy_validation() {
    use gateveriq::website::{validate_origin, WebsitePolicy};
    for origin in [
        "https://site.example",
        "http://127.0.0.1:8090",
        "https://site.example:8443",
    ] {
        assert!(validate_origin(origin).is_ok());
    }
    for origin in [
        "null",
        "*",
        "https://site.example/",
        "https://user@site.example",
        "http://site.example",
        "https://site.example/path",
        "https://site.example?x=1",
    ] {
        assert!(validate_origin(origin).is_err(), "{origin}");
    }
    let mut policy = WebsitePolicy::default();
    assert!(policy.valid());
    policy.history_retention_seconds = 86401;
    assert!(!policy.valid());
}

#[tokio::test]
async fn website_preflight_and_submit_time_additional_decision() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let (_dir, a, id, h) = website_setup();
    let c: core::Challenge = serde_json::from_value(
        web_prepare(&a, &h, &id, &core::random(), "account").unwrap()["challenge"].clone(),
    )
    .unwrap();
    let (public, _) = routers(a.clone());
    for (origin, expected) in [
        ("https://site.example", StatusCode::NO_CONTENT),
        ("https://evil.example", StatusCode::FORBIDDEN),
    ] {
        let response = public
            .clone()
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/public/v1/web/solutions")
                    .header("origin", origin)
                    .header("access-control-request-method", "POST")
                    .header("access-control-request-headers", "content-type")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        if expected == StatusCode::NO_CONTENT {
            assert_eq!(response.headers()["access-control-allow-origin"], origin);
        }
    }
    // Another completed login can raise account risk after this task was issued.
    let account: String =
        a.db.lock()
            .unwrap()
            .query_row(
                "SELECT account_hash FROM website_attempts WHERE challenge_id=?1",
                [&c.challenge_id],
                |r| r.get(0),
            )
            .unwrap();
    a.db.lock()
        .unwrap()
        .execute(
            "INSERT INTO login_outcomes(challenge_id,integration_id,account_hash,success,created,action) VALUES(?1,?2,?3,0,?4,'register')",
            rusqlite::params![core::random(), id, account, now()],
        )
        .unwrap();
    let (status, result) = web_submit(
        a.clone(),
        &c,
        Some("https://site.example"),
        "/public/v1/web/solutions",
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(result["decision"], "additional_verification_required");
    assert!(result["verification_token"].is_null());
    let count: i64 =
        a.db.lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM challenges WHERE token_hash IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
    assert_eq!(count, 0);
}

fn visual_task(
    a: &std::sync::Arc<App>,
    h: &HeaderMap,
    id: &str,
) -> (String, serde_json::Value, core::Challenge) {
    let ctx = core::random();
    let c: core::Challenge = serde_json::from_value(
        web_prepare(a, h, id, &ctx, "visual-account").unwrap()["challenge"].clone(),
    )
    .unwrap();
    let offered = gateveriq::visual::offer(a, &c.challenge_id, "https://site.example").unwrap();
    (ctx, offered, c)
}
fn visual_answer(
    a: &std::sync::Arc<App>,
    c: &core::Challenge,
    offered: &serde_json::Value,
) -> gateveriq::visual::VisualSolution {
    let mask: u32 =
        a.db.lock()
            .unwrap()
            .query_row(
                "SELECT answer FROM visual_tasks WHERE challenge_id=?1",
                [&c.challenge_id],
                |r| r.get(0),
            )
            .unwrap();
    gateveriq::visual::VisualSolution {
        challenge_id: c.challenge_id.clone(),
        task_id: offered["visual"]["task_id"].as_str().unwrap().into(),
        nonce: core::solve(c).unwrap(),
        selected: (0u8..16).filter(|i| mask & (1 << i) != 0).collect(),
    }
}
#[tokio::test]
async fn visual_completion_bound_to_attempt_and_normal_token_lifecycle() {
    let (_dir, a, id, h) = website_setup();
    let (ctx, offered, c) = visual_task(&a, &h, &id);
    assert_eq!(offered["visual"]["images"].as_array().unwrap().len(), 16);
    assert!(offered["visual"].get("answer").is_none());
    let unique: std::collections::HashSet<_> = offered["visual"]["images"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(unique.len(), 16);
    assert_eq!(
        offered,
        gateveriq::visual::offer(&a, &c.challenge_id, "https://site.example").unwrap()
    );
    assert!(offered["verification_token"].is_null());
    assert_eq!(
        web_submit(
            a.clone(),
            &c,
            Some("https://site.example"),
            "/public/v1/web/solutions"
        )
        .await
        .1["decision"],
        "additional_verification_required"
    );
    assert_eq!(
        gateveriq::visual::solve(&a, "https://evil.example", visual_answer(&a, &c, &offered))
            .unwrap_err()
            .1,
        "ORIGIN_NOT_ALLOWED"
    );
    let (_, other, c2) = visual_task(&a, &h, &id);
    let mut crossed = visual_answer(&a, &c, &offered);
    crossed.task_id = other["visual"]["task_id"].as_str().unwrap().into();
    assert_eq!(
        gateveriq::visual::solve(&a, "https://site.example", crossed)
            .unwrap_err()
            .1,
        "VISUAL_NOT_FOUND"
    );
    let mut bad_proof = visual_answer(&a, &c, &offered);
    bad_proof.nonce = "bad".into();
    assert_eq!(
        gateveriq::visual::solve(&a, "https://site.example", bad_proof)
            .unwrap_err()
            .1,
        "INVALID_SOLUTION"
    );
    let mut duplicate = visual_answer(&a, &c, &offered);
    duplicate.selected = vec![1, 1];
    assert_eq!(
        gateveriq::visual::solve(&a, "https://site.example", duplicate)
            .unwrap_err()
            .1,
        "INVALID_REQUEST"
    );
    // Persisted image tasks and their answers survive restart.
    let reopened = App::open(a.config.clone()).unwrap();
    let result = gateveriq::visual::solve(
        &reopened,
        "https://site.example",
        visual_answer(&a, &c, &offered),
    )
    .unwrap();
    assert_eq!(
        result,
        gateveriq::visual::solve(&a, "https://site.example", visual_answer(&a, &c, &offered))
            .unwrap()
    );
    let token = result["verification_token"].as_str().unwrap();
    assert_eq!(
        consume(
            &a,
            &h,
            request(&id, token, &c2.challenge_id, "visual-login")
        )
        .unwrap_err()
        .1,
        "CONTEXT_MISMATCH"
    );
    consume(&a, &h, request(&id, token, &ctx, "visual-login")).unwrap();
    assert_eq!(
        gateveriq::visual::solve(&a, "https://site.example", visual_answer(&a, &c, &offered))
            .unwrap_err()
            .1,
        "INVALID_TOKEN"
    );
}
#[test]
fn visual_attempts_expiration_and_no_reset() {
    let (_dir, a, id, h) = website_setup();
    let (_, mut offered, c) = visual_task(&a, &h, &id);
    for attempt in 0..3 {
        let mut wrong = visual_answer(&a, &c, &offered);
        wrong.selected = vec![];
        assert_eq!(
            gateveriq::visual::solve(&a, "https://site.example", wrong)
                .unwrap_err()
                .1,
            "INVALID_VISUAL_SOLUTION"
        );
        if attempt < 2 {
            let previous = offered;
            offered =
                gateveriq::visual::offer(&a, &c.challenge_id, "https://site.example").unwrap();
            assert_ne!(previous["visual"]["task_id"], offered["visual"]["task_id"]);
            assert_ne!(
                previous["visual"]["reference"],
                offered["visual"]["reference"]
            );
            assert_eq!(
                previous["visual"]["expires_at"],
                offered["visual"]["expires_at"]
            );
            assert_eq!(offered["visual"]["max_attempts"], 2 - attempt);
            let mut stale = visual_answer(&a, &c, &previous);
            stale.selected = vec![];
            assert_eq!(
                gateveriq::visual::solve(&a, "https://site.example", stale)
                    .unwrap_err()
                    .1,
                "VISUAL_NOT_FOUND"
            );
        }
    }
    assert_eq!(
        gateveriq::visual::solve(&a, "https://site.example", visual_answer(&a, &c, &offered))
            .unwrap_err()
            .1,
        "VISUAL_ATTEMPTS_EXHAUSTED"
    );
    assert_eq!(
        gateveriq::visual::offer(&a, &c.challenge_id, "https://site.example")
            .unwrap_err()
            .1,
        "VISUAL_ATTEMPTS_EXHAUSTED"
    );
    let (_, offered, c) = visual_task(&a, &h, &id);
    a.db.lock()
        .unwrap()
        .execute(
            "UPDATE visual_tasks SET expires=?1 WHERE challenge_id=?2",
            rusqlite::params![now() - 1, c.challenge_id],
        )
        .unwrap();
    assert_eq!(
        gateveriq::visual::solve(&a, "https://site.example", visual_answer(&a, &c, &offered))
            .unwrap_err()
            .1,
        "VISUAL_EXPIRED"
    );
    let count: i64 =
        a.db.lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM challenges WHERE token_hash IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
    assert_eq!(count, 0);
}
#[tokio::test]
async fn mandatory_visual_snapshot_and_receipt() {
    let (_dir, a, id, h) = website_setup();
    a.db.lock()
        .unwrap()
        .execute(
            "INSERT INTO action_visual_policies VALUES(?1,'register','always')",
            [&id],
        )
        .unwrap();
    let ctx = core::random();
    let offered = web_prepare(&a, &h, &id, &ctx, "mandatory").unwrap();
    assert!(offered["verification_token"].is_null());
    let c: core::Challenge = serde_json::from_value(offered["challenge"].clone()).unwrap();
    assert_eq!(
        offered,
        web_prepare(&a, &h, &id, &ctx, "mandatory").unwrap()
    );
    for path in ["/public/v1/solutions", "/public/v1/web/solutions"] {
        let (_, response) = web_submit(a.clone(), &c, Some("https://site.example"), path).await;
        assert!(response
            .get("verification_token")
            .is_none_or(|v| v.is_null()));
    }
    a.db.lock()
        .unwrap()
        .execute("UPDATE action_visual_policies SET visual='adaptive'", [])
        .unwrap();
    assert_eq!(
        offered,
        web_prepare(&a, &h, &id, &ctx, "mandatory").unwrap()
    );
    // Even a missing task cannot bypass the snapshotted requirement.
    a.db.lock()
        .unwrap()
        .execute(
            "DELETE FROM visual_tasks WHERE challenge_id=?1",
            [&c.challenge_id],
        )
        .unwrap();
    let (_, response) = web_submit(
        a.clone(),
        &c,
        Some("https://site.example"),
        "/public/v1/solutions",
    )
    .await;
    assert!(response.get("verification_token").is_none());
    let offered = gateveriq::visual::offer(&a, &c.challenge_id, "https://site.example").unwrap();
    let solved =
        gateveriq::visual::solve(&a, "https://site.example", visual_answer(&a, &c, &offered))
            .unwrap();
    let token = solved["verification_token"].as_str().unwrap();
    let first = consume(&a, &h, request(&id, token, &ctx, "mandatory-request")).unwrap();
    assert_eq!(first["verification_method"], "pow_visual");
    a.db.lock()
        .unwrap()
        .execute("UPDATE visual_tasks SET expires=0", [])
        .unwrap();
    a.db.lock()
        .unwrap()
        .execute("UPDATE challenges SET token_expires=0", [])
        .unwrap();
    let reopened = App::open(a.config.clone()).unwrap();
    assert_eq!(
        first,
        consume(
            &reopened,
            &h,
            request(&id, token, &ctx, "mandatory-request")
        )
        .unwrap()
    );
    assert!(consume(&a, &h, request(&id, token, &ctx, "different-request")).is_err());
}

#[test]
fn outcome_history_is_scoped_to_action() {
    let (_dir, a, id, h) = website_setup();
    let ctx = core::random();
    let initial = web_prepare(&a, &h, &id, &ctx, "scoped").unwrap();
    let challenge = initial["challenge"]["challenge_id"].as_str().unwrap();
    let db = a.db.lock().unwrap();
    let account: String = db
        .query_row(
            "SELECT account_hash FROM website_attempts WHERE challenge_id=?1",
            [challenge],
            |r| r.get(0),
        )
        .unwrap();
    db.execute(
        "INSERT INTO login_outcomes VALUES('other',?1,?2,0,?3,'email_verify')",
        rusqlite::params![id, account, now()],
    )
    .unwrap();
    drop(db);
    assert_eq!(
        web_prepare(&a, &h, &id, &core::random(), "scoped").unwrap()["decision"],
        "verify"
    );
    a.db.lock()
        .unwrap()
        .execute("UPDATE login_outcomes SET action='register'", [])
        .unwrap();
    assert_eq!(
        web_prepare(&a, &h, &id, &core::random(), "scoped").unwrap()["decision"],
        "additional_verification_required"
    );
}

#[test]
fn migration_preserves_legacy_attempts_and_unknown_outcomes() {
    let (_dir, a, id, h) = setup();
    let c = prepare(
        &a,
        &h,
        Prepare {
            integration_id: id.clone(),
            action: "register".into(),
            context_id: "legacy".into(),
        },
    )
    .unwrap();
    let db = a.db.lock().unwrap();
    db.execute_batch("DROP TABLE used_contexts; DROP TABLE action_visual_policies; DROP INDEX outcome_action_history;
        ALTER TABLE challenges DROP COLUMN visual_policy;
        ALTER TABLE challenges DROP COLUMN verification_method;
        ALTER TABLE login_outcomes DROP COLUMN action;
        DELETE FROM schema_migrations WHERE version=2;").unwrap();
    db.execute(
        "INSERT INTO login_outcomes VALUES('deleted-challenge',?1,'hash',0,?2)",
        rusqlite::params![id, now()],
    )
    .unwrap();
    drop(db);
    let upgraded = App::open(a.config.clone()).unwrap();
    let db = upgraded.db.lock().unwrap();
    let unknown: Option<String> = db
        .query_row("SELECT action FROM login_outcomes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(unknown, None);
    let policy: String = db
        .query_row(
            "SELECT visual_policy FROM challenges WHERE id=?1",
            [&c.challenge_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(policy, "adaptive");
    drop(db);
    App::open(a.config.clone()).unwrap();
}

#[test]
fn cleaned_context_cannot_be_recreated() {
    let (_dir, a, id, h) = setup();
    let p = || Prepare {
        integration_id: id.clone(),
        action: "register".into(),
        context_id: "used".into(),
    };
    prepare(&a, &h, p()).unwrap();
    a.db.lock()
        .unwrap()
        .execute("UPDATE challenges SET expires=0", [])
        .unwrap();
    a.cleanup().unwrap();
    assert_eq!(prepare(&a, &h, p()).unwrap_err().1, "CONTEXT_ALREADY_USED");
}
