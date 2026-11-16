use std::path::PathBuf;

use lexopt::prelude::*;
use url::Url;

use crate::config::{self, AppConfig};
use crate::error::{Error, Result};
use crate::http_client::HttpClient;
use crate::portal::PortalServer;
use crate::{table, ua};

const DEFAULT_ENDPOINT: &str = "http://10.1.10.1";
const DEFAULT_ACID: &str = "1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NetCommand {
    Info,
    Login,
    Logout,
    Internet,
}

#[derive(Debug, Default)]
struct Args {
    help_topics: Option<Vec<String>>,
    net_command: Option<NetCommand>,
    config_file: Option<PathBuf>,
    save: bool,
    verbose: bool,
    endpoint: Option<String>,
    acid: Option<String>,
    username: Option<String>,
    password: Option<String>,
    daemon: bool,
    interval: u64,
    iface: Option<String>,
    ua_preset: Option<String>,
    user_agent: Option<String>,
}

#[derive(Debug)]
struct Effective {
    endpoint: String,
    acid: String,
    username: Option<String>,
    password: Option<String>,
    iface: Option<String>,
    ua_preset: Option<String>,
    user_agent: Option<String>,
    resolved_user_agent: Option<String>,
}

pub fn run() -> Result<()> {
    let args = parse_args()?;
    if let Some(topics) = &args.help_topics {
        print_topic_help(topics)?;
        return Ok(());
    }

    let (mut cfg, cfg_path, existed) = config::load(args.config_file.clone())?;
    if args.verbose && existed {
        eprintln!("Using config file: {}", cfg_path.display());
    }

    let effective = merge(&args, &cfg)?;
    Url::parse(&effective.endpoint)?;

    let command = args.net_command.ok_or_else(|| {
        Error::msg("missing command; use `wlu-login-cli net <info|login|logout|internet>`")
    })?;

    let client = HttpClient::new(
        effective.resolved_user_agent.clone(),
        effective.iface.clone(),
    )?;
    let mut portal = PortalServer::new(
        effective.endpoint.clone(),
        effective.acid.clone(),
        client,
        args.verbose,
    );

    match command {
        NetCommand::Info => {
            let info = portal.get_user_info()?;
            table::print(&info);
        }
        NetCommand::Login => {
            let username = effective
                .username
                .as_deref()
                .ok_or_else(|| Error::msg("username is empty"))?;
            let password = effective
                .password
                .as_deref()
                .ok_or_else(|| Error::msg("password is empty"))?;
            portal.set_username(username)?;
            portal.set_password(password)?;

            let info = portal.get_user_info()?;
            if args.verbose {
                table::print(&info);
            }

            let challenge = portal.get_challenge()?;
            if args.verbose {
                table::print(&challenge);
            }

            let response = portal.login()?;
            table::print(&response);

            if args.daemon {
                portal.daemon_login_loop(args.interval.max(1))?;
            }
        }
        NetCommand::Logout => {
            let info = portal.get_user_info()?;
            if !info.is_ok() {
                println!("you are not login");
                return Ok(());
            }
            if args.verbose {
                table::print(&info);
            }
            println!("you are logout account {}", info.user_name);
            portal.set_username(info.user_name)?;
            let response = portal.logout()?;
            table::print(&response);
        }
        NetCommand::Internet => {
            println!("{}", portal.internet());
        }
    }

    if args.save {
        apply_effective_to_config(&mut cfg, &effective);
        config::save(&cfg_path, &cfg)?;
    }

    Ok(())
}

fn parse_args() -> Result<Args> {
    let mut args = Args {
        interval: 60,
        ..Args::default()
    };
    let mut seen_net = false;
    let mut parser = lexopt::Parser::from_env();

    while let Some(arg) = parser.next()? {
        match arg {
            Short('h') | Long("help") => {
                print_help();
                std::process::exit(0);
            }
            Short('v') | Long("version") => {
                println!("wlu-login-cli {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            Long("config") => args.config_file = Some(parser.value()?.into()),
            Short('s') | Long("save") => args.save = true,
            Short('V') | Long("verbose") => args.verbose = true,
            Short('e') | Long("endpoint") => args.endpoint = Some(parser.value()?.string()?),
            Short('a') | Long("acid") => args.acid = Some(parser.value()?.string()?),
            Short('u') | Long("username") => args.username = Some(parser.value()?.string()?),
            Short('p') | Long("password") => args.password = Some(parser.value()?.string()?),
            Short('d') | Long("daemon") => args.daemon = true,
            Short('i') | Long("interval") => args.interval = parser.value()?.parse()?,
            Short('f') | Long("interface") => args.iface = Some(parser.value()?.string()?),
            Long("ua-preset") => args.ua_preset = Some(parser.value()?.string()?),
            Long("user-agent") => args.user_agent = Some(parser.value()?.string()?),
            Value(value) => {
                let value = value.string()?;
                if let Some(topics) = args.help_topics.as_mut() {
                    topics.push(value);
                    continue;
                }

                if !seen_net {
                    if value == "help" {
                        args.help_topics = Some(Vec::new());
                        continue;
                    }
                    if value != "net" {
                        return Err(Error::msg(format!(
                            "unknown command `{value}`; only `net` and `help` are supported"
                        )));
                    }
                    seen_net = true;
                    continue;
                }

                if value == "help" && args.net_command.is_none() {
                    args.help_topics = Some(vec!["net".to_owned()]);
                    continue;
                }

                if args.net_command.is_some() {
                    return Err(Error::msg(format!(
                        "unexpected positional argument `{value}`"
                    )));
                }

                args.net_command = Some(match value.as_str() {
                    "info" => NetCommand::Info,
                    "login" => NetCommand::Login,
                    "logout" => NetCommand::Logout,
                    "internet" => NetCommand::Internet,
                    _ => {
                        return Err(Error::msg(format!(
                            "unknown net command `{value}`; use info, login, logout, or internet"
                        )));
                    }
                });
            }
            _ => return Err(arg.unexpected().into()),
        }
    }

    Ok(args)
}

fn merge(args: &Args, cfg: &AppConfig) -> Result<Effective> {
    let endpoint = args
        .endpoint
        .clone()
        .or_else(|| cfg.net.endpoint.clone())
        .unwrap_or_else(|| DEFAULT_ENDPOINT.to_owned());
    let username = args
        .username
        .clone()
        .or_else(|| cfg.net.auth.username.clone());
    let password = args
        .password
        .clone()
        .or_else(|| cfg.net.auth.password.clone());
    let iface = args.iface.clone().or_else(|| cfg.net.auth.iface.clone());

    let ua_preset = args.ua_preset.clone().or_else(|| cfg.net.ua_preset.clone());
    let user_agent = args
        .user_agent
        .clone()
        .or_else(|| cfg.net.user_agent.clone());
    let resolved_user_agent = if let Some(value) = args.user_agent.as_deref() {
        ua::resolve(None, Some(value))?
    } else if let Some(value) = cfg.net.user_agent.as_deref() {
        ua::resolve(None, Some(value))?
    } else if let Some(value) = args.ua_preset.as_deref() {
        ua::resolve(Some(value), None)?
    } else if let Some(value) = cfg.net.ua_preset.as_deref() {
        ua::resolve(Some(value), None)?
    } else {
        ua::resolve(None, None)?
    };
    let acid = args
        .acid
        .clone()
        .or_else(|| cfg.net.acid.clone())
        .unwrap_or_else(|| infer_acid(resolved_user_agent.clone()));

    Ok(Effective {
        endpoint,
        acid,
        username,
        password,
        iface,
        ua_preset,
        user_agent,
        resolved_user_agent,
    })
}

fn apply_effective_to_config(cfg: &mut AppConfig, effective: &Effective) {
    cfg.net.endpoint = Some(effective.endpoint.clone());
    cfg.net.acid = Some(effective.acid.clone());
    cfg.net.auth.username = effective.username.clone();
    cfg.net.auth.password = effective.password.clone();
    cfg.net.auth.iface = effective.iface.clone();
    cfg.net.ua_preset = effective.ua_preset.clone();
    cfg.net.user_agent = effective.user_agent.clone();
}

fn infer_acid(user_agent: Option<String>) -> String {
    let Ok(client) = HttpClient::new(user_agent, None) else {
        return DEFAULT_ACID.to_owned();
    };
    let Ok(response) = client.get("http://www.baidu.com") else {
        return DEFAULT_ACID.to_owned();
    };
    let Ok(final_url) = Url::parse(&response.final_url) else {
        return DEFAULT_ACID.to_owned();
    };

    final_url
        .query_pairs()
        .find_map(|(key, value)| (key == "ac_id").then(|| value.into_owned()))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_ACID.to_owned())
}

fn print_help() {
    println!(
        "Westlake University net login cli

Usage:
  wlu-login-cli [FLAGS] net <info|login|logout|internet> [NET FLAGS]

Flags:
  --config <FILE>             config file (default: $HOME/.wlu-login-cli.yaml)
  -s, --save                  save effective config
  -V, --verbose               show request URLs and extra responses
  -h, --help                  show help
  -v, --version               show version

Net flags:
  -e, --endpoint <URL>        SRUN endpoint (default: http://10.1.10.1)
  -a, --acid <AC_ID>          SRUN ac_id (default: 1)
  -u, --username <VALUE>      SRUN username
  -p, --password <VALUE>      SRUN password
  -f, --interface <NAME>      network interface to bind requests
  -d, --daemon                keep checking and retry login
  -i, --interval <SECONDS>    daemon check interval (default: 60)
      --ua-preset <PRESET>    windows-chrome, windows-edge, mac-safari, ios-safari, android-chrome, none
      --user-agent <VALUE>    custom User-Agent; overrides preset"
    );
}

fn print_topic_help(topics: &[String]) -> Result<()> {
    let topic = topics.iter().map(String::as_str).collect::<Vec<_>>();
    match topic.as_slice() {
        [] => print_help(),
        ["net"] => print_net_help(),
        ["net", "info"] | ["info"] => print_net_info_help(),
        ["net", "login"] | ["login"] => print_net_login_help(),
        ["net", "logout"] | ["logout"] => print_net_logout_help(),
        ["net", "internet"] | ["internet"] => print_net_internet_help(),
        _ => {
            return Err(Error::msg(format!(
                "unknown help topic `{}`",
                topics.join(" ")
            )));
        }
    }
    Ok(())
}

fn print_net_help() {
    println!(
        "westlake university network auth cli

Usage:
  wlu-login-cli net <info|login|logout|internet> [NET FLAGS]

Available Commands:
  info        show info of your Westlake University network
  login       login Westlake University of the account
  logout      logout the account
  internet    check if connect to the internet

Net flags:
  -e, --endpoint <URL>        SRUN endpoint (default: http://10.1.10.1)
  -a, --acid <AC_ID>          SRUN ac_id; if omitted, try baidu redirect ac_id, fallback 1
  -f, --interface <NAME>      network interface to bind requests
      --ua-preset <PRESET>    windows-chrome, windows-edge, mac-safari, ios-safari, android-chrome, none
      --user-agent <VALUE>    custom User-Agent; overrides preset"
    );
}

fn print_net_info_help() {
    println!(
        "show info of your Westlake University network

Usage:
  wlu-login-cli net info [FLAGS]

Flags:
  -f, --interface <NAME>      network interface to bind requests"
    );
}

fn print_net_login_help() {
    println!(
        "login Westlake University of the account

Usage:
  wlu-login-cli net login [FLAGS]

Flags:
  -u, --username <VALUE>      SRUN username
  -p, --password <VALUE>      SRUN password
  -d, --daemon                keep checking and retry login
  -i, --interval <SECONDS>    daemon check interval (default: 60)
  -f, --interface <NAME>      network interface to bind requests"
    );
}

fn print_net_logout_help() {
    println!(
        "logout the account

Usage:
  wlu-login-cli net logout [FLAGS]

Flags:
  -u, --username <VALUE>      accepted for compatibility; current online user is used
  -f, --interface <NAME>      network interface to bind requests"
    );
}

fn print_net_internet_help() {
    println!(
        "check if connect to the internet

Usage:
  wlu-login-cli net internet [FLAGS]

Flags:
  -f, --interface <NAME>      network interface to bind requests"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_ua_precedence_prefers_config_user_agent_over_cli_preset() {
        let args = Args {
            acid: Some("1".to_owned()),
            ua_preset: Some("none".to_owned()),
            ..Args::default()
        };
        let mut cfg = AppConfig::default();
        cfg.net.user_agent = Some("from-config".to_owned());
        let effective = merge(&args, &cfg).unwrap();
        assert_eq!(
            effective.resolved_user_agent,
            Some("from-config".to_owned())
        );
    }

    #[test]
    fn configured_acid_skips_network_inference() {
        let args = Args::default();
        let mut cfg = AppConfig::default();
        cfg.net.acid = Some("6".to_owned());
        let effective = merge(&args, &cfg).unwrap();
        assert_eq!(effective.acid, "6");
    }
}
