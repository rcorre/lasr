use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::Parser;
use etcetera::{AppStrategy, AppStrategyArgs, choose_app_strategy};
use lasr::config::Config;
use lasr::tui::App;
use tracing::debug;
use tracing_error::ErrorLayer;
use tracing_subscriber::{Layer as _, layer::SubscriberExt as _, util::SubscriberInitExt as _};

#[derive(Parser)]
#[command(version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    /// Paths to search, defaults to "."
    paths: Vec<PathBuf>,

    #[arg(short, long)]
    /// Path to the config file, defaults to $XDG_CONFIG_HOME/lasr/lasr.toml (~/.config/lasr/lasr.toml).
    /// No config is loaded if an empty string is given.
    config_path: Option<PathBuf>,

    #[arg(long)]
    /// Print the current config to stdout and exit
    dump_config: bool,

    #[arg(short, long)]
    /// Whether to start with the ignore-case option enabled
    ignore_case: bool,

    #[arg(short, long = "type", default_values_t=["all".to_string()])]
    /// File types to search, use --type-list to view available types
    types: Vec<String>,

    #[arg(long)]
    /// List all file types available to -t
    type_list: bool,
}

fn strategy() -> AppStrategyArgs {
    etcetera::AppStrategyArgs {
        app_name: env!("CARGO_PKG_NAME").to_string(),
        author: "rrc".to_string(),
        top_level_domain: "codes".to_string(),
    }
}

fn initialize_logging() -> Result<()> {
    let strategy = choose_app_strategy(strategy())?;
    let cache_dir = strategy.cache_dir();
    let log_path = cache_dir.join("log.txt");
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

fn load_config(path: Option<PathBuf>) -> Result<Config> {
    let path = if let Some(path) = path {
        path
    } else {
        let strategy = choose_app_strategy(strategy())?;
        strategy.config_dir().join("lasr.toml")
    };
    if path.as_os_str().is_empty() {
        debug!("Skipping config load");
        return Ok(Config::default());
    }
    debug!("Loading config from {path:?}");
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(toml::from_str(&s)?),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(err) => bail!(err),
    }
}

fn main() -> Result<()> {
    initialize_logging()?;

    let cli = Cli::parse();

    if cli.type_list {
        let mut types = ignore::types::TypesBuilder::new();
        types.add_defaults();
        for def in types.build()?.definitions() {
            println!("{}: {:?}", def.name(), def.globs());
        }
        return Ok(());
    }

    let config = load_config(cli.config_path)?;

    if cli.dump_config {
        print!("{}", toml::to_string_pretty(&config)?);
        return Ok(());
    }

    let mut types = ignore::types::TypesBuilder::new();
    types.add_defaults();
    for t in cli.types {
        types.select(&t);
    }
    let types = match types.build() {
        Ok(types) => types,
        Err(err) => {
            eprintln!("{err}");
            return Ok(());
        }
    };

    let mut terminal = ratatui::init();
    crossterm::execute!(
        std::io::stdout(),
        crossterm::cursor::SetCursorStyle::BlinkingBar
    )?;

    let (tx, rx) = crossbeam::channel::bounded(0);
    std::thread::spawn(move || {
        loop {
            let ev = crossterm::event::read().unwrap();
            if tx.send(ev).is_err() {
                break;
            };
        }
    });
    {
        let mut app = App::new(cli.paths, types, config, rx, cli.ignore_case);
        app.run(&mut terminal)?;
    }

    ratatui::restore();
    Ok(())
}
