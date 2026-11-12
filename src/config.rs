//! API key and model configuration for Albert API.
//!
//! Keys can be provided through the `ALBERT_API_KEY` environment variable
//! (highest priority) or the local `config.toml` file in the platform config directory.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Environment variable that holds the Albert API key.
pub const ALBERT_API_KEY_ENV: &str = "ALBERT_API_KEY";

/// Legacy alias for Albert env var.
pub const API_KEY_ENV: &str = ALBERT_API_KEY_ENV;

/// Recommended Albert models.
pub const ALBERT_MODELS: &[&str] = &[
    "deepseek-v4-flash",
    "qwen-2.5-coder",
    "deepseek-v3",
    "openweight-medium",
    "albert-light",
];

/// The name of the config file inside the platform config directory.
pub const CONFIG_FILE_NAME: &str = "config.toml";

/// Where the API key came from, used for user-facing messages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeySource {
    Environment,
    ConfigFile,
    Missing,
}

/// The resolved Albert API key and model configuration.
#[derive(Clone, Debug)]
pub struct ApiKeyConfig {
    /// Albert API key.
    pub key: String,
    /// Where the key came from.
    #[allow(dead_code)]
    pub source: KeySource,
    /// The selected Albert model, if any.
    pub model: Option<String>,
    /// The Albert endpoint, if any.
    pub endpoint: Option<String>,
}

/// The on-disk representation of the config file.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FileConfig {
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub endpoint: Option<String>,
}

impl ApiKeyConfig {
    /// Load the configuration from environment variables and/or config file.
    pub fn load() -> ApiKeyConfig {
        let file_cfg = Self::load_file_config();

        let mut key = String::new();
        let mut source = KeySource::Missing;
        if let Ok(k) = std::env::var(ALBERT_API_KEY_ENV) {
            let k = k.trim().to_string();
            if !k.is_empty() {
                key = k;
                source = KeySource::Environment;
            }
        }
        if key.is_empty()
            && let Some(ref cfg) = file_cfg
        {
            let k = cfg.api_key.clone().unwrap_or_default().trim().to_string();
            if !k.is_empty() {
                key = k;
                source = KeySource::ConfigFile;
            }
        }

        // Ignore any leftover non-Albert model or endpoint in config.toml
        let model = file_cfg
            .as_ref()
            .and_then(|c| c.model.clone())
            .filter(|m| {
                let lower = m.trim().to_lowercase();
                !lower.is_empty() && !lower.starts_with("gemini") && !lower.starts_with("gpt")
            });

        let endpoint = file_cfg
            .as_ref()
            .and_then(|c| c.endpoint.clone())
            .filter(|ep| {
                let lower = ep.trim().to_lowercase();
                !lower.is_empty()
                    && !lower.contains("googleapis.com")
                    && !lower.contains("openai.com")
            });

        ApiKeyConfig {
            key,
            source,
            model,
            endpoint,
        }
    }

    /// Read the config file, if present.
    fn load_file_config() -> Option<FileConfig> {
        let path = Self::config_file_path();
        let content = std::fs::read_to_string(&path).ok()?;
        toml::from_str(&content).ok()
    }

    /// The absolute path to the config file.
    pub fn config_file_path() -> PathBuf {
        crate::platform::config_dir().join(CONFIG_FILE_NAME)
    }

    /// Persist the Albert API key, model, and endpoint to the config file.
    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_file_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("Failed to create config directory: {}", e))?;
        }
        let config = FileConfig {
            api_key: if self.key.is_empty() {
                None
            } else {
                Some(self.key.clone())
            },
            model: self.model.clone(),
            endpoint: self.endpoint.clone(),
        };
        let content = toml::to_string_pretty(&config)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        std::fs::write(&path, content).map_err(|e| format!("Failed to write config file: {}", e))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = std::fs::Permissions::from_mode(0o600);
            let _ = std::fs::set_permissions(&path, permissions);
        }

        Ok(())
    }

    /// Validate the Albert API key configuration.
    pub fn validate(&self) -> Result<(), String> {
        if self.key.trim().is_empty() {
            return Err(format!(
                "Clé API Albert manquante. Veuillez saisir votre clé dans les Réglages (⚙) ou définir {}.",
                ALBERT_API_KEY_ENV
            ));
        }
        if self.key.chars().any(char::is_whitespace) {
            return Err("La clé API contient des espaces invalides.".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_empty() {
        let cfg = ApiKeyConfig {
            key: String::new(),
            source: KeySource::Missing,
            model: None,
            endpoint: None,
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_accepts_valid_key() {
        let cfg = ApiKeyConfig {
            key: "sk-1234567890abcdef".to_string(),
            source: KeySource::Environment,
            model: None,
            endpoint: None,
        };
        assert!(cfg.validate().is_ok());
    }
}
