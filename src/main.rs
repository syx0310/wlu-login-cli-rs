mod cli;
mod config;
mod crypto;
mod error;
mod http_client;
mod jsonp;
mod portal;
mod table;
mod ua;

use error::Result;

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    cli::run()
}
