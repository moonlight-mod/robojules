use super::{
    asar::FileTree,
    diff::{DiffedExtension, ModifiedExtension},
    LogicResult,
};
use crate::logic::{asar::parse_asar, diff};
use anyhow::Context;
use http_body_util::BodyExt;
use std::{
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

pub async fn get_url(client: &octocrab::Octocrab, url: &str) -> anyhow::Result<Vec<u8>> {
    let req = client._get(url).await?;
    let req = client.follow_location_to_data(req).await?;
    Ok(req.into_body().collect().await?.to_bytes().to_vec())
}

pub async fn get_asar_from_zip(zip: Vec<u8>, ext_id: &str) -> anyhow::Result<FileTree> {
    let mut zip = zip::ZipArchive::new(Cursor::new(zip)).context("Failed to open zip")?;

    let file = zip
        .by_name(format!("{}.asar", ext_id).as_str())
        .context("Failed to find .asar")?;
    let bytes = file
        .bytes()
        .collect::<Result<Vec<u8>, _>>()
        .context("Failed to read .asar")?;

    let mut reader = Cursor::new(bytes);
    let asar = parse_asar(&mut reader).context("Failed to parse .asar")?;
    Ok(asar)
}

pub async fn extract_asar(asar: &FileTree, dir: &Path) -> anyhow::Result<()> {
    for (path, data) in asar {
        let path = dir.join(path);
        let parent = path.parent().context("No parent")?;
        tokio::fs::create_dir_all(parent)
            .await
            .context("Failed to create parent dir")?;
        tokio::fs::write(&path, data)
            .await
            .context("Failed to write file")?;
    }

    Ok(())
}

pub async fn copy_recursive(src: PathBuf, dest: PathBuf) -> std::io::Result<()> {
    let mut files = tokio::fs::read_dir(src).await?;

    while let Some(entry) = files.next_entry().await? {
        let path = entry.path();
        let file_name = path.file_name().unwrap();
        if file_name == ".git" {
            continue;
        }

        let dest = dest.join(file_name);

        if path.is_dir() {
            tokio::fs::create_dir(&dest).await?;
            Box::pin(copy_recursive(path, dest)).await?;
        } else {
            tokio::fs::copy(&path, &dest).await?;
        }
    }

    Ok(())
}

pub async fn checkout_copy(src: PathBuf, dest: PathBuf, commit: &str) -> anyhow::Result<()> {
    log::debug!("Checking out commit {}", commit);

    tokio::process::Command::new("git")
        .arg("checkout")
        .arg(commit)
        .current_dir(&src)
        .output()
        .await
        .context("Failed to checkout commit")?;
    copy_recursive(src, dest)
        .await
        .context("Failed to copy files")
}

pub async fn download_extension(
    client: &octocrab::Octocrab,
    ext: &ModifiedExtension,
    artifact_url: &str,
) -> LogicResult<DiffedExtension> {
    log::debug!("Downloading extension {}", ext.id);

    let temp_dir = std::env::temp_dir().join("robojules").join(ext.id.clone());
    if temp_dir.exists() {
        tokio::fs::remove_dir_all(&temp_dir)
            .await
            .context("Failed to remove old temp dir")?;
    }
    tokio::fs::create_dir_all(&temp_dir)
        .await
        .context("Failed to create temp dir")?;

    let old_asar_dir = temp_dir.join("old_asar");
    let new_asar_dir = temp_dir.join("new_asar");
    let source_dir = temp_dir.join("source");
    let old_source_dir = temp_dir.join("old_source");
    let new_source_dir = temp_dir.join("new_source");

    for dir in vec![
        &old_asar_dir,
        &new_asar_dir,
        &source_dir,
        &old_source_dir,
        &new_source_dir,
    ] {
        tokio::fs::create_dir(dir)
            .await
            .context("Failed to create temp dir")?;
    }

    log::debug!("Downloading artifact .asar from {}", artifact_url);
    let artifact_asar = get_url(client, artifact_url)
        .await
        .context("Failed to download artifact .asar")?;
    let artifact_asar = get_asar_from_zip(artifact_asar, &ext.id)
        .await
        .context("Failed to parse artifact .asar")?;
    extract_asar(&artifact_asar, &new_asar_dir)
        .await
        .context("Failed to extract artifact .asar")?;

    let current_asar_url = format!(
        "https://github.com/moonlight-mod/extensions-dist/raw/refs/heads/main/exts/{}.asar",
        ext.id
    );
    log::debug!("Downloading current .asar from {}", current_asar_url);
    let current_asar = get_url(client, &current_asar_url).await?;
    let mut current_asar = Cursor::new(current_asar);
    let current_asar = parse_asar(&mut current_asar).context("Failed to parse current .asar")?;
    extract_asar(&current_asar, &old_asar_dir)
        .await
        .context("Failed to extract current .asar")?;

    let asar_diff = diff::calculate_diff(&old_asar_dir, &new_asar_dir)
        .await
        .context("Failed to diff .asar")?;

    // --branch doesn't work with commit hashes, so let's clone the entire repo and copy files
    log::debug!("Cloning repository {}", ext.repository);
    tokio::process::Command::new("git")
        .arg("clone")
        .arg(ext.repository.clone())
        .arg(&source_dir)
        .output()
        .await
        .context("Failed to clone repository")?;

    checkout_copy(source_dir.clone(), new_source_dir.clone(), &ext.new_commit)
        .await
        .context("Failed to checkout new commit")?;
    checkout_copy(source_dir.clone(), old_source_dir.clone(), &ext.old_commit)
        .await
        .context("Failed to checkout old commit")?;
    let source_diff = diff::calculate_diff(&old_source_dir, &new_source_dir)
        .await
        .context("Failed to diff source")?;

    Ok(DiffedExtension {
        source_diff,
        asar_diff,
    })
}
