use nostr_sdk::{FromBech32, SecretKey};

pub const CHIIHOU_NSEC_ENV: &str = "CHIIHOU_NSEC";

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ChiihouSecretError {
    #[error("required environment variable is missing: CHIIHOU_NSEC")]
    Missing,

    #[error("CHIIHOU_NSEC is not valid Unicode")]
    NonUnicode,

    #[error("CHIIHOU_NSEC must contain an NIP-19 nsec private key")]
    InvalidNsec,
}

pub fn load_chiihou_nsec() -> Result<String, ChiihouSecretError> {
    load_chiihou_nsec_with(|name| std::env::var(name))
}

fn load_chiihou_nsec_with<F>(get: F) -> Result<String, ChiihouSecretError>
where
    F: FnOnce(&str) -> Result<String, std::env::VarError>,
{
    match get(CHIIHOU_NSEC_ENV) {
        Ok(value) => validate_chiihou_nsec(&value),
        Err(std::env::VarError::NotPresent) => Err(ChiihouSecretError::Missing),
        Err(std::env::VarError::NotUnicode(_)) => Err(ChiihouSecretError::NonUnicode),
    }
}

pub fn validate_chiihou_nsec(value: &str) -> Result<String, ChiihouSecretError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ChiihouSecretError::InvalidNsec);
    }
    SecretKey::from_bech32(trimmed).map_err(|_| ChiihouSecretError::InvalidNsec)?;
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr_sdk::{Keys, ToBech32};

    // テスト専用の秘密鍵。実際の運用で使用してはならない。
    const TEST_AI_SECRET_KEY_HEX: &str =
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn test_nsec() -> String {
        Keys::parse(TEST_AI_SECRET_KEY_HEX)
            .unwrap()
            .secret_key()
            .to_bech32()
            .unwrap()
    }

    fn test_npub() -> String {
        Keys::parse(TEST_AI_SECRET_KEY_HEX)
            .unwrap()
            .public_key()
            .to_bech32()
            .unwrap()
    }

    #[test]
    fn accepts_valid_nsec() {
        let nsec = test_nsec();
        assert_eq!(validate_chiihou_nsec(&nsec).unwrap(), nsec);
    }

    #[test]
    fn trims_whitespace() {
        let nsec = test_nsec();
        let padded = format!("  {nsec}\n");
        assert_eq!(validate_chiihou_nsec(&padded).unwrap(), nsec);
    }

    #[test]
    fn rejects_hex_secret_key() {
        assert_eq!(
            validate_chiihou_nsec(TEST_AI_SECRET_KEY_HEX),
            Err(ChiihouSecretError::InvalidNsec)
        );
    }

    #[test]
    fn rejects_npub() {
        assert_eq!(
            validate_chiihou_nsec(&test_npub()),
            Err(ChiihouSecretError::InvalidNsec)
        );
    }

    #[test]
    fn rejects_nostr_uri_nsec() {
        let uri = format!("nostr:{}", test_nsec());
        assert_eq!(
            validate_chiihou_nsec(&uri),
            Err(ChiihouSecretError::InvalidNsec)
        );
    }

    #[test]
    fn rejects_malformed_nsec() {
        assert_eq!(
            validate_chiihou_nsec("nsec1invalid"),
            Err(ChiihouSecretError::InvalidNsec)
        );
    }

    #[test]
    fn rejects_empty_string() {
        assert_eq!(
            validate_chiihou_nsec(""),
            Err(ChiihouSecretError::InvalidNsec)
        );
        assert_eq!(
            validate_chiihou_nsec("   "),
            Err(ChiihouSecretError::InvalidNsec)
        );
    }

    #[test]
    fn error_does_not_contain_input_value() {
        let input = "this-secret-must-not-appear";
        let error = validate_chiihou_nsec(input).unwrap_err();
        assert!(!error.to_string().contains(input));
    }

    #[test]
    fn load_with_returns_validated_nsec() {
        let nsec = test_nsec();
        let padded = format!(" {nsec} ");
        let result = load_chiihou_nsec_with(|name| {
            assert_eq!(name, CHIIHOU_NSEC_ENV);
            Ok(padded.clone())
        });
        assert_eq!(result.unwrap(), nsec);
    }

    #[test]
    fn load_with_maps_not_present_to_missing() {
        let result = load_chiihou_nsec_with(|_| Err(std::env::VarError::NotPresent));
        assert_eq!(result, Err(ChiihouSecretError::Missing));
    }

    #[test]
    fn load_with_maps_not_unicode_to_non_unicode() {
        let result = load_chiihou_nsec_with(|_| {
            Err(std::env::VarError::NotUnicode(std::ffi::OsString::from(
                "broken",
            )))
        });
        assert_eq!(result, Err(ChiihouSecretError::NonUnicode));
    }

    #[test]
    fn load_with_rejects_invalid_value() {
        let result = load_chiihou_nsec_with(|_| Ok("not-an-nsec".to_string()));
        assert_eq!(result, Err(ChiihouSecretError::InvalidNsec));
    }
}
