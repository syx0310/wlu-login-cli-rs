use serde::de::DeserializeOwned;

use crate::error::{Error, Result};

pub fn parse<T: DeserializeOwned>(body: &str) -> Result<T> {
    let json = extract_json(body)?;
    Ok(serde_json::from_str(json)?)
}

fn extract_json(body: &str) -> Result<&str> {
    let start = body
        .find('(')
        .ok_or_else(|| Error::msg("invalid JSONP response: missing `(`"))?;
    let end = body
        .rfind(')')
        .ok_or_else(|| Error::msg("invalid JSONP response: missing `)`"))?;

    if end <= start {
        return Err(Error::msg("invalid JSONP response: empty callback body"));
    }

    Ok(&body[start + 1..end])
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    #[test]
    fn parses_jsonp() {
        let value: Value = parse(r#"jQuery({"error":"ok","num":1})"#).unwrap();
        assert_eq!(value["error"], "ok");
        assert_eq!(value["num"], 1);
    }

    #[test]
    fn rejects_plain_json() {
        assert!(parse::<Value>(r#"{"error":"ok"}"#).is_err());
    }
}
