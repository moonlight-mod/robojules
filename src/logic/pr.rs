use super::{
    diff::{ModifiedExtension, PullRequestUpdate},
    LogicResult,
};
use crate::logic::download::get_url;
use anyhow::Context;
use serde::Deserialize;

#[derive(Deserialize)]
struct ExtensionManifest {
    repository: String,
    commit: String,
}

pub async fn get_pull_request(
    client: &octocrab::Octocrab,
    num: u64,
) -> LogicResult<PullRequestUpdate> {
    log::debug!("Getting pull request {}", num);

    let pr = client
        .pulls("moonlight-mod", "extensions")
        .get(num)
        .await
        .context("Getting pull request failed")?;
    let changed_files = client
        .pulls("moonlight-mod", "extensions")
        .list_files(num)
        .await
        .context("Getting changed files failed")?
        .take_items();

    let mut extensions = Vec::new();
    for file in changed_files {
        if file.filename.starts_with("exts/") {
            let old = format!(
                "https://raw.githubusercontent.com/moonlight-mod/extensions/{}/{}",
                pr.base.sha, file.filename
            );
            let new = format!(
                "https://raw.githubusercontent.com/moonlight-mod/extensions/{}/{}",
                pr.head.sha, file.filename
            );

            let ext_id = file
                .filename
                .trim_start_matches("exts/")
                .trim_end_matches(".json");

            let old = get_url(client, &old)
                .await
                .context("Failed to download old file")?;
            let old = std::str::from_utf8(&old).context("Failed to parse old file")?;
            let old = serde_json::from_str::<ExtensionManifest>(old)
                .context("Failed to parse old manifest")?;

            let new = get_url(client, &new)
                .await
                .context("Failed to download new file")?;
            let new = std::str::from_utf8(&new).context("Failed to parse new file")?;
            let new = serde_json::from_str::<ExtensionManifest>(new)
                .context("Failed to parse new manifest")?;

            extensions.push(ModifiedExtension {
                id: ext_id.to_string(),
                repository: old.repository,
                old_commit: old.commit,
                new_commit: new.commit,
            });
        }
    }

    let runs = client
        .workflows("moonlight-mod", "extensions")
        .list_runs("pull_request.yml")
        .event("pull_request")
        .send()
        .await
        .context("Getting workflows failed")?
        .take_items();
    let run = runs
        .iter()
        .find(|run| {
            run.head_sha == pr.head.sha
                && run.event == "pull_request"
                && run.status == "completed"
                && run.conclusion == Some("success".to_string())
        })
        .context("No run found for PR")?;

    let artifacts = client
        .actions()
        .list_workflow_run_artifacts("moonlight-mod", "extensions", run.id)
        .send()
        .await
        .context("Getting artifacts failed")?
        .value
        .context("No artifacts for run")?
        .take_items();
    let artifact = artifacts.first().context("No artifacts for run")?;

    Ok(PullRequestUpdate {
        extensions,
        // The actual artifact URL requires you to be authenticated, so we can't use it
        // nightly.link is trustworthy
        artifact_url: format!(
            "https://nightly.link/moonlight-mod/extensions/actions/runs/{}/{}.zip",
            run.id, artifact.name
        ),
    })
}
