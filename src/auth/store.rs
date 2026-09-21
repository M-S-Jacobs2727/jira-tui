use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::Result;

const SERVICE: &str = "jira-tui";
const ACCOUNT: &str = "oauth-tokens";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StoredTokens {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub client_secret: String,
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub expires_at: u64,
}

impl StoredTokens {
    pub fn is_access_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.access_token.is_empty() || now + 60 >= self.expires_at
    }

    pub fn has_refresh(&self) -> bool {
        !self.refresh_token.is_empty()
    }

    pub fn apply_token_response(
        &mut self,
        access_token: String,
        refresh_token: Option<String>,
        expires_in: u64,
    ) {
        self.access_token = access_token;
        if let Some(refresh) = refresh_token {
            if !refresh.is_empty() {
                self.refresh_token = refresh;
            }
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.expires_at = now.saturating_add(expires_in);
    }
}

#[derive(Clone)]
pub struct TokenStore;

impl TokenStore {
    pub fn load() -> Result<StoredTokens> {
        if let Ok(entry) = keyring::Entry::new(SERVICE, ACCOUNT) {
            if let Ok(payload) = entry.get_password() {
                if let Ok(tokens) = serde_json::from_str::<StoredTokens>(&payload) {
                    return Ok(tokens);
                }
            }
        }
        Self::load_file()
    }

    pub fn save(tokens: &StoredTokens) -> Result<()> {
        let payload = serde_json::to_string(tokens)?;
        let mut keyring_ok = false;
        if let Ok(entry) = keyring::Entry::new(SERVICE, ACCOUNT) {
            if entry.set_password(&payload).is_ok() {
                keyring_ok = true;
            }
        }
        // Always keep a file fallback so rotating refresh tokens survive keyring gaps.
        Self::save_file(tokens)?;
        if !keyring_ok {
            tracing::debug!("keyring unavailable; tokens stored in credentials file");
        }
        Ok(())
    }

    pub fn clear() -> Result<()> {
        if let Ok(entry) = keyring::Entry::new(SERVICE, ACCOUNT) {
            let _ = entry.delete_credential();
        }
        let path = Config::credentials_path()?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    fn load_file() -> Result<StoredTokens> {
        let path = Config::credentials_path()?;
        if !path.exists() {
            return Ok(StoredTokens::default());
        }
        let raw = fs::read_to_string(path)?;
        Ok(toml::from_str(&raw)?)
    }

    fn save_file(tokens: &StoredTokens) -> Result<()> {
        let path = Config::credentials_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, toml::to_string_pretty(tokens)?)?;
        let mut perms = fs::metadata(&path)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&path, perms)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::StoredTokens;

    #[test]
    fn has_refresh_does_not_require_client_secret() {
        let tokens = StoredTokens {
            refresh_token: "refresh".into(),
            ..StoredTokens::default()
        };
        assert!(tokens.has_refresh());
        assert!(tokens.client_secret.is_empty());
    }

    #[test]
    fn omits_empty_client_secret() {
        let tokens = StoredTokens {
            access_token: "a".into(),
            refresh_token: "r".into(),
            ..StoredTokens::default()
        };
        let encoded = toml::to_string(&tokens).unwrap();
        assert!(!encoded.contains("client_secret"));
    }
}
