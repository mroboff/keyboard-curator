//! The local clone of a zmk-config repository, driven through `git`.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::BuildError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub dir: PathBuf,
}

/// The owner and name in a GitHub remote URL, in either the `https://` or
/// the `git@` form.
pub fn parse_remote(url: &str) -> Option<(String, String)> {
    let url = url.trim();
    let path = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("git@github.com:"))
        .or_else(|| url.strip_prefix("ssh://git@github.com/"))?;
    let path = path
        .strip_suffix(".git")
        .unwrap_or(path)
        .trim_end_matches('/');
    let (owner, name) = path.split_once('/')?;
    let valid = !owner.is_empty() && !name.is_empty() && !name.contains('/');
    valid.then(|| (owner.to_string(), name.to_string()))
}

impl Repo {
    /// Opens an existing clone.
    pub fn open(dir: &Path) -> Result<Self, BuildError> {
        let repo = Self {
            dir: dir.to_path_buf(),
        };
        match repo.git(&["rev-parse", "--is-inside-work-tree"]) {
            Ok(_) => Ok(repo),
            Err(BuildError::Git { .. }) => {
                Err(BuildError::NotARepository(dir.display().to_string()))
            }
            Err(other) => Err(other),
        }
    }

    /// Turns a folder into a new repository on the `main` branch.
    pub fn init(dir: &Path) -> Result<Self, BuildError> {
        std::fs::create_dir_all(dir)?;
        let repo = Self {
            dir: dir.to_path_buf(),
        };
        repo.git(&["init", "--initial-branch=main"])?;
        Ok(repo)
    }

    fn git(&self, args: &[&str]) -> Result<String, BuildError> {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.dir)
            // Never stop to ask for credentials; fail and report instead.
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map_err(BuildError::GitMissing)?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(BuildError::Git {
                command: args.first().copied().unwrap_or_default().to_string(),
                message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            })
        }
    }

    /// The GitHub owner and repository name of `origin`.
    pub fn github(&self) -> Result<(String, String), BuildError> {
        let url = self.git(&["remote", "get-url", "origin"])?;
        parse_remote(&url).ok_or(BuildError::NotGitHub(url))
    }

    pub fn set_origin(&self, url: &str) -> Result<(), BuildError> {
        match self.git(&["remote", "add", "origin", url]) {
            Ok(_) => Ok(()),
            Err(_) => self.git(&["remote", "set-url", "origin", url]).map(|_| ()),
        }
    }

    pub fn head(&self) -> Result<String, BuildError> {
        self.git(&["rev-parse", "HEAD"])
    }

    /// Writes files into the working tree, relative to the repository root.
    pub fn write(&self, files: &[(String, String)]) -> Result<(), BuildError> {
        for (path, contents) in files {
            let path = self.dir.join(path);
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(path, contents)?;
        }
        Ok(())
    }

    /// Commits whatever has changed. Returns false when nothing had.
    pub fn commit(&self, message: &str) -> Result<bool, BuildError> {
        self.git(&["add", "--all"])?;
        if self.git(&["status", "--porcelain"])?.is_empty() {
            return Ok(false);
        }
        self.git(&["commit", "--message", message])?;
        Ok(true)
    }

    /// Pushes the current branch to GitHub and returns the commit that was
    /// pushed. The token is passed for this one push only, so it works
    /// whether or not git has credentials for GitHub, and is never stored.
    pub fn push(&self, token: &str) -> Result<String, BuildError> {
        let (owner, name) = self.github()?;
        let branch = self.git(&["rev-parse", "--abbrev-ref", "HEAD"])?;
        let url = format!("https://x-access-token:{token}@github.com/{owner}/{name}.git");
        let refspec = format!("HEAD:refs/heads/{branch}");
        self.git(&["push", &url, &refspec])
            .map_err(|error| match error {
                // Keep the token out of anything shown to the user.
                BuildError::Git { command, message } => BuildError::Git {
                    command,
                    message: message.replace(token, "<token>"),
                },
                other => other,
            })?;
        self.head()
    }

    /// Whether the folder has any commits yet.
    pub fn has_commits(&self) -> bool {
        self.head().is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_remotes_are_recognized_in_every_form() {
        let expected = Some(("mroboff".to_string(), "zmk-config".to_string()));
        for url in [
            "https://github.com/mroboff/zmk-config.git",
            "https://github.com/mroboff/zmk-config",
            "git@github.com:mroboff/zmk-config.git",
            "ssh://git@github.com/mroboff/zmk-config.git\n",
        ] {
            assert_eq!(parse_remote(url), expected, "{url}");
        }
        for url in [
            "https://gitlab.com/mroboff/zmk-config.git",
            "https://github.com/mroboff",
            "https://github.com/a/b/c",
            "",
        ] {
            assert_eq!(parse_remote(url), None, "{url}");
        }
    }

    #[test]
    fn files_are_written_and_committed_only_when_they_change() {
        let dir = std::env::temp_dir().join(format!("kc-build-repo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(Repo::open(&std::env::temp_dir().join("kc-build-missing")).is_err());

        let repo = Repo::init(&dir).unwrap();
        repo.git(&["config", "user.email", "test@example.com"])
            .unwrap();
        repo.git(&["config", "user.name", "Test"]).unwrap();
        let files = vec![("config/board.keymap".to_string(), "/ {};\n".to_string())];
        repo.write(&files).unwrap();
        assert!(repo.commit("First build").unwrap());
        let first = repo.head().unwrap();
        assert_eq!(first.len(), 40);

        repo.write(&files).unwrap();
        assert!(!repo.commit("Nothing changed").unwrap());
        assert_eq!(repo.head().unwrap(), first);

        assert!(Repo::open(&dir).is_ok());
        assert!(
            matches!(repo.github(), Err(BuildError::Git { .. })),
            "no origin yet"
        );
        repo.set_origin("https://example.com/x/y.git").unwrap();
        assert!(matches!(repo.github(), Err(BuildError::NotGitHub(_))));
        repo.set_origin("git@github.com:me/zmk-config.git").unwrap();
        assert_eq!(
            repo.github().unwrap(),
            ("me".to_string(), "zmk-config".to_string())
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
