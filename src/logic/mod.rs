use crate::logic::diff::FileDiff;
use anyhow::Context;
use diff::{DiffedExtension, ModifiedExtension, PullRequestUpdate};
use std::path::PathBuf;
use tokio::runtime::Runtime;

pub mod asar;
pub mod diff;
pub mod download;
pub mod pr;

pub const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Clone, Debug)]
pub struct LogicError(String);
pub type LogicResult<T> = Result<T, LogicError>;

impl std::fmt::Display for LogicError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<anyhow::Error> for LogicError {
    fn from(err: anyhow::Error) -> Self {
        Self(format!("{:?}", err))
    }
}

impl From<String> for LogicError {
    fn from(err: String) -> Self {
        Self(err)
    }
}

#[derive(Debug, Clone)]
pub enum LogicCommand {
    GetPullRequest(u64),
    DownloadExtension {
        extension: ModifiedExtension,
        artifact_url: String,
    },
    DiffFile(PathBuf, PathBuf),
}

#[derive(Debug, Clone)]
pub enum LogicResponse {
    PullRequest(LogicResult<PullRequestUpdate>),
    ExtensionDownloadComplete(LogicResult<DiffedExtension>),
    FileDiff(LogicResult<FileDiff>),
}

fn build_octocrab() -> anyhow::Result<octocrab::Octocrab> {
    octocrab::Octocrab::builder()
        .build()
        .context("Failed to build Octocrab client")
}

async fn app_logic_thread_inner(
    rx: flume::Receiver<LogicCommand>,
    tx: flume::Sender<LogicResponse>,
) -> anyhow::Result<()> {
    let client = build_octocrab()?;

    loop {
        match rx.recv()? {
            LogicCommand::GetPullRequest(num) => {
                let res = pr::get_pull_request(&client, num).await;
                log::debug!("Got pull request: {:?}", res);
                tx.send(LogicResponse::PullRequest(res))?;
            }

            LogicCommand::DownloadExtension {
                extension,
                artifact_url,
            } => {
                let res = download::download_extension(&client, &extension, &artifact_url).await;
                log::debug!("Downloaded extension: {:?}", res);
                tx.send(LogicResponse::ExtensionDownloadComplete(res))?;
            }

            LogicCommand::DiffFile(old, new) => {
                let res = diff::calculate_file_diff(&old, &new).await;
                tx.send(LogicResponse::FileDiff(res))?;
            }
        }
    }
}

// I'm not sure how well Tokio will work if the main thread is blocking, so we do this on a separate thread
// I could also be wrong, because I didn't test it, lmfao
pub fn app_logic_thread(rx: flume::Receiver<LogicCommand>, tx: flume::Sender<LogicResponse>) {
    let runtime = Runtime::new().expect("Unable to create the runtime");
    runtime.block_on(async move {
        if let Err(err) = app_logic_thread_inner(rx, tx).await {
            log::error!("Logic thread error: {:?}", err);
        }
    });
}
