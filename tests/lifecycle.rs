use axum::http::{HeaderMap, HeaderValue, StatusCode};
use gateveriq::{core, service::*};
fn setup() -> (tempfile::TempDir, std::sync::Arc<App>, String, HeaderMap) {
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("key");
    std::fs::write(&key, core::random()).unwrap();
    let c = Config {
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
