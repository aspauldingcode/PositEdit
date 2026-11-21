//! Supervised Git commands. Destructive operations require an explicit confirmation
//! and are not executed when that confirmation is absent.
use std::path::Path;
use std::process::Command;

pub const COMPONENT: &str = "ide-git";
pub const IMPLEMENTED: bool = true;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    NotInstalled,
    Ready,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitError {
    NotARepository,
    ConfirmationRequired,
    CommandFailed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusEntry {
    pub path: String,
    pub staged: char,
    pub unstaged: char,
}

pub fn status() -> Availability {
    if command_exists("git") {
        Availability::Ready
    } else {
        Availability::NotInstalled
    }
}

pub fn repository_status(repo: &Path) -> Result<Vec<StatusEntry>, GitError> {
    let output = git(repo, &["status", "--porcelain"])?;
    Ok(output
        .lines()
        .filter(|line| line.len() >= 4)
        .map(|line| StatusEntry {
            staged: line.chars().next().unwrap_or(' '),
            unstaged: line.chars().nth(1).unwrap_or(' '),
            path: line[3..].to_string(),
        })
        .collect())
}

pub fn stage(repo: &Path, path: &str) -> Result<(), GitError> {
    git(repo, &["add", "--", path]).map(|_| ())
}

pub fn diff(repo: &Path) -> Result<String, GitError> {
    git(repo, &["diff", "--"])
}

pub fn blame_line(repo: &Path, path: &str, line: u32) -> Result<String, GitError> {
    git(
        repo,
        &["blame", "-L", &format!("{line},{line}"), "--", path],
    )
}

pub fn reset_hard(repo: &Path, confirm: bool) -> Result<(), GitError> {
    if !confirm {
        return Err(GitError::ConfirmationRequired);
    }
    git(repo, &["reset", "--hard"]).map(|_| ())
}

fn git(repo: &Path, args: &[&str]) -> Result<String, GitError> {
    if !repo.join(".git").exists() {
        return Err(GitError::NotARepository);
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|_| GitError::CommandFailed)?;
    if !output.status.success() {
        return Err(GitError::CommandFailed);
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn command_exists(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn repo() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("axiom-git-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let init = Command::new("git")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .arg("-C")
            .arg(&dir)
            .args(["init", "-b", "main"])
            .output()
            .unwrap();
        assert!(
            init.status.success(),
            "git init failed\n{}{}",
            String::from_utf8_lossy(&init.stdout),
            String::from_utf8_lossy(&init.stderr)
        );
        fs::write(dir.join("note.txt"), "hello\n").unwrap();
        dir
    }

    #[test]
    fn status_stage_and_diff_use_the_repository() {
        let dir = repo();
        let before = repository_status(&dir).unwrap();
        assert!(before
            .iter()
            .any(|entry| entry.path == "note.txt" && entry.unstaged == '?'));
        stage(&dir, "note.txt").unwrap();
        let after = repository_status(&dir).unwrap();
        assert!(after
            .iter()
            .any(|entry| entry.path == "note.txt" && entry.staged == 'A'));
        fs::write(dir.join("note.txt"), "hello\nworld\n").unwrap();
        let diff = diff(&dir).unwrap();
        assert!(diff.contains("+world"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reset_without_confirmation_does_not_run() {
        let dir = repo();
        stage(&dir, "note.txt").unwrap();
        assert_eq!(reset_hard(&dir, false), Err(GitError::ConfirmationRequired));
        let entries = repository_status(&dir).unwrap();
        assert!(entries.iter().any(|entry| entry.staged == 'A'));
        assert!(dir.join(".git").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
