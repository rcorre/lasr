use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Result;
use regex::Regex;
use tokio::sync::Mutex;
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::trace;

pub struct Finding {
    path: PathBuf,
    line_number: usize,
    line: String,
}

pub async fn search(tx: Sender<Finding>, re: Arc<Mutex<Regex>>, path: PathBuf) {
    tokio::spawn(async move {
        search_task(tx, re, path).await.unwrap();
    });
}

async fn search_task(tx: Sender<Finding>, re: Arc<Mutex<Regex>>, path: PathBuf) -> Result<()> {
    for entry in std::fs::read_dir(path.as_path())? {
        let entry = entry?;
        let meta = entry.metadata()?;
        let path = entry.path();
        trace!("Exploring path {path:?}: {meta:?}");
        if meta.is_dir() {
            search(tx.clone(), re.clone(), path.into()).await;
        } else if meta.is_file() {
            let contents = std::fs::read_to_string(path.as_path())?;
            for (line_number, line) in contents.lines().enumerate() {
                if re.lock().await.is_match(&contents) {
                    trace!("Found match in {path:?}:{line_number}: {line}");
                    tx.send(Finding {
                        path: path.clone(),
                        line_number,
                        line: line.to_string(),
                    })
                    .await;
                }
            }
        }
    }
    Ok(())
}
