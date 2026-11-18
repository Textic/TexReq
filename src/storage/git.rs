use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Default)]
pub struct GitStatusInfo {
    pub git_available: bool,
    pub is_repo: bool,
    pub branch: String,
    pub changed_files: usize,
    pub remote_url: Option<String>,
    pub last_commit: Option<String>,
}

fn git_cmd(dir: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(dir);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn run_git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = git_cmd(dir)
        .args(args)
        .output()
        .map_err(|e| format!("Failed to run git: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if out.status.success() {
        Ok(stdout)
    } else if !stderr.is_empty() {
        Err(stderr)
    } else if !stdout.is_empty() {
        Err(stdout)
    } else {
        Err(format!("git exited with status {}", out.status))
    }
}

pub fn inspect_repo(dir: &Path) -> GitStatusInfo {
    inspect_repo_with_scope(dir, None)
}

pub fn inspect_folder_repo(dir: &Path) -> GitStatusInfo {
    inspect_repo_with_scope(dir, Some("."))
}

fn inspect_repo_with_scope(dir: &Path, status_path: Option<&str>) -> GitStatusInfo {
    let git_available = git_cmd(dir).arg("--version").output().is_ok_and(|o| o.status.success());
    if !git_available || !dir.exists() {
        return GitStatusInfo {
            git_available,
            ..Default::default()
        };
    }

    let is_repo = run_git(dir, &["rev-parse", "--is-inside-work-tree"])
        .is_ok_and(|s| s == "true");
    if !is_repo {
        return GitStatusInfo {
            git_available: true,
            is_repo: false,
            ..Default::default()
        };
    }

    let branch = run_git(dir, &["branch", "--show-current"])
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| run_git(dir, &["rev-parse", "--short", "HEAD"]).ok())
        .unwrap_or_else(|| "main".to_string());

    let status_args: Vec<&str> = match status_path {
        Some(path) => vec!["status", "--porcelain", "--", path],
        None => vec!["status", "--porcelain"],
    };
    let changed_files = run_git(dir, &status_args)
        .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0);

    let remote_url = run_git(dir, &["remote", "get-url", "origin"])
        .ok()
        .filter(|s| !s.is_empty());

    let last_commit = run_git(dir, &["log", "-1", "--pretty=format:%h %s"])
        .ok()
        .filter(|s| !s.is_empty());

    GitStatusInfo {
        git_available: true,
        is_repo: true,
        branch,
        changed_files,
        remote_url,
        last_commit,
    }
}

pub fn ensure_workspace_gitignore(dir: &Path) {
    let path = dir.join(".gitignore");
    if !path.exists() {
        let content = "# TexReq workspace exclusions\nhistory.json\nsettings.json\n.env\n*.local.*\n*.tmp\n";
        let _ = fs::write(path, content);
    }
}

pub fn init_repo(dir: &Path) -> Result<String, String> {
    if !dir.exists() {
        fs::create_dir_all(dir).map_err(|e| format!("Failed to create folder: {e}"))?;
    }
    let msg = run_git(dir, &["init"])?;
    ensure_workspace_gitignore(dir);
    Ok(if msg.is_empty() {
        "Initialized Git repository".to_string()
    } else {
        msg
    })
}

pub fn commit_workspace(dir: &Path, message: &str) -> Result<String, String> {
    commit_paths(
        dir,
        message,
        &["collections.json", "environments.toml", ".gitignore"],
    )
}

pub fn commit_linked_folder(dir: &Path, message: &str) -> Result<String, String> {
    commit_paths(dir, message, &["collection.json", ".gitignore"])
}

fn commit_paths(dir: &Path, message: &str, candidates: &[&str]) -> Result<String, String> {
    ensure_workspace_gitignore(dir);
    let mut files_to_stage = Vec::new();
    for &candidate in candidates {
        if dir.join(candidate).exists() {
            files_to_stage.push(candidate);
        }
    }
    if files_to_stage.is_empty() {
        return Err("No TexReq collection or environment files found to commit".to_string());
    }

    let mut add_args = vec!["add", "--"];
    add_args.extend(files_to_stage.iter().copied());
    run_git(dir, &add_args)?;

    let mut status_args = vec!["status", "--porcelain", "--"];
    status_args.extend(files_to_stage);
    let status = run_git(dir, &status_args)?;
    if status.trim().is_empty() {
        return Ok("Working tree is clean (nothing to commit)".to_string());
    }

    let commit_msg = if message.trim().is_empty() {
        "Update TexReq collections"
    } else {
        message.trim()
    };
    run_git(dir, &["commit", "-m", commit_msg])
}

pub fn pull_repo(dir: &Path) -> Result<String, String> {
    run_git(dir, &["pull", "--rebase"])
}

pub fn push_repo(dir: &Path) -> Result<String, String> {
    run_git(dir, &["push"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initializes_and_commits_workspace_files() {
        let dir = std::env::temp_dir().join(format!("texreq_git_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();

        let status = inspect_repo(&dir);
        if !status.git_available {
            let _ = fs::remove_dir_all(dir);
            return;
        }
        assert!(!status.is_repo);

        init_repo(&dir).expect("git init should succeed");
        run_git(&dir, &["config", "user.name", "TexReq Test"]).unwrap();
        run_git(&dir, &["config", "user.email", "test@texreq.local"]).unwrap();
        fs::write(dir.join("collections.json"), "[]").unwrap();
        fs::write(dir.join("environments.toml"), "environments = []\n").unwrap();

        let commit_out = commit_workspace(&dir, "Initial workspace").expect("commit should succeed");
        assert!(!commit_out.is_empty());

        let after = inspect_repo(&dir);
        assert!(after.is_repo);
        assert_eq!(after.changed_files, 0);
        assert!(after.last_commit.is_some());

        let subfolder = dir.join("api-col");
        fs::create_dir_all(&subfolder).unwrap();
        fs::write(subfolder.join("collection.json"), "{}").unwrap();
        let scoped = inspect_folder_repo(&subfolder);
        assert!(scoped.is_repo);
        assert_eq!(scoped.changed_files, 1);
        commit_linked_folder(&subfolder, "Add linked folder").expect("linked commit should succeed");
        assert_eq!(inspect_folder_repo(&subfolder).changed_files, 0);

        let _ = fs::remove_dir_all(dir);
    }
}
