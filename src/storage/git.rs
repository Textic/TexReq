use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::models::{CollectionFolder, RequestItem};
use crate::theme::badge_info;

const MAX_DIFF_PREVIEW_LINES: usize = 450;
const MAX_UNTRACKED_PREVIEW_BYTES: usize = 200 * 1024;

#[derive(Debug, Clone, Default)]
pub struct GitStatusInfo {
    pub git_available: bool,
    pub is_repo: bool,
    pub branch: String,
    pub changed_files: usize,
    pub remote_url: Option<String>,
    pub last_commit: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitChangeKind {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
}

impl GitChangeKind {
    pub fn badge(self) -> &'static str {
        match self {
            Self::Modified => "M",
            Self::Added => "A",
            Self::Deleted => "D",
            Self::Renamed => "R",
            Self::Untracked => "??",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitFileChange {
    pub repo_path: String,
    pub display_path: String,
    pub kind: GitChangeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    Meta,
    Hunk,
    Added,
    Removed,
    Context,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitDiffLine {
    pub kind: DiffLineKind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommitSummary {
    pub hash: String,
    pub summary: String,
    pub relative_time: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestDiffSummaryItem {
    pub kind: GitChangeKind,
    pub method_label: String,
    pub name: String,
    pub detail: String,
}

#[derive(Debug, Clone, Default)]
pub struct GitRepoDetails {
    pub status: GitStatusInfo,
    pub repo_root: Option<PathBuf>,
    pub has_head: bool,
    pub changes: Vec<GitFileChange>,
    pub recent_commits: Vec<GitCommitSummary>,
    pub request_changes: Vec<RequestDiffSummaryItem>,
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
    run_git_raw(dir, args).map(|s| s.trim().to_string())
}

/// Executes git without stripping leading whitespace on the first output line, which is
/// required for fixed-column outputs such as `git status --porcelain=v1`.
fn run_git_raw(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = git_cmd(dir)
        .args(args)
        .output()
        .map_err(|e| format!("Failed to run git: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if out.status.success() {
        Ok(stdout)
    } else if !stderr.is_empty() {
        Err(stderr)
    } else if !stdout.trim().is_empty() {
        Err(stdout.trim().to_string())
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
    let git_available = git_cmd(dir)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !git_available || !dir.exists() {
        return GitStatusInfo {
            git_available,
            ..Default::default()
        };
    }

    let is_repo = run_git(dir, &["rev-parse", "--is-inside-work-tree"]).is_ok_and(|s| s == "true");
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

    let mut status_args = vec!["status", "--porcelain=v1", "-uall"];
    if let Some(path) = status_path {
        status_args.extend(["--", path]);
    }
    let changed_files = run_git_raw(dir, &status_args)
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

pub fn inspect_repo_details(dir: &Path, folder_scoped: bool) -> GitRepoDetails {
    let scope = if folder_scoped { Some(".") } else { None };
    let status = inspect_repo_with_scope(dir, scope);
    if !status.git_available || !status.is_repo {
        return GitRepoDetails {
            status,
            ..Default::default()
        };
    }

    let repo_root = run_git(dir, &["rev-parse", "--show-toplevel"])
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    let prefix = if folder_scoped {
        run_git(dir, &["rev-parse", "--show-prefix"]).unwrap_or_default()
    } else {
        String::new()
    };
    let has_head = run_git(dir, &["rev-parse", "--verify", "HEAD"]).is_ok();

    let mut status_args = vec!["-c", "core.quotePath=false", "status", "--porcelain=v1", "-uall"];
    if folder_scoped {
        status_args.extend(["--", "."]);
    }
    let changes = run_git_raw(dir, &status_args)
        .map(|raw| parse_porcelain_changes(&raw, &prefix))
        .unwrap_or_default();

    let recent_commits = if has_head {
        let mut log_args = vec!["log", "-n", "5", "--pretty=format:%h%x09%s%x09%cr"];
        if folder_scoped {
            log_args.extend(["--", "."]);
        }
        let mut parsed = run_git_raw(dir, &log_args)
            .map(|raw| parse_recent_commits(&raw))
            .unwrap_or_default();
        if folder_scoped && parsed.is_empty() {
            parsed = run_git_raw(dir, &["log", "-n", "5", "--pretty=format:%h%x09%s%x09%cr"])
                .map(|raw| parse_recent_commits(&raw))
                .unwrap_or_default();
        }
        parsed
    } else {
        Vec::new()
    };

    let request_changes = match repo_root.as_deref() {
        Some(root) => collect_request_changes_for_files(root, &changes, has_head),
        None => Vec::new(),
    };

    GitRepoDetails {
        status,
        repo_root,
        has_head,
        changes,
        recent_commits,
        request_changes,
    }
}

fn parse_porcelain_changes(raw: &str, prefix: &str) -> Vec<GitFileChange> {
    let mut items = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim_end_matches('\r');
        if trimmed.len() < 4 {
            continue;
        }
        let xy = &trimmed[..2];
        let raw_path = trimmed[3..].trim();
        let path_part = match raw_path.rsplit_once(" -> ") {
            Some((_, new_path)) => new_path,
            None => raw_path,
        };
        let repo_path = path_part.trim_matches('"').to_string();
        if repo_path.is_empty() {
            continue;
        }
        let kind = if xy == "??" {
            GitChangeKind::Untracked
        } else if xy.contains('D') {
            GitChangeKind::Deleted
        } else if xy.contains('R') {
            GitChangeKind::Renamed
        } else if xy.contains('A') {
            GitChangeKind::Added
        } else {
            GitChangeKind::Modified
        };
        let display_path = if !prefix.is_empty() {
            repo_path
                .strip_prefix(prefix)
                .unwrap_or(&repo_path)
                .to_string()
        } else {
            repo_path.clone()
        };
        items.push(GitFileChange {
            repo_path,
            display_path,
            kind,
        });
    }
    items
}

fn parse_recent_commits(raw: &str) -> Vec<GitCommitSummary> {
    raw.lines()
        .filter_map(|line| {
            let line = line.trim_end_matches('\r');
            let mut parts = line.splitn(3, '\t');
            let hash = parts.next()?.trim().to_string();
            let summary = parts.next()?.trim().to_string();
            let relative_time = parts.next().unwrap_or("").trim().to_string();
            if hash.is_empty() {
                return None;
            }
            Some(GitCommitSummary {
                hash,
                summary,
                relative_time,
            })
        })
        .collect()
}

pub fn read_file_diff(
    repo_root: &Path,
    file: &GitFileChange,
    has_head: bool,
) -> Vec<GitDiffLine> {
    if file.kind != GitChangeKind::Untracked
        && has_head
        && let Ok(raw) = run_git_raw(
            repo_root,
            &[
                "-c",
                "core.quotePath=false",
                "diff",
                "--no-color",
                "HEAD",
                "--",
                &file.repo_path,
            ],
        )
    {
        let parsed = parse_unified_diff(&raw);
        if !parsed.is_empty() {
            return parsed;
        }
    }

    if !has_head
        && file.kind != GitChangeKind::Untracked
        && let Ok(raw) = run_git_raw(
            repo_root,
            &[
                "-c",
                "core.quotePath=false",
                "diff",
                "--cached",
                "--no-color",
                "--",
                &file.repo_path,
            ],
        )
    {
        let parsed = parse_unified_diff(&raw);
        if !parsed.is_empty() {
            return parsed;
        }
    }

    build_untracked_or_missing_preview(repo_root, file)
}

fn parse_unified_diff(raw: &str) -> Vec<GitDiffLine> {
    let mut out = Vec::new();
    let total_lines = raw.lines().count();
    for line in raw.lines().take(MAX_DIFF_PREVIEW_LINES) {
        let text = line.trim_end_matches('\r').to_string();
        let kind = if text.starts_with("diff --git")
            || text.starts_with("index ")
            || text.starts_with("--- ")
            || text.starts_with("+++ ")
            || text.starts_with("new file mode")
            || text.starts_with("deleted file mode")
        {
            DiffLineKind::Meta
        } else if text.starts_with("@@") {
            DiffLineKind::Hunk
        } else if text.starts_with('+') {
            DiffLineKind::Added
        } else if text.starts_with('-') {
            DiffLineKind::Removed
        } else {
            DiffLineKind::Context
        };
        out.push(GitDiffLine { kind, text });
    }
    if total_lines > MAX_DIFF_PREVIEW_LINES {
        out.push(GitDiffLine {
            kind: DiffLineKind::Meta,
            text: format!(
                "... diff preview truncated ({} more lines)",
                total_lines - MAX_DIFF_PREVIEW_LINES
            ),
        });
    }
    out
}

fn build_untracked_or_missing_preview(repo_root: &Path, file: &GitFileChange) -> Vec<GitDiffLine> {
    let full_path = repo_root.join(&file.repo_path);
    if !full_path.exists() {
        return vec![GitDiffLine {
            kind: DiffLineKind::Meta,
            text: format!("File removed: {}", file.display_path),
        }];
    }
    let Ok(bytes) = fs::read(&full_path) else {
        return vec![GitDiffLine {
            kind: DiffLineKind::Meta,
            text: format!("Unable to read {}", file.display_path),
        }];
    };
    if bytes.len() > MAX_UNTRACKED_PREVIEW_BYTES || bytes.contains(&0) {
        return vec![GitDiffLine {
            kind: DiffLineKind::Meta,
            text: format!(
                "{} ({} bytes — preview omitted for large/binary file)",
                file.display_path,
                bytes.len()
            ),
        }];
    }
    let text = String::from_utf8_lossy(&bytes);
    let total = text.lines().count();
    let mut lines = vec![
        GitDiffLine {
            kind: DiffLineKind::Meta,
            text: format!("new file: {}", file.display_path),
        },
        GitDiffLine {
            kind: DiffLineKind::Hunk,
            text: format!("@@ -0,0 +1,{total} @@"),
        },
    ];
    for line in text.lines().take(MAX_DIFF_PREVIEW_LINES) {
        lines.push(GitDiffLine {
            kind: DiffLineKind::Added,
            text: format!("+{line}"),
        });
    }
    if total > MAX_DIFF_PREVIEW_LINES {
        lines.push(GitDiffLine {
            kind: DiffLineKind::Meta,
            text: format!(
                "... preview truncated ({} more lines)",
                total - MAX_DIFF_PREVIEW_LINES
            ),
        });
    }
    lines
}

fn collect_request_changes_for_files(
    repo_root: &Path,
    changes: &[GitFileChange],
    has_head: bool,
) -> Vec<RequestDiffSummaryItem> {
    let mut summary = Vec::new();
    for file in changes {
        let file_name = Path::new(&file.repo_path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let single_folder = match file_name {
            "collection.json" => true,
            "collections.json" => false,
            _ => continue,
        };

        let before_raw = if has_head && file.kind != GitChangeKind::Untracked {
            let spec = format!("HEAD:{}", file.repo_path);
            run_git_raw(repo_root, &["show", &spec]).ok()
        } else {
            None
        };
        let after_raw = if file.kind != GitChangeKind::Deleted {
            fs::read_to_string(repo_root.join(&file.repo_path)).ok()
        } else {
            None
        };

        let before_reqs = before_raw
            .as_deref()
            .map(|raw| extract_requests_from_json(raw, single_folder))
            .unwrap_or_default();
        let after_reqs = after_raw
            .as_deref()
            .map(|raw| extract_requests_from_json(raw, single_folder))
            .unwrap_or_default();

        summary.extend(diff_request_lists(&before_reqs, &after_reqs));
    }
    summary
}

fn extract_requests_from_json(raw: &str, single_folder: bool) -> Vec<(String, RequestItem)> {
    let mut out = Vec::new();
    if single_folder {
        if let Ok(folder) = serde_json::from_str::<CollectionFolder>(raw) {
            collect_requests_recursive(&folder, "", &mut out);
        }
    } else if let Ok(folders) = serde_json::from_str::<Vec<CollectionFolder>>(raw) {
        for folder in &folders {
            collect_requests_recursive(folder, &folder.name, &mut out);
        }
    }
    out
}

fn collect_requests_recursive(
    folder: &CollectionFolder,
    prefix: &str,
    out: &mut Vec<(String, RequestItem)>,
) {
    for req in &folder.requests {
        out.push((prefix.to_string(), req.clone()));
    }
    for sub in &folder.subfolders {
        let next_prefix = if prefix.is_empty() {
            sub.name.clone()
        } else {
            format!("{prefix} / {}", sub.name)
        };
        collect_requests_recursive(sub, &next_prefix, out);
    }
}

fn diff_request_lists(
    before: &[(String, RequestItem)],
    after: &[(String, RequestItem)],
) -> Vec<RequestDiffSummaryItem> {
    let before_map: HashMap<&str, (&str, &RequestItem)> = before
        .iter()
        .map(|(scope, req)| (req.id.as_str(), (scope.as_str(), req)))
        .collect();
    let after_map: HashMap<&str, (&str, &RequestItem)> = after
        .iter()
        .map(|(scope, req)| (req.id.as_str(), (scope.as_str(), req)))
        .collect();

    let mut out = Vec::new();
    for (scope, req) in after {
        let (method_label, _) = badge_info(req.kind, &req.body);
        let display_name = if scope.is_empty() {
            req.name.clone()
        } else {
            format!("{scope} / {}", req.name)
        };
        match before_map.get(req.id.as_str()) {
            None => out.push(RequestDiffSummaryItem {
                kind: GitChangeKind::Added,
                method_label: method_label.to_string(),
                name: display_name,
                detail: if req.url.trim().is_empty() {
                    "New request".to_string()
                } else {
                    req.url.clone()
                },
            }),
            Some((_, prev)) if *prev != req => out.push(RequestDiffSummaryItem {
                kind: GitChangeKind::Modified,
                method_label: method_label.to_string(),
                name: display_name,
                detail: summarize_changed_request_fields(prev, req),
            }),
            _ => {}
        }
    }

    for (scope, req) in before {
        if !after_map.contains_key(req.id.as_str()) {
            let (method_label, _) = badge_info(req.kind, &req.body);
            let display_name = if scope.is_empty() {
                req.name.clone()
            } else {
                format!("{scope} / {}", req.name)
            };
            out.push(RequestDiffSummaryItem {
                kind: GitChangeKind::Deleted,
                method_label: method_label.to_string(),
                name: display_name,
                detail: req.url.clone(),
            });
        }
    }
    out
}

fn summarize_changed_request_fields(before: &RequestItem, after: &RequestItem) -> String {
    let mut parts = Vec::new();
    if before.name != after.name {
        parts.push("name");
    }
    if before.kind != after.kind {
        parts.push("method");
    }
    if before.url != after.url {
        parts.push("url");
    }
    if before.params != after.params {
        parts.push("params");
    }
    if before.headers != after.headers {
        parts.push("headers");
    }
    if before.auth != after.auth {
        parts.push("auth");
    }
    if before.body != after.body {
        parts.push("body");
    }
    if before.captures != after.captures {
        parts.push("captures");
    }
    if before.network != after.network {
        parts.push("network");
    }
    if parts.is_empty() {
        "updated".to_string()
    } else {
        format!("{} changed", parts.join(", "))
    }
}

pub fn discard_file_change(
    repo_root: &Path,
    file: &GitFileChange,
    has_head: bool,
) -> Result<String, String> {
    if file.kind == GitChangeKind::Untracked {
        let target = repo_root.join(&file.repo_path);
        if target.exists() {
            fs::remove_file(&target)
                .map_err(|e| format!("Failed to delete {}: {e}", file.display_path))?;
        }
        return Ok(format!("Removed untracked file {}", file.display_path));
    }
    if !has_head {
        return Err("Cannot discard tracked files before the first commit".to_string());
    }
    run_git(repo_root, &["checkout", "HEAD", "--", &file.repo_path])?;
    Ok(format!("Discarded changes in {}", file.display_path))
}

pub fn ensure_workspace_gitignore(dir: &Path) {
    let path = dir.join(".gitignore");
    if !path.exists() {
        let content =
            "# TexReq workspace exclusions\nhistory.json\nsettings.json\n.env\n*.local.*\n*.tmp\n";
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

    let mut status_args = vec!["status", "--porcelain=v1", "--"];
    status_args.extend(files_to_stage);
    let status = run_git_raw(dir, &status_args)?;
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
    use crate::models::{HttpMethod, RequestKind};

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

        let unborn = inspect_repo_details(&dir, false);
        assert!(unborn.status.is_repo);
        assert!(!unborn.has_head);
        assert!(unborn.changes.iter().any(|c| c.display_path == "collections.json"));
        let first_preview = read_file_diff(
            unborn.repo_root.as_deref().unwrap(),
            unborn.changes.iter().find(|c| c.display_path == "collections.json").unwrap(),
            unborn.has_head,
        );
        assert!(first_preview.iter().any(|l| l.kind == DiffLineKind::Added));

        let commit_out = commit_workspace(&dir, "Initial workspace").expect("commit should succeed");
        assert!(!commit_out.is_empty());

        let after = inspect_repo(&dir);
        assert!(after.is_repo);
        assert_eq!(after.changed_files, 0);
        assert!(after.last_commit.is_some());

        let subfolder = dir.join("api-col");
        fs::create_dir_all(&subfolder).unwrap();
        let req = RequestItem {
            name: "Create User".to_string(),
            kind: RequestKind::Http(HttpMethod::Post),
            url: "https://api.example.com/users".to_string(),
            ..Default::default()
        };
        let mut folder = CollectionFolder::new("API Collection");
        folder.requests.push(req);
        fs::write(
            subfolder.join("collection.json"),
            serde_json::to_string_pretty(&folder).unwrap(),
        )
        .unwrap();

        let scoped = inspect_folder_repo(&subfolder);
        assert!(scoped.is_repo);
        assert_eq!(scoped.changed_files, 1);
        commit_linked_folder(&subfolder, "Add linked folder").expect("linked commit should succeed");
        assert_eq!(inspect_folder_repo(&subfolder).changed_files, 0);

        folder.requests[0].url = "https://api.example.com/v2/users".to_string();
        let added_req = RequestItem {
            name: "Health".to_string(),
            url: "https://api.example.com/health".to_string(),
            ..Default::default()
        };
        folder.requests.push(added_req);
        fs::write(
            subfolder.join("collection.json"),
            serde_json::to_string_pretty(&folder).unwrap(),
        )
        .unwrap();

        let sub_details = inspect_repo_details(&subfolder, true);
        assert_eq!(sub_details.changes.len(), 1);
        assert_eq!(sub_details.changes[0].display_path, "collection.json");
        assert_eq!(sub_details.changes[0].kind, GitChangeKind::Modified);
        assert_eq!(sub_details.request_changes.len(), 2);
        assert!(sub_details.request_changes.iter().any(|r| {
            r.kind == GitChangeKind::Modified && r.name == "Create User" && r.detail.contains("url")
        }));
        assert!(sub_details.request_changes.iter().any(|r| {
            r.kind == GitChangeKind::Added && r.name == "Health"
        }));

        let diff_lines = read_file_diff(
            sub_details.repo_root.as_deref().unwrap(),
            &sub_details.changes[0],
            sub_details.has_head,
        );
        assert!(diff_lines.iter().any(|l| l.kind == DiffLineKind::Added && l.text.contains("/v2/users")));
        assert!(diff_lines.iter().any(|l| l.kind == DiffLineKind::Removed && l.text.contains("/users")));

        discard_file_change(
            sub_details.repo_root.as_deref().unwrap(),
            &sub_details.changes[0],
            sub_details.has_head,
        )
        .expect("discard should succeed");
        assert_eq!(inspect_folder_repo(&subfolder).changed_files, 0);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn detects_deleted_requests_and_removes_untracked_files() {
        let dir = std::env::temp_dir().join(format!("texreq_git_del_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();

        if !inspect_repo(&dir).git_available {
            let _ = fs::remove_dir_all(dir);
            return;
        }

        init_repo(&dir).expect("git init should succeed");
        run_git(&dir, &["config", "user.name", "TexReq Test"]).unwrap();
        run_git(&dir, &["config", "user.email", "test@texreq.local"]).unwrap();

        let mut folder = CollectionFolder::new("Temp Col");
        let req = RequestItem {
            name: "Old Endpoint".to_string(),
            url: "https://api.example.com/old".to_string(),
            ..Default::default()
        };
        folder.requests.push(req);
        fs::write(
            dir.join("collection.json"),
            serde_json::to_string_pretty(&folder).unwrap(),
        )
        .unwrap();
        commit_linked_folder(&dir, "Initial").expect("initial commit should succeed");

        folder.requests.clear();
        fs::write(
            dir.join("collection.json"),
            serde_json::to_string_pretty(&folder).unwrap(),
        )
        .unwrap();
        fs::write(dir.join("scratch.txt"), "temp\n").unwrap();

        let details = inspect_repo_details(&dir, true);
        assert!(details.request_changes.iter().any(|item| {
            item.kind == GitChangeKind::Deleted && item.name == "Old Endpoint"
        }));

        let untracked = details
            .changes
            .iter()
            .find(|c| c.display_path == "scratch.txt" && c.kind == GitChangeKind::Untracked)
            .cloned()
            .expect("untracked file should be listed");
        discard_file_change(details.repo_root.as_deref().unwrap(), &untracked, details.has_head)
            .expect("deleting untracked file should succeed");
        assert!(!dir.join("scratch.txt").exists());

        let _ = fs::remove_dir_all(dir);
    }
}
