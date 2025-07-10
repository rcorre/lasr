use anyhow::Result;
use clap::Parser;
use sploot::tui::App;
use tracing_error::ErrorLayer;
use tracing_subscriber::{Layer as _, layer::SubscriberExt as _, util::SubscriberInitExt as _};

#[derive(Parser)]
#[command(version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {}

pub fn initialize_logging() -> Result<()> {
    let xdg_dirs = xdg::BaseDirectories::with_prefix(env!("CARGO_PKG_NAME"));
    let log_path = xdg_dirs.place_cache_file("log.txt")?;
    let log_file = std::fs::File::create(log_path)?;
    let file_subscriber = tracing_subscriber::fmt::layer()
        .with_file(true)
        .with_line_number(true)
        .with_writer(log_file)
        .with_target(false)
        .with_ansi(false)
        .with_filter(tracing_subscriber::filter::EnvFilter::from_default_env());
    tracing_subscriber::registry()
        .with(file_subscriber)
        .with(ErrorLayer::default())
        .init();
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    initialize_logging()?;

    let cli = Cli::parse();

    let mut terminal = ratatui::init();
    crossterm::execute!(
        std::io::stdout(),
        crossterm::cursor::SetCursorStyle::BlinkingBar
    )?;
    let result = App::new()?.run(&mut terminal).await;
    ratatui::restore();
    result
}
