use super::{
    diff::{ModifiedExtension, PullRequestUpdate},
    LogicResult,
};
use crate::logic::download::get_url;
use anyhow::Context;

fn match_regex(line: &str, regex: &regex::Regex) -> Option<String> {
    regex
        .captures(line)
        .map(|cap| cap.get(1))
        .flatten()
        .map(|m| m.as_str().to_string())
}

// This SUCKS. We should output JSON in the future that we can parse.
fn parse_extensions_from_log(log: &str) -> anyhow::Result<Vec<ModifiedExtension>> {
    let mut extensions = Vec::new();

    let repository_regex = regex::Regex::new(r"- Repository: <(.+)>")?;
    let old_commit_regex = regex::Regex::new(r"- Old commit: \[(.+)\]")?;
    let new_commit_regex = regex::Regex::new(r"- New commit: \[(.+)\]")?;

    let mut ext_id = None;
    let mut ext_repository = None;
    let mut ext_old_commit = None;

    for line in log.lines() {
        // Remove the timestamp
        let line = line.splitn(2, ' ').nth(1).context("Invalid log line")?;

        if line.starts_with("## ") {
            let id = line[3..].to_string();
            log::debug!("Found extension ID: {}", id);
            ext_id = Some(id);
        } else if let Some(id) = &ext_id {
            if let Some(repo) = match_regex(line, &repository_regex) {
                log::debug!("Found repo: {}", repo);
                ext_repository = Some(repo);
            } else if let Some(old_commit) = match_regex(line, &old_commit_regex) {
                log::debug!("Found old commit: {}", old_commit);
                ext_old_commit = Some(old_commit);
            } else if let Some(new_commit) = match_regex(line, &new_commit_regex) {
                log::debug!("Found new commit: {}", new_commit);
                extensions.push(ModifiedExtension {
                    id: id.clone(),
                    repository: ext_repository
                        .clone()
                        .context("Repository is out of order")?,
                    old_commit: ext_old_commit
                        .clone()
                        .context("Old commit is out of order")?,
                    new_commit: new_commit,
                });

                ext_id = None;
            }
        }
    }

    Ok(extensions)
}

pub async fn get_pull_request(
    client: &octocrab::Octocrab,
    num: u64,
) -> LogicResult<PullRequestUpdate> {
    log::debug!("Getting pull request {}", num);

    // First, the commit of the PR is needed
    let pr = client
        .pulls("moonlight-mod", "extensions")
        .get(num)
        .await
        .context("Getting pull request failed")?;

    // Use that commit to find the workflow run
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

    // Use the workflow run to find the job
    let jobs = client
        .workflows("moonlight-mod", "extensions")
        .list_jobs(run.id)
        .send()
        .await
        .context("Getting jobs failed")?
        .take_items();
    let job = jobs.first().context("No jobs for run")?;

    let log = get_url(
        &client,
        format!(
            "https://api.github.com/repos/moonlight-mod/extensions/actions/jobs/{}/logs",
            job.id
        )
        .as_str(),
    )
    .await?;
    let log = std::str::from_utf8(&log).context("Invalid log")?;
    let extensions = parse_extensions_from_log(log)?;

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
