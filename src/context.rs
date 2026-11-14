//! Stateless, expiring operation contexts, authenticated by the service key.
use crate::{
    core,
    service::{bad, Error},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use hmac::{Hmac, Mac};
use sha2::Sha256;

const DOMAIN: &[u8] = b"gateveriq:context:v1:";
const PAYLOAD_LEN: usize = 41;
const ENCODED_LEN: usize = PAYLOAD_LEN + 32;

fn mac(key: &[u8], integration: &str, action: &str, payload: &[u8]) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(DOMAIN);
    for part in [integration.as_bytes(), action.as_bytes(), payload] {
        mac.update(&(part.len() as u64).to_be_bytes());
        mac.update(part);
    }
    mac
}

pub(crate) fn issue(key: &[u8], integration: &str, action: &str, expires: i64) -> String {
    let mut payload = vec![1];
    payload.extend_from_slice(&expires.to_be_bytes());
    payload.extend_from_slice(&core::decode(&core::random(), 32).unwrap());
    let tag = mac(key, integration, action, &payload)
        .finalize()
        .into_bytes();
    payload.extend_from_slice(&tag);
    B64.encode(payload)
}

pub(crate) fn verify(
    key: &[u8],
    integration: &str,
    action: &str,
    value: &str,
    now: i64,
) -> Result<i64, Error> {
    let bytes = core::decode(value, ENCODED_LEN).ok_or(bad("INVALID_CONTEXT"))?;
    if bytes[0] != 1
        || mac(key, integration, action, &bytes[..PAYLOAD_LEN])
            .verify_slice(&bytes[PAYLOAD_LEN..])
            .is_err()
    {
        return Err(bad("INVALID_CONTEXT"));
    }
    let expires = i64::from_be_bytes(bytes[1..9].try_into().unwrap());
    if expires <= now {
        return Err(bad("CONTEXT_EXPIRED"));
    }
    Ok(expires)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binding_tampering_and_expiration() {
        let key = [42; 32];
        let context = issue(&key, "integration", "login", 100);
        assert_eq!(context.len(), 98);
        assert_eq!(
            verify(&key, "integration", "login", &context, 99).unwrap(),
            100
        );
        assert_eq!(
            verify(&key, "integration", "login", &context, 100)
                .unwrap_err()
                .1,
            "CONTEXT_EXPIRED"
        );
        for (key, integration, action) in [
            ([41; 32], "integration", "login"),
            (key, "other", "login"),
            (key, "integration", "register"),
        ] {
            assert_eq!(
                verify(&key, integration, action, &context, 99)
                    .unwrap_err()
                    .1,
                "INVALID_CONTEXT"
            );
        }
        let mut bytes = core::decode(&context, ENCODED_LEN).unwrap();
        bytes[8] ^= 1;
        assert!(verify(&key, "integration", "login", &B64.encode(bytes), 99).is_err());
        assert!(verify(&key, "integration", "login", &core::random(), 99).is_err());
    }

    #[test]
    fn expired_context_stays_rejected_after_cleanup_and_restart() {
        use crate::service::{self, App, Config, Prepare};
        use axum::http::HeaderMap;
        let directory = tempfile::tempdir().unwrap();
        let key = [42; 32];
        let key_file = directory.path().join("key");
        std::fs::write(&key_file, B64.encode(key)).unwrap();
        let app = App::open(Config {
            website: Default::default(),
            database: directory.path().join("db").to_string_lossy().into(),
            token_key_file: key_file.to_string_lossy().into(),
            public_listen: "127.0.0.1:0".parse().unwrap(),
            internal_listen: "127.0.0.1:0".parse().unwrap(),
            difficulty_bits: 4,
            challenge_ttl_seconds: 120,
            token_ttl_seconds: 60,
            retention_seconds: 300,
            requests_per_minute: 100,
            max_active_challenges: 10,
        })
        .unwrap();
        let (id, secret) = app
            .create_integration("context-test", &["login".into()])
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("authorization", format!("Bearer {secret}").parse().unwrap());
        // An authentic old context and the challenge it used to identify.
        let expired = issue(&key, &id, "login", service::now() - 1000);
        app.db.lock().unwrap().execute(
            "INSERT INTO challenges(id,integration_id,action,context,salt,difficulty,expires) VALUES(?1,?2,'login',?3,?4,4,?5)",
            rusqlite::params![core::random(), id, expired, core::random(), service::now()-1000],
        ).unwrap();
        assert_eq!(app.cleanup().unwrap(), 1);
        let reopened = App::open(app.config.clone()).unwrap();
        let request = Prepare {
            integration_id: id,
            action: "login".into(),
            context_id: expired,
        };
        assert_eq!(
            service::prepare(&reopened, &headers, request)
                .unwrap_err()
                .1,
            "CONTEXT_EXPIRED"
        );
        let db = reopened.db.lock().unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM challenges", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='used_contexts'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }
}
