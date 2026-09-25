use std::collections::BTreeSet;
use std::fmt;

use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SecretRef(String);

impl SecretRef {
    pub fn new(name: impl Into<String>) -> Result<Self, SecretError> {
        let name = name.into();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') {
            return Err(SecretError::InvalidName(name));
        }
        Ok(Self(name))
    }

    pub fn name(&self) -> &str { &self.0 }
}

pub struct SecretValue(String);

impl SecretValue {
    pub fn expose(&self) -> &str { &self.0 }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("SecretValue([REDACTED])") }
}

impl fmt::Display for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("[REDACTED]") }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SecretError {
    #[error("invalid secret name: {0}")]
    InvalidName(String),
    #[error("secret is not allowlisted: {0}")]
    NotAllowed(String),
    #[error("secret is unavailable: {0}")]
    Unavailable(String),
}

pub trait SecretVault: Send + Sync {
    fn resolve(&self, reference: &SecretRef) -> Result<SecretValue, SecretError>;
}

#[derive(Clone, Debug, Default)]
pub struct EnvSecretVault {
    allowlist: BTreeSet<String>,
}

impl EnvSecretVault {
    pub fn new<I, S>(allowlist: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self { allowlist: allowlist.into_iter().map(Into::into).collect() }
    }
}

impl SecretVault for EnvSecretVault {
    fn resolve(&self, reference: &SecretRef) -> Result<SecretValue, SecretError> {
        if !self.allowlist.contains(reference.name()) {
            return Err(SecretError::NotAllowed(reference.name().into()));
        }
        std::env::var(reference.name())
            .map(SecretValue)
            .map_err(|_| SecretError::Unavailable(reference.name().into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_debug_and_display_are_redacted() {
        let value = SecretValue("super-secret".into());
        assert_eq!(format!("{value:?}"), "SecretValue([REDACTED])");
        assert_eq!(format!("{value}"), "[REDACTED]");
    }

    #[test]
    fn rejects_non_allowlisted_secret() {
        let vault = EnvSecretVault::new(["ALLOWED_KEY"]);
        let key = SecretRef::new("OTHER_KEY").unwrap();
        assert_eq!(vault.resolve(&key), Err(SecretError::NotAllowed("OTHER_KEY".into())));
    }

    #[test]
    fn validates_secret_reference_names() {
        assert!(SecretRef::new("OPENAI_API_KEY").is_ok());
        assert!(SecretRef::new("openai-key").is_err());
    }
}
