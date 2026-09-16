use std::fs;

use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use jira_tui::app;
use jira_tui::config::Config;
use jira_tui::error::Result;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing()?;
    let config = Config::load()?;
    if let Err(err) = app::run(config).await {
        ratatui::restore();
        eprintln!("{err}");
        std::process::exit(1);
    }
    Ok(())
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
