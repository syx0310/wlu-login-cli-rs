use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub net: NetConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetConfig {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_opt_string"
    )]
    pub endpoint: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_opt_string"
    )]
    pub acid: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_opt_string"
    )]
    pub ua_preset: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_opt_string"
    )]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub auth: AuthConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthConfig {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_opt_string"
    )]
    pub username: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_opt_string"
    )]
    pub password: Option<String>,
    #[serde(
        default,
        rename = "interface",
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_opt_string"
    )]
    pub iface: Option<String>,
}

pub fn default_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| Error::msg("cannot find home directory for default config path"))?;
    Ok(PathBuf::from(home).join(".wlu-login-cli.yaml"))
}

pub fn load(path: Option<PathBuf>) -> Result<(AppConfig, PathBuf, bool)> {
    let path = match path {
        Some(path) => path,
        None => default_path()?,
    };

    if !path.exists() {
        return Ok((AppConfig::default(), path, false));
    }

    let content = fs::read_to_string(&path)?;
    if content.trim().is_empty() {
        return Ok((AppConfig::default(), path, true));
    }

    let config = serde_yaml::from_str(&content)?;
    Ok((config, path, true))
}

pub fn save(path: &PathBuf, config: &AppConfig) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    let content = serde_yaml::to_string(config)?;
    fs::write(path, content)?;
    Ok(())
}

fn deserialize_opt_string<'de, D>(deserializer: D) -> std::result::Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_yaml::Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| match value {
        serde_yaml::Value::Null => None,
        serde_yaml::Value::String(value) => Some(value),
        serde_yaml::Value::Number(value) => Some(value.to_string()),
        serde_yaml::Value::Bool(value) => Some(value.to_string()),
        other => Some(serde_yaml::to_string(&other).unwrap_or_default()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_numeric_username_and_acid() {
        let cfg: AppConfig = serde_yaml::from_str(
            r#"
net:
  acid: 1
  auth:
    username: 20051101
    password: your-password
"#,
        )
        .unwrap();
        assert_eq!(cfg.net.acid.as_deref(), Some("1"));
        assert_eq!(cfg.net.auth.username.as_deref(), Some("20051101"));
        assert_eq!(cfg.net.auth.password.as_deref(), Some("your-password"));
    }
}
