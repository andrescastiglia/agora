use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use thiserror::Error;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SignatureError {
    #[error("signature header is missing")]
    Missing,
    #[error("signature header has an invalid format")]
    InvalidFormat,
    #[error("signature does not match request body")]
    Mismatch,
}

pub fn verify_meta_signature(
    header: Option<&str>,
    body: &[u8],
    app_secret: &str,
) -> Result<(), SignatureError> {
    let header = header.ok_or(SignatureError::Missing)?;
    let encoded = header
        .strip_prefix("sha256=")
        .ok_or(SignatureError::InvalidFormat)?;
    let provided = hex::decode(encoded).map_err(|_| SignatureError::InvalidFormat)?;
    if provided.len() != 32 {
        return Err(SignatureError::InvalidFormat);
    }

    let mut mac =
        HmacSha256::new_from_slice(app_secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(body);
    let expected = mac.finalize().into_bytes();

    if expected.as_slice().ct_eq(provided.as_slice()).into() {
        Ok(())
    } else {
        Err(SignatureError::Mismatch)
    }
}

pub fn sha256_hex(body: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(Sha256::digest(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_signature() {
        let body = br#"{"event":"ok"}"#;
        let secret = "top-secret";
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let signature = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));

        assert_eq!(
            verify_meta_signature(Some(&signature), body, secret),
            Ok(())
        );
    }

    #[test]
    fn rejects_missing_malformed_and_mismatched_signatures() {
        assert_eq!(
            verify_meta_signature(None, b"body", "secret"),
            Err(SignatureError::Missing)
        );
        assert_eq!(
            verify_meta_signature(Some("md5=abc"), b"body", "secret"),
            Err(SignatureError::InvalidFormat)
        );
        assert_eq!(
            verify_meta_signature(Some("sha256=abc"), b"body", "secret"),
            Err(SignatureError::InvalidFormat)
        );
        assert_eq!(
            verify_meta_signature(
                Some("sha256=0000000000000000000000000000000000000000000000000000000000000000"),
                b"body",
                "secret"
            ),
            Err(SignatureError::Mismatch)
        );
    }

    #[test]
    fn hashes_are_stable() {
        assert_eq!(
            sha256_hex(b"agora"),
            "f7070d57bbe5496e29249421e91572f46ac4c2b62953b7ea046fa3707b9e6b2a"
        );
    }
}
