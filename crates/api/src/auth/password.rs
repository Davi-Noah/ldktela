//! Argon2id password hashing with the RNF-06 parameters.
//!
//! Memory 64 MiB, 3 iterations, parallelism 4. These are not tunable downwards
//! without discussion: the cost is the defence, and on the target ARM VM one
//! verification runs around 100 ms, which is why the login endpoint also carries
//! a rate limit.

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::RngCore;

use crate::config::Argon2Config;

/// A hash of a password that no user has, used to spend the same time on a
/// login attempt for an unknown e-mail as for a known one. Without it, response
/// time tells an attacker which addresses exist.
const DUMMY_HASH: &str = "$argon2id$v=19$m=65536,t=3,p=4$\
c29tZXNhbHRzb21lc2FsdA$Yl3f7Q0f3lC2W2m1lWJk0k1V6l7aQ9m3s2Xh7Zt0Xyk";

#[derive(Debug, thiserror::Error)]
pub enum PasswordError {
    #[error("hashing failed: {0}")]
    Hash(String),
    #[error("stored hash is malformed")]
    MalformedHash,
}

fn argon2(config: &Argon2Config) -> Result<Argon2<'static>, PasswordError> {
    let params = Params::new(
        config.memory_kib,
        config.iterations,
        config.parallelism,
        None,
    )
    .map_err(|e| PasswordError::Hash(e.to_string()))?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

/// Produces a PHC string ready for `users.password_hash`.
pub fn hash(config: &Argon2Config, password: &str) -> Result<String, PasswordError> {
    let mut salt_bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut salt_bytes);
    let salt =
        SaltString::encode_b64(&salt_bytes).map_err(|e| PasswordError::Hash(e.to_string()))?;
    let hash = argon2(config)?
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| PasswordError::Hash(e.to_string()))?;
    Ok(hash.to_string())
}

/// Constant-ish time verification. A malformed stored hash is a `false`, not an
/// error the caller has to branch on: either way the login fails.
pub fn verify(config: &Argon2Config, password: &str, stored: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(stored) else {
        return false;
    };
    let Ok(hasher) = argon2(config) else {
        return false;
    };
    hasher.verify_password(password.as_bytes(), &parsed).is_ok()
}

/// Burns the same work as a real verification, for e-mails that do not exist.
pub fn verify_dummy(config: &Argon2Config, password: &str) {
    let _ = verify(config, password, DUMMY_HASH);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cheap parameters: these tests exercise the wiring, not the cost.
    fn fast() -> Argon2Config {
        Argon2Config {
            memory_kib: 1024,
            iterations: 1,
            parallelism: 1,
        }
    }

    #[test]
    fn a_password_verifies_against_its_own_hash_and_nothing_else() {
        let config = fast();
        let hash = hash(&config, "senha-correta").unwrap();
        assert!(verify(&config, "senha-correta", &hash));
        assert!(!verify(&config, "senha-errada", &hash));
        assert!(!verify(&config, "", &hash));
    }

    #[test]
    fn two_hashes_of_the_same_password_differ() {
        let config = fast();
        let a = hash(&config, "mesma").unwrap();
        let b = hash(&config, "mesma").unwrap();
        assert_ne!(a, b, "o salt precisa ser aleatório por hash");
        assert!(verify(&config, "mesma", &a));
        assert!(verify(&config, "mesma", &b));
    }

    #[test]
    fn the_hash_records_the_configured_parameters() {
        let config = Argon2Config {
            memory_kib: 65536,
            iterations: 3,
            parallelism: 4,
        };
        let hash = hash(&config, "x").unwrap();
        assert!(
            hash.starts_with("$argon2id$v=19$m=65536,t=3,p=4$"),
            "{hash}"
        );
    }

    #[test]
    fn a_malformed_stored_hash_fails_instead_of_panicking() {
        assert!(!verify(&fast(), "qualquer", "não-é-um-phc-string"));
        assert!(!verify(&fast(), "qualquer", ""));
    }

    /// Measurement harness for RNF-06, not an assertion: the number is machine
    /// dependent. Run with:
    /// `cargo test -p api --lib argon2id_cost -- --ignored --nocapture`
    #[test]
    #[ignore = "measurement, not a correctness check"]
    fn argon2id_cost_at_the_rnf06_parameters() {
        let config = Argon2Config {
            memory_kib: 65536,
            iterations: 3,
            parallelism: 4,
        };
        let stored = hash(&config, "senha-de-referencia").unwrap();
        let started = std::time::Instant::now();
        const ROUNDS: u32 = 10;
        for _ in 0..ROUNDS {
            assert!(verify(&config, "senha-de-referencia", &stored));
        }
        let per_verify = started.elapsed() / ROUNDS;
        println!("argon2id verify (m=65536,t=3,p=4): {per_verify:?} por verificação");
    }

    #[test]
    fn the_dummy_hash_is_parseable_so_the_timing_defence_actually_runs() {
        // Se o hash de fachada não parsear, `verify` retorna cedo e o tempo de
        // resposta volta a distinguir e-mail existente de inexistente.
        assert!(
            PasswordHash::new(DUMMY_HASH).is_ok(),
            "DUMMY_HASH precisa ser um PHC string válido"
        );
        verify_dummy(&fast(), "qualquer");
    }
}
