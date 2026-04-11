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
}
