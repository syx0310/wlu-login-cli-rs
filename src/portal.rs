use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::crypto;
use crate::error::{Error, Result};
use crate::http_client::HttpClient;
use crate::jsonp;

const DEFAULT_INTERNET_CHECK: &str = "http://www.baidu.com";

#[derive(Debug, Clone)]
pub struct PortalServer {
    endpoint: String,
    acid: String,
    callback: String,
    timestamp: String,
    username: Option<String>,
    password: Option<String>,
    user_info: Option<UserInfo>,
    challenge: Option<Challenge>,
    client: HttpClient,
    internet_check: String,
    verbose: bool,
}

impl PortalServer {
    pub fn new(endpoint: String, acid: String, client: HttpClient, verbose: bool) -> Self {
        let timestamp = unix_nanos().to_string();
        Self {
            endpoint,
            acid,
            callback: format!("jQuery112403771213770126085_{timestamp}"),
            timestamp,
            username: None,
            password: None,
            user_info: None,
            challenge: None,
            client,
            internet_check: DEFAULT_INTERNET_CHECK.to_owned(),
            verbose,
        }
    }

    pub fn set_username(&mut self, username: impl Into<String>) -> Result<()> {
        let username = username.into();
        if username.trim().is_empty() {
            return Err(Error::msg("username is empty"));
        }
        self.username = Some(username);
        Ok(())
    }

    pub fn set_password(&mut self, password: impl Into<String>) -> Result<()> {
        let password = password.into();
        if password.is_empty() {
            return Err(Error::msg("password is empty"));
        }
        self.password = Some(password);
        Ok(())
    }

    pub fn get_user_info(&mut self) -> Result<UserInfo> {
        let mut url = self.api_url("/cgi-bin/rad_user_info")?;
        url.query_pairs_mut()
            .append_pair("callback", &self.callback)
            .append_pair("_", &self.timestamp);
        let info: UserInfo = self.get_jsonp(url)?;
        self.user_info = Some(info.clone());
        Ok(info)
    }

    pub fn get_challenge(&mut self) -> Result<Challenge> {
        let username = self.username()?;
        let mut url = self.api_url("/cgi-bin/get_challenge")?;
        url.query_pairs_mut()
            .append_pair("callback", &self.callback)
            .append_pair("username", username)
            .append_pair("_", &self.timestamp);
        let challenge: Challenge = self.get_jsonp(url)?;
        self.challenge = Some(challenge.clone());
        Ok(challenge)
    }

    pub fn login(&mut self) -> Result<LoginResponse> {
        let url = self.login_url()?;
        let response: LoginResponse = self.get_jsonp(url)?;
        Ok(response)
    }

    pub fn logout(&mut self) -> Result<LogoutResponse> {
        let username = self.username()?;
        let ip = self.client_ip()?;
        let time = unix_seconds().to_string();
        let sign = crypto::sha1_hex(&format!("{time}{username}{ip}1{time}"));

        let mut url = self.api_url("/cgi-bin/rad_user_dm")?;
        url.query_pairs_mut()
            .append_pair("callback", &self.callback)
            .append_pair("ip", ip)
            .append_pair("username", username)
            .append_pair("time", &time)
            .append_pair("unbind", "1")
            .append_pair("sign", &sign)
            .append_pair("_", &self.timestamp);

        let response: LogoutResponse = self.get_jsonp(url)?;
        Ok(response)
    }

    pub fn internet(&self) -> bool {
        let Ok(check_url) = Url::parse(&self.internet_check) else {
            return false;
        };
        let Ok(response) = self.client.get(&self.internet_check) else {
            return false;
        };
        let Ok(final_url) = Url::parse(&response.final_url) else {
            return false;
        };

        final_url.host_str() == check_url.host_str()
            && !response.body.contains("Authentication is required. Click ")
    }

    pub fn daemon_login_loop(&mut self, interval: u64) -> Result<()> {
        eprintln!("start daemon: check every {interval} seconds");
        let mut start = SystemTime::now();

        loop {
            let info = self.get_user_info()?;
            if !info.is_ok() {
                eprintln!("check failed: start retry");
                self.get_challenge()?;
                self.login()?;
                start = SystemTime::now();
            }

            let live_time = SystemTime::now()
                .duration_since(start)
                .unwrap_or_default()
                .as_secs_f64();
            eprintln!("check succeed: live time {live_time}s");

            if !self.internet() {
                eprintln!("internet is not available, try to login again");
                if let Err(err) = self.logout() {
                    eprintln!("{err}");
                }
            } else {
                thread::sleep(Duration::from_secs(interval));
            }
        }
    }

    fn login_url(&self) -> Result<Url> {
        let username = self.username()?;
        let password = self.password()?;
        let token = self.token()?;
        let ip = self.client_ip()?;
        let password_md5 = crypto::hmac_md5_hex(password, token);
        let info = crypto::encrypted_user_info(username, password, ip, &self.acid, token);
        let chksum = crypto::checksum(username, &password_md5, &self.acid, ip, &info, token);

        let mut url = self.api_url("/cgi-bin/srun_portal")?;
        url.query_pairs_mut()
            .append_pair("callback", &self.callback)
            .append_pair("action", "login")
            .append_pair("username", username)
            .append_pair("password", &format!("{{MD5}}{password_md5}"))
            .append_pair("os", "Windows 10")
            .append_pair("name", "Windows")
            .append_pair("double_stack", "0")
            .append_pair("chksum", &chksum)
            .append_pair("info", &info)
            .append_pair("ac_id", &self.acid)
            .append_pair("ip", ip)
            .append_pair("n", "200")
            .append_pair("type", "1")
            .append_pair("_", &self.timestamp);
        Ok(url)
    }

    fn get_jsonp<T>(&self, url: Url) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        if self.verbose {
            eprintln!("{url}");
        }
        let response = self.client.get(url.as_str())?;
        jsonp::parse(&response.body)
    }

    fn api_url(&self, path: &str) -> Result<Url> {
        let endpoint = self.endpoint.trim_end_matches('/');
        Ok(Url::parse(&format!("{endpoint}{path}"))?)
    }

    fn username(&self) -> Result<&str> {
        self.username
            .as_deref()
            .ok_or_else(|| Error::msg("username is empty"))
    }

    fn password(&self) -> Result<&str> {
        self.password
            .as_deref()
            .ok_or_else(|| Error::msg("password is empty"))
    }

    fn token(&self) -> Result<&str> {
        self.challenge
            .as_ref()
            .map(|challenge| challenge.challenge.as_str())
            .filter(|token| !token.is_empty())
            .ok_or_else(|| Error::msg("get challenge error"))
    }

    fn client_ip(&self) -> Result<&str> {
        self.user_info
            .as_ref()
            .map(|info| info.online_ip.as_str())
            .filter(|ip| !ip.is_empty())
            .ok_or_else(|| Error::msg("client ip is empty; run info before login/logout"))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserInfo {
    #[serde(default, rename = "error")]
    pub error: String,
    #[serde(default, rename = "online_ip")]
    pub online_ip: String,
    #[serde(default, rename = "online_ip6")]
    pub online_ip6: String,
    #[serde(default, rename = "user_name")]
    pub user_name: String,
    #[serde(default, rename = "real_name")]
    pub real_name: String,
    #[serde(default, rename = "user_mac")]
    pub user_mac: String,
    #[serde(default, rename = "sum_bytes")]
    pub sum_bytes: Value,
    #[serde(default, rename = "sum_seconds")]
    pub sum_seconds: Value,
    #[serde(default, rename = "remain_bytes")]
    pub remain_bytes: Value,
    #[serde(default, rename = "remain_seconds")]
    pub remain_seconds: Value,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl UserInfo {
    pub fn is_ok(&self) -> bool {
        self.error == "ok"
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Challenge {
    #[serde(default, rename = "challenge")]
    pub challenge: String,
    #[serde(default, rename = "client_ip")]
    pub client_ip: String,
    #[serde(default, rename = "ecode")]
    pub error_code: Value,
    #[serde(default, rename = "error")]
    pub error: String,
    #[serde(default, rename = "error_msg")]
    pub error_msg: String,
    #[serde(default, rename = "expire")]
    pub expire: String,
    #[serde(default, rename = "online_ip")]
    pub online_ip: String,
    #[serde(default, rename = "res")]
    pub res: String,
    #[serde(default, rename = "srun_ver")]
    pub srun_version: String,
    #[serde(default, rename = "st")]
    pub timestamp: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LoginResponse {
    #[serde(default, rename = "client_ip")]
    pub client_ip: String,
    #[serde(default, rename = "ecode")]
    pub error_code: Value,
    #[serde(default, rename = "error")]
    pub error: String,
    #[serde(default, rename = "error_msg")]
    pub error_msg: String,
    #[serde(default, rename = "online_ip")]
    pub online_ip: String,
    #[serde(default, rename = "res")]
    pub res: String,
    #[serde(default, rename = "srun_ver")]
    pub srun_version: String,
    #[serde(default, rename = "st")]
    pub timestamp: Value,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LogoutResponse {
    #[serde(default, rename = "client_ip")]
    pub client_ip: String,
    #[serde(default, rename = "ecode")]
    pub error_code: Value,
    #[serde(default, rename = "error")]
    pub error: String,
    #[serde(default, rename = "error_msg")]
    pub error_msg: String,
    #[serde(default, rename = "online_ip")]
    pub online_ip: String,
    #[serde(default, rename = "res")]
    pub res: String,
    #[serde(default, rename = "srun_ver")]
    pub srun_version: String,
    #[serde(default, rename = "st")]
    pub timestamp: Value,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_info_ok_uses_error_field() {
        assert!(
            UserInfo {
                error: "ok".to_owned(),
                ..UserInfo::default()
            }
            .is_ok()
        );
        assert!(
            !UserInfo {
                error: "not_online".to_owned(),
                ..UserInfo::default()
            }
            .is_ok()
        );
    }

    #[test]
    fn login_url_contains_srun_auth_parameters() {
        let client = HttpClient::new(None, None).unwrap();
        let mut portal = PortalServer::new(
            "https://10.1.10.1".to_owned(),
            "1".to_owned(),
            client,
            false,
        );
        portal.set_username("user").unwrap();
        portal.set_password("pass").unwrap();
        portal.user_info = Some(UserInfo {
            online_ip: "10.2.3.4".to_owned(),
            ..UserInfo::default()
        });
        portal.challenge = Some(Challenge {
            challenge: "1234567890abcdef".to_owned(),
            ..Challenge::default()
        });

        let url = portal.login_url().unwrap();
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.path(), "/cgi-bin/srun_portal");

        let query = url
            .query_pairs()
            .into_owned()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(query.get("action").map(String::as_str), Some("login"));
        assert_eq!(query.get("username").map(String::as_str), Some("user"));
        assert_eq!(
            query.get("password").map(String::as_str),
            Some("{MD5}602fea0a5d01301885054023348f03f3")
        );
        assert_eq!(query.get("ac_id").map(String::as_str), Some("1"));
        assert_eq!(query.get("ip").map(String::as_str), Some("10.2.3.4"));
        assert_eq!(query.get("n").map(String::as_str), Some("200"));
        assert_eq!(query.get("type").map(String::as_str), Some("1"));
        assert!(
            query
                .get("info")
                .is_some_and(|value| value.starts_with("{SRBX1}"))
        );
        assert_eq!(query.get("chksum").map(String::len), Some(40));
    }
}
