//! The parts of the GitHub API a firmware build needs.

use std::io::Read;
use std::process::Command;

use serde::Deserialize;

use crate::BuildError;

const API: &str = "https://api.github.com";

/// Where a workflow run has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    /// Queued or running.
    InProgress,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub id: u64,
    pub state: RunState,
    /// The run's page on github.com.
    pub url: String,
}

#[derive(Deserialize)]
struct RunJson {
    id: u64,
    status: String,
    conclusion: Option<String>,
    html_url: String,
}

impl From<RunJson> for Run {
    fn from(run: RunJson) -> Self {
        let state = match (run.status.as_str(), run.conclusion.as_deref()) {
            ("completed", Some("success")) => RunState::Succeeded,
            ("completed", _) => RunState::Failed,
            _ => RunState::InProgress,
        };
        Run {
            id: run.id,
            state,
            url: run.html_url,
        }
    }
}

#[derive(Deserialize)]
struct RunsJson {
    workflow_runs: Vec<RunJson>,
}

#[derive(Deserialize)]
struct ArtifactJson {
    id: u64,
}

#[derive(Deserialize)]
struct ArtifactsJson {
    artifacts: Vec<ArtifactJson>,
}

#[derive(Deserialize)]
struct JobJson {
    id: u64,
    name: String,
    conclusion: Option<String>,
}

#[derive(Deserialize)]
struct JobsJson {
    jobs: Vec<JobJson>,
}

/// A token for the GitHub API: from the environment, or from the GitHub
/// CLI if the user is signed in to it.
pub fn find_token() -> Option<String> {
    for name in ["GH_TOKEN", "GITHUB_TOKEN"] {
        if let Some(token) = std::env::var(name).ok().filter(|t| !t.is_empty()) {
            return Some(token);
        }
    }
    // Apps launched from the Finder do not inherit the shell's PATH.
    ["gh", "/opt/homebrew/bin/gh", "/usr/local/bin/gh"]
        .iter()
        .find_map(|gh| {
            let output = Command::new(gh).args(["auth", "token"]).output().ok()?;
            let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
            (output.status.success() && !token.is_empty()).then_some(token)
        })
}

/// The lines of a build log that say what went wrong, with the timestamps
/// GitHub prefixes removed. Falls back to the end of the log.
pub fn summarize_log(log: &str) -> String {
    let lines: Vec<&str> = log
        .lines()
        .map(|line| match line.split_once(' ') {
            Some((stamp, rest)) if stamp.ends_with('Z') && stamp.contains('T') => rest,
            _ => line,
        })
        .collect();
    let telling: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|l| {
            let lower = l.to_lowercase();
            lower.contains("error") || lower.contains("devicetree") && lower.contains("fail")
        })
        .take(20)
        .collect();
    if telling.is_empty() {
        let start = lines.len().saturating_sub(20);
        lines[start..].join("\n")
    } else {
        telling.join("\n")
    }
}

pub struct GitHub {
    token: String,
    agent: ureq::Agent,
}

impl GitHub {
    pub fn new(token: String) -> Self {
        Self {
            token,
            agent: ureq::AgentBuilder::new()
                .timeout(std::time::Duration::from_secs(60))
                .build(),
        }
    }

    fn request(&self, method: &str, url: &str) -> ureq::Request {
        self.agent
            .request(method, url)
            .set("Authorization", &format!("Bearer {}", self.token))
            .set("Accept", "application/vnd.github+json")
            .set("X-GitHub-Api-Version", "2022-11-28")
            .set("User-Agent", "keyboard-curator")
    }

    fn fail(error: ureq::Error) -> BuildError {
        match error {
            ureq::Error::Status(status, response) => BuildError::Api {
                status,
                message: response
                    .into_json::<serde_json::Value>()
                    .ok()
                    .and_then(|v| v["message"].as_str().map(str::to_string))
                    .unwrap_or_else(|| "no details".to_string()),
            },
            other => BuildError::Network(other.to_string()),
        }
    }

    fn get<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, BuildError> {
        self.request("GET", url)
            .call()
            .map_err(Self::fail)?
            .into_json()
            .map_err(|e| BuildError::Network(e.to_string()))
    }

    /// The signed-in user's login.
    pub fn user(&self) -> Result<String, BuildError> {
        let user: serde_json::Value = self.get(&format!("{API}/user"))?;
        Ok(user["login"].as_str().unwrap_or_default().to_string())
    }

    /// Creates a private repository for the signed-in user and returns its
    /// clone URL.
    pub fn create_repo(&self, name: &str) -> Result<String, BuildError> {
        let created: serde_json::Value = self
            .request("POST", &format!("{API}/user/repos"))
            .send_json(serde_json::json!({
                "name": name,
                "private": true,
                "description": "ZMK keyboard configuration, managed by Keyboard Curator",
            }))
            .map_err(Self::fail)?
            .into_json()
            .map_err(|e| BuildError::Network(e.to_string()))?;
        Ok(created["clone_url"]
            .as_str()
            .unwrap_or_default()
            .to_string())
    }

    /// The newest workflow run for a commit, once GitHub has started one.
    pub fn run_for_commit(
        &self,
        owner: &str,
        repo: &str,
        sha: &str,
    ) -> Result<Option<Run>, BuildError> {
        let runs: RunsJson = self.get(&format!(
            "{API}/repos/{owner}/{repo}/actions/runs?head_sha={sha}&per_page=5"
        ))?;
        Ok(runs.workflow_runs.into_iter().next().map(Run::from))
    }

    pub fn run(&self, owner: &str, repo: &str, id: u64) -> Result<Run, BuildError> {
        let run: RunJson = self.get(&format!("{API}/repos/{owner}/{repo}/actions/runs/{id}"))?;
        Ok(run.into())
    }

    /// The archives a finished run produced, as zip files.
    pub fn artifacts(&self, owner: &str, repo: &str, run: u64) -> Result<Vec<Vec<u8>>, BuildError> {
        let list: ArtifactsJson = self.get(&format!(
            "{API}/repos/{owner}/{repo}/actions/runs/{run}/artifacts"
        ))?;
        list.artifacts
            .iter()
            .map(|artifact| {
                let url = format!(
                    "{API}/repos/{owner}/{repo}/actions/artifacts/{}/zip",
                    artifact.id
                );
                let mut bytes = Vec::new();
                self.request("GET", &url)
                    .call()
                    .map_err(Self::fail)?
                    .into_reader()
                    .take(64 * 1024 * 1024)
                    .read_to_end(&mut bytes)?;
                Ok(bytes)
            })
            .collect()
    }

    /// What went wrong in a failed run: the telling lines of each failed
    /// job's log.
    pub fn failure(&self, owner: &str, repo: &str, run: u64) -> Result<String, BuildError> {
        let jobs: JobsJson = self.get(&format!(
            "{API}/repos/{owner}/{repo}/actions/runs/{run}/jobs"
        ))?;
        let mut report = String::new();
        for job in jobs
            .jobs
            .iter()
            .filter(|j| j.conclusion.as_deref() == Some("failure"))
        {
            let url = format!("{API}/repos/{owner}/{repo}/actions/jobs/{}/logs", job.id);
            let log = self
                .request("GET", &url)
                .call()
                .map_err(Self::fail)?
                .into_string()
                .unwrap_or_default();
            report.push_str(&format!("{}:\n{}\n\n", job.name, summarize_log(&log)));
        }
        Ok(report.trim_end().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(status: &str, conclusion: Option<&str>) -> RunState {
        Run::from(RunJson {
            id: 1,
            status: status.into(),
            conclusion: conclusion.map(str::to_string),
            html_url: String::new(),
        })
        .state
    }

    #[test]
    fn run_states_are_read_from_status_and_conclusion() {
        assert_eq!(run("queued", None), RunState::InProgress);
        assert_eq!(run("in_progress", None), RunState::InProgress);
        assert_eq!(run("completed", Some("success")), RunState::Succeeded);
        assert_eq!(run("completed", Some("failure")), RunState::Failed);
        assert_eq!(run("completed", Some("cancelled")), RunState::Failed);
    }

    #[test]
    fn api_responses_parse() {
        let runs: RunsJson = serde_json::from_str(
            r#"{"total_count":1,"workflow_runs":[{"id":42,"status":"completed","conclusion":"success","html_url":"https://github.com/o/r/actions/runs/42","extra":true}]}"#,
        )
        .unwrap();
        let run = Run::from(runs.workflow_runs.into_iter().next().unwrap());
        assert_eq!((run.id, run.state), (42, RunState::Succeeded));
        let artifacts: ArtifactsJson =
            serde_json::from_str(r#"{"artifacts":[{"id":7,"name":"firmware"}]}"#).unwrap();
        assert_eq!(artifacts.artifacts[0].id, 7);
    }

    #[test]
    fn failed_logs_are_reduced_to_what_went_wrong() {
        let log = "2026-10-04T06:00:00.000Z -- west build: generating a build system\n\
                   2026-10-04T06:00:01.000Z devicetree error: /keymap/layer_0: undefined node label 'nope'\n\
                   2026-10-04T06:00:02.000Z FATAL ERROR: command exited with status 1\n";
        assert_eq!(
            summarize_log(log),
            "devicetree error: /keymap/layer_0: undefined node label 'nope'\nFATAL ERROR: command exited with status 1"
        );
        assert_eq!(summarize_log("all\nfine"), "all\nfine");
    }
}
