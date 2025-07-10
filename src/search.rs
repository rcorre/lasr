use anyhow::Result;
use regex::Regex;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::mpsc::Sender;
use tracing::trace;

pub struct Finding {
    path: PathBuf,
    line_number: usize,
    line: String,
}

pub async fn search(tx: Sender<Finding>, re: Arc<Regex>, path: PathBuf) -> Result<()> {
    tokio::spawn(async move { search_task(tx, re, path).await }).await?
}

async fn search_task(tx: Sender<Finding>, re: Arc<Regex>, path: PathBuf) -> Result<()> {
    for entry in std::fs::read_dir(path.as_path())? {
        let entry = entry?;
        let meta = entry.metadata()?;
        let path = entry.path();
        trace!("Exploring path {path:?}: {meta:?}");
        if meta.is_dir() {
            Box::pin(search_task(tx.clone(), re.clone(), path)).await?;
        } else if meta.is_file() {
            let contents = std::fs::read_to_string(path.as_path())?;
            for (line_number, line) in contents.lines().enumerate() {
                if re.is_match(line) {
                    trace!("Found match in {path:?}:{line_number}: {line}");
                    tx.send(Finding {
                        path: path.clone(),
                        line_number,
                        line: line.to_string(),
                    })
                    .await?;
                }
            }
        }
    }
    Ok(())
}
