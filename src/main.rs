use std::fs;

use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use jira_tui::app;
use jira_tui::auth::oauth;
use jira_tui::config::Config;
use jira_tui::error::{Error, Result};

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing()?;
    let cli = token_service_from_args()?;
    let url = oauth::configure_token_service(cli.as_deref());
    tracing::info!(url = %url, "using token service");
    let config = Config::load()?;
    if let Err(err) = app::run(config).await {
        ratatui::restore();
        eprintln!("{err}");
        std::process::exit(1);
    }
    Ok(())
}

fn token_service_from_args() -> Result<Option<String>> {
    let mut args = std::env::args().skip(1);
    let mut url = None;
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--token-service=") {
            if value.is_empty() {
                return Err(Error::config("--token-service requires a URL"));
            }
            url = Some(value.to_string());
        } else if arg == "--token-service" {
            let value = args
                .next()
                .ok_or_else(|| Error::config("--token-service requires a URL"))?;
            if value.is_empty() {
                return Err(Error::config("--token-service requires a URL"));
            }
            url = Some(value);
        } else if arg == "--help" || arg == "-h" {
            println!(
                "Usage: jira-tui [--token-service URL]\n\n  URL overrides JIRA_TUI_TOKEN_SERVICE.\n  Default: {}",
                oauth::DEFAULT_TOKEN_SERVICE
            );
            std::process::exit(0);
        } else {
            return Err(Error::config(format!("unknown argument {arg}")));
        }
    }
    Ok(url)
}

fn init_tracing() -> Result<()> {
    let log_dir = Config::log_dir()?;
    fs::create_dir_all(&log_dir)?;
    let file_appender = tracing_appender::rolling::daily(log_dir, "jira-tui.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    // Keep the guard for the process lifetime.
    std::mem::forget(guard);
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(non_blocking),
        )
        .init();
    Ok(())
}
