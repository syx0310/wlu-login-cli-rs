use crate::error::{Error, Result};

pub const DEFAULT_PRESET: &str = "windows-chrome";

pub fn resolve(preset: Option<&str>, user_agent: Option<&str>) -> Result<Option<String>> {
    if let Some(value) = user_agent.filter(|value| !value.trim().is_empty()) {
        return Ok(Some(value.to_owned()));
    }

    match preset.unwrap_or(DEFAULT_PRESET) {
        "windows-chrome" => Ok(Some("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36".to_owned())),
        "windows-edge" => Ok(Some("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36 Edg/124.0.0.0".to_owned())),
        "mac-safari" => Ok(Some("Mozilla/5.0 (Macintosh; Intel Mac OS X 14_4) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Safari/605.1.15".to_owned())),
        "ios-safari" => Ok(Some("Mozilla/5.0 (iPhone; CPU iPhone OS 17_4 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Mobile/15E148 Safari/604.1".to_owned())),
        "android-chrome" => Ok(Some("Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Mobile Safari/537.36".to_owned())),
        "none" => Ok(None),
        other => Err(Error::msg(format!(
            "unknown ua preset `{other}`; use windows-chrome, windows-edge, mac-safari, ios-safari, android-chrome, or none"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_user_agent_wins() {
        assert_eq!(
            resolve(Some("none"), Some("custom")).unwrap(),
            Some("custom".to_owned())
        );
    }

    #[test]
    fn none_suppresses_user_agent() {
        assert_eq!(resolve(Some("none"), None).unwrap(), None);
    }
}
