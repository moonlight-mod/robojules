use super::LogicResult;
use anyhow::Context;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct ModifiedExtension {
    pub id: String,
    pub repository: String,
    pub old_commit: String,
    pub new_commit: String,
}

#[derive(Debug, Clone)]
pub struct PullRequestUpdate {
    pub extensions: Vec<ModifiedExtension>,
    pub artifact_url: String,
}

#[derive(Debug, Clone)]
pub struct DiffedExtension {
    pub source_diff: Diff,
    pub asar_diff: Diff,
}

#[derive(Debug, Clone)]
pub enum FileState {
    Modified,
    Added,
    Removed,
}

pub type Directory = Vec<FilesystemItem>;

#[derive(Debug, Clone)]
pub enum FilesystemItem {
    File {
        name: String,
        state: FileState,
    },
    Directory {
        name: Option<String>,
        children: Directory,
    },
}

#[derive(Debug, Clone)]
pub struct Diff {
    pub old: PathBuf,
    pub new: PathBuf,
    pub dir: Directory,
}

// path/to/file -> sha256
pub async fn get_dir_tree(dir: &Path) -> anyhow::Result<HashMap<String, String>> {
    let mut tree = HashMap::new();

    let mut files = tokio::fs::read_dir(dir)
        .await
        .context("Failed to read directory")?;

    while let Some(file) = files.next_entry().await? {
        let path = file.path();
        let path_str = path.strip_prefix(dir)?.to_string_lossy().to_string();

        if path.is_dir() {
            let children = Box::pin(get_dir_tree(&path)).await?;
            for (child_path, hash) in children {
                tree.insert(format!("{}/{}", path_str, child_path), hash);
            }
        } else {
            let mut hash = Sha256::new();
            hash.update(tokio::fs::read(&path).await?);
            tree.insert(path_str, format!("{:x}", hash.finalize()));
        }
    }

    Ok(tree)
}

pub fn unflatten_tree(
    tree: &HashMap<String, FileState>,
    prefix: Option<String>,
) -> anyhow::Result<Directory> {
    let mut children: Vec<FilesystemItem> = Vec::new();

    let items = tree
        .keys()
        .filter(|path| {
            if let Some(prefix) = &prefix {
                path.starts_with(prefix)
            } else {
                true
            }
        })
        .map(|path| {
            if let Some(prefix) = &prefix {
                path.strip_prefix(format!("{}/", prefix).as_str())
                    .unwrap()
                    .to_string()
            } else {
                path.clone()
            }
        })
        .collect::<Vec<_>>();

    let files = items
        .iter()
        .filter(|path| !path.contains('/'))
        .collect::<Vec<_>>();
    let dirs = items
        .iter()
        .filter(|path| path.contains('/'))
        .map(|path| path.split('/').next().unwrap())
        .collect::<Vec<_>>();

    for dir in dirs {
        if children.iter().any(|item| match item {
            FilesystemItem::Directory { name, .. } => name.as_deref() == Some(dir),
            _ => false,
        }) {
            continue;
        }

        let path = if let Some(prefix) = &prefix {
            format!("{}/{}", prefix, dir)
        } else {
            dir.to_string()
        };

        let subtree = unflatten_tree(tree, Some(path))?;
        children.push(FilesystemItem::Directory {
            name: Some(dir.to_string()),
            children: subtree,
        });
    }

    for file in files {
        let path = if let Some(prefix) = &prefix {
            format!("{}/{}", prefix, file)
        } else {
            file.to_string()
        };
        if let Some(state) = tree.get(&path) {
            children.push(FilesystemItem::File {
                name: file.to_string(),
                state: state.clone(),
            });
        }
    }

    Ok(children)
}

pub async fn calculate_diff(old_dir: &Path, new_dir: &Path) -> anyhow::Result<Diff> {
    let old_tree = get_dir_tree(old_dir).await?;
    let new_tree = get_dir_tree(new_dir).await?;

    let mut tree = HashMap::new();
    for (path, old_hash) in &old_tree {
        if let Some(new_hash) = new_tree.get(&*path) {
            if *old_hash != *new_hash {
                tree.insert(path.clone(), FileState::Modified);
            }
        } else {
            tree.insert(path.clone(), FileState::Removed);
        }
    }
    for (path, _) in new_tree {
        if old_tree.get(&path).is_none() {
            tree.insert(path, FileState::Added);
        }
    }

    let file_tree = unflatten_tree(&tree, None)?;

    Ok(Diff {
        old: old_dir.to_path_buf(),
        new: new_dir.to_path_buf(),
        dir: file_tree,
    })
}

pub async fn get_diff_string(old: &Path, new: &Path) -> LogicResult<String> {
    if !old.exists() {
        return tokio::fs::read_to_string(new)
            .await
            .map_err(|e| e.to_string().into());
    }

    if !new.exists() {
        return Ok("(deleted file)".to_string());
    }

    let width = std::env::var("DFT_WIDTH").unwrap_or_else(|_| "240".to_string());
    let stdout = Command::new("difft")
        .arg(old)
        .arg(new)
        .env("DFT_COLOR", "always")
        .env("DFT_WIDTH", width)
        .env("DFT_SYNTAX_HIGHLIGHT", "on")
        .env("DFT_STRIP_CR", "on")
        .output()
        .await
        .context("Failed to run difft")?
        .stdout;

    Ok(String::from_utf8(stdout).map_err(|_| anyhow::anyhow!("Invalid UTF-8 in diff"))?)
}
