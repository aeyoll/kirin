use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use sha2::{Digest, Sha256};

pub fn sha256_hex(input: &[u8]) -> String {
    let d = Sha256::digest(input);
    hex::encode(d)
}

pub fn hash_download_password(plain: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut rand::thread_rng());
    let argon = Argon2::default();
    Ok(argon.hash_password(plain.as_bytes(), &salt)?.to_string())
}

pub fn verify_download_password(hash: &str, plain: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(plain.as_bytes(), &parsed)
        .is_ok()
}

pub fn verify_admin_password_hex(expected_hex: &str, plain: &str) -> bool {
    let Ok(expected) = hex::decode(expected_hex) else {
        return false;
    };
    if expected.len() != 32 {
        return false;
    }
    let got = Sha256::digest(plain.as_bytes());
    subtle::ConstantTimeEq::ct_eq(got.as_slice(), expected.as_slice()).into()
}

/// Verify admin credential: Argon2 PHC string (preferred) or legacy 64-char SHA-256 hex of the password.
pub fn verify_admin_password(stored: &str, plain: &str) -> bool {
    if stored.is_empty() {
        return false;
    }
    if stored.starts_with("$argon2") {
        let Ok(parsed) = PasswordHash::new(stored) else {
            return false;
        };
        return Argon2::default()
            .verify_password(plain.as_bytes(), &parsed)
            .is_ok();
    }
    verify_admin_password_hex(stored, plain)
}

pub fn hash_admin_password(plain: &str) -> anyhow::Result<String> {
    hash_download_password(plain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_download_password() {
        let h = hash_download_password("secret").unwrap();
        assert!(verify_download_password(&h, "secret"));
        assert!(!verify_download_password(&h, "wrong"));
    }

    #[test]
    fn admin_sha256() {
        let hex = sha256_hex(b"admin");
        assert!(verify_admin_password_hex(&hex, "admin"));
        assert!(!verify_admin_password_hex(&hex, "nope"));
    }

    #[test]
    fn admin_argon2_roundtrip() {
        let h = hash_admin_password("admin-secret").unwrap();
        assert!(h.starts_with("$argon2"));
        assert!(verify_admin_password(&h, "admin-secret"));
        assert!(!verify_admin_password(&h, "wrong"));
        assert!(verify_admin_password(&sha256_hex(b"x"), "x"));
    }
}
