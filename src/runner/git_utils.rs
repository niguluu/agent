use super::store::set_task_diff;
use super::text_utils::{pretty_diff_output, short_prompt};
use crate::models::SharedTasks;
use crate::models::{AGENTS_BRANCH, GUIDELINES_TEXT, PSEUDOCODE_TEXT};
use std::{
    collections::HashSet,
    io,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::process::Command;

pub async fn allocate_task_slot(start_id: usize) -> (usize, String, String) {
    let used_branches = existing_branch_names().await;
    let used_worktrees = existing_worktree_paths().await;
    let mut candidate = start_id.max(1);

    loop {
        let branch_name = format!("task-{}", candidate);
        let worktree_path = format!("../agent-worktree-{}", candidate);

        if !used_branches.contains(&branch_name) && !used_worktrees.contains(&worktree_path) {
            return (candidate, branch_name, worktree_path);
        }

        candidate += 1;
    }
}

pub struct WorktreeEntry {
    pub path: String,
    pub branch: String,
}

pub async fn existing_task_worktrees() -> Vec<WorktreeEntry> {
    let Ok(entries) = existing_repo_worktrees().await else {
        return Vec::new();
    };

    let mut entries: Vec<_> = entries
        .into_iter()
        .filter(|entry| is_task_branch(&entry.branch))
        .collect();

    entries.sort_by_key(|entry| task_id_from_branch(&entry.branch).unwrap_or(usize::MAX));
    entries
}

async fn existing_repo_worktrees() -> Result<Vec<WorktreeEntry>, String> {
    let out = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .output()
        .await
        .map_err(|err| format!("worktree list failed {}", err))?;

    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }

    let text = String::from_utf8_lossy(&out.stdout);
    let mut entries = Vec::new();
    let mut current_path: Option<String> = None;
    let mut current_branch: Option<String> = None;

    for line in text.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let (Some(path), Some(branch)) = (current_path.take(), current_branch.take()) {
                entries.push(WorktreeEntry { path, branch });
            }
            current_path = Some(path.trim().to_string());
            current_branch = None;
        } else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
            current_branch = Some(branch.trim().to_string());
        } else if line.trim().is_empty() {
            if let (Some(path), Some(branch)) = (current_path.take(), current_branch.take()) {
                entries.push(WorktreeEntry { path, branch });
            }
        }
    }

    if let (Some(path), Some(branch)) = (current_path, current_branch) {
        entries.push(WorktreeEntry { path, branch });
    }

    Ok(entries)
}

fn is_task_branch(branch: &str) -> bool {
    branch.starts_with("task-") || branch.starts_with("agent/task-")
}

pub fn task_id_from_branch(branch: &str) -> Option<usize> {
    branch
        .rsplit('-')
        .next()
        .and_then(|value| value.parse::<usize>().ok())
}

async fn existing_branch_names() -> HashSet<String> {
    let out = Command::new("git")
        .args(["branch", "--format=%(refname:short)"])
        .output()
        .await;

    match out {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        _ => HashSet::new(),
    }
}

async fn existing_worktree_paths() -> HashSet<String> {
    let out = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .output()
        .await;

    match out {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.strip_prefix("worktree "))
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .filter_map(|path| normalize_worktree_path(path))
            .collect(),
        _ => HashSet::new(),
    }
}

fn normalize_worktree_path(path: &str) -> Option<String> {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!("../{}", name))
}

pub async fn repo_root() -> Result<String, String> {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .await
        .map_err(|err| format!("git root check failed {}", err))?;

    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }

    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub async fn current_branch_name(repo_root: &str) -> Option<String> {
    let out = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(repo_root)
        .output()
        .await
        .ok()?;

    if !out.status.success() {
        return None;
    }

    let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if branch.is_empty() {
        None
    } else {
        Some(branch)
    }
}

async fn pick_merge_branch(repo_root: &str) -> Result<String, String> {
    for branch in ["main", "master"] {
        let out = Command::new("git")
            .args(["rev-parse", "--verify", branch])
            .current_dir(repo_root)
            .output()
            .await
            .map_err(|err| format!("branch check failed {}", err))?;

        if out.status.success() {
            return Ok(branch.to_string());
        }
    }

    let out = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(repo_root)
        .output()
        .await
        .map_err(|err| format!("current branch check failed {}", err))?;

    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }

    let current = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if current.is_empty() {
        Err("could not find local main branch".to_string())
    } else {
        Ok(current)
    }
}

pub async fn ensure_agents_branch(repo_root: &str) -> Result<(), String> {
    let exists = Command::new("git")
        .args(["rev-parse", "--verify", AGENTS_BRANCH])
        .current_dir(repo_root)
        .output()
        .await
        .map_err(|err| format!("{} branch check failed {}", AGENTS_BRANCH, err))?;

    if exists.status.success() {
        return Ok(());
    }

    let base_branch = pick_merge_branch(repo_root).await?;
    let create = Command::new("git")
        .args(["branch", AGENTS_BRANCH, &base_branch])
        .current_dir(repo_root)
        .output()
        .await
        .map_err(|err| format!("{} branch create failed {}", AGENTS_BRANCH, err))?;

    if !create.status.success() {
        return Err(format!(
            "could not create {} {}",
            AGENTS_BRANCH,
            String::from_utf8_lossy(&create.stderr).trim()
        ));
    }

    Ok(())
}

async fn merge_branch(
    repo_root: &str,
    source_branch: &str,
    target_branch: &str,
) -> Result<(), String> {
    if let Some(path) = branch_worktree_path(target_branch).await? {
        return merge_branch_in_place(&path, source_branch, target_branch).await;
    }

    let temp_worktree = create_temp_merge_worktree(repo_root, target_branch).await?;
    let merge_result = merge_branch_in_place(&temp_worktree, source_branch, target_branch).await;
    let remove_result = remove_temp_merge_worktree(repo_root, &temp_worktree).await;

    match (merge_result, remove_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(err), _) => Err(err),
        (Ok(()), Err(err)) => Err(err),
    }
}

async fn branch_worktree_path(branch_name: &str) -> Result<Option<String>, String> {
    Ok(existing_repo_worktrees()
        .await?
        .into_iter()
        .find(|entry| entry.branch == branch_name)
        .map(|entry| entry.path))
}

async fn merge_branch_in_place(
    worktree_path: &str,
    source_branch: &str,
    target_branch: &str,
) -> Result<(), String> {
    let merge = Command::new("git")
        .args([
            "merge",
            "--no-ff",
            "-m",
            &format!("Merge {}", source_branch),
            source_branch,
        ])
        .current_dir(worktree_path)
        .output()
        .await
        .map_err(|err| format!("merge command failed {}", err))?;

    if !merge.status.success() {
        return Err(format!(
            "merge to {} failed {}",
            target_branch,
            String::from_utf8_lossy(&merge.stderr).trim()
        ));
    }

    Ok(())
}

async fn create_temp_merge_worktree(
    repo_root: &str,
    target_branch: &str,
) -> Result<String, String> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| format!("clock error {}", err))?
        .as_nanos();
    let temp_path = std::env::temp_dir().join(format!("junie-merge-{}-{}", target_branch, stamp));
    let temp_path_str = temp_path.to_string_lossy().to_string();

    let add = Command::new("git")
        .args(["worktree", "add", "--detach", &temp_path_str, target_branch])
        .current_dir(repo_root)
        .output()
        .await
        .map_err(|err| format!("temp worktree add failed {}", err))?;

    if !add.status.success() {
        return Err(format!(
            "temp worktree add failed {}",
            String::from_utf8_lossy(&add.stderr).trim()
        ));
    }

    Ok(temp_path_str)
}

async fn remove_temp_merge_worktree(repo_root: &str, worktree_path: &str) -> Result<(), String> {
    let remove = Command::new("git")
        .args(["worktree", "remove", "--force", worktree_path])
        .current_dir(repo_root)
        .output()
        .await
        .map_err(|err| format!("temp worktree remove failed {}", err))?;

    if !remove.status.success() {
        return Err(format!(
            "temp worktree remove failed {}",
            String::from_utf8_lossy(&remove.stderr).trim()
        ));
    }

    Ok(())
}

async fn merge_task_branch_to_agents(
    branch_name: &str,
    worktree_path: &str,
) -> Result<String, String> {
    let repo_root = repo_root().await?;
    ensure_agents_branch(&repo_root).await?;
    let commit_summary = commit_worktree_changes(worktree_path, branch_name).await?;
    merge_branch(&repo_root, branch_name, AGENTS_BRANCH).await?;

    let remove = Command::new("git")
        .args(["worktree", "remove", "--force", worktree_path])
        .current_dir(&repo_root)
        .output()
        .await
        .map_err(|err| format!("worktree remove failed {}", err))?;

    if !remove.status.success() {
        return Err(format!(
            "merged but worktree remove failed {}",
            String::from_utf8_lossy(&remove.stderr).trim()
        ));
    }

    let branch_delete = Command::new("git")
        .args(["branch", "-D", branch_name])
        .current_dir(&repo_root)
        .output()
        .await
        .map_err(|err| format!("branch delete failed {}", err))?;

    if !branch_delete.status.success() {
        return Err(format!(
            "merged but branch delete failed {}",
            String::from_utf8_lossy(&branch_delete.stderr).trim()
        ));
    }

    let worktrees = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(&repo_root)
        .output()
        .await
        .map_err(|err| format!("worktree list failed {}", err))?;

    if !worktrees.status.success() {
        return Err(format!(
            "merged but worktree check failed {}",
            String::from_utf8_lossy(&worktrees.stderr).trim()
        ));
    }

    let listed = String::from_utf8_lossy(&worktrees.stdout);
    if listed.contains(worktree_path) {
        return Err("merged but worktree still active".to_string());
    }

    if let Some(summary) = commit_summary {
        Ok(format!("{} then merged to {}", summary, AGENTS_BRANCH))
    } else {
        Ok(format!("merged to {}", AGENTS_BRANCH))
    }
}

#[allow(dead_code)]
async fn push_branch(repo_root: &str, branch: &str) -> Result<(), String> {
    let out = Command::new("git")
        .args(["push", "origin", branch])
        .current_dir(repo_root)
        .output()
        .await
        .map_err(|err| format!("git push failed {}", err))?;

    if !out.status.success() {
        return Err(format!(
            "push {} failed {}",
            branch,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }

    Ok(())
}

async fn merge_agents_to_main() -> Result<String, String> {
    let repo_root = repo_root().await?;
    ensure_agents_branch(&repo_root).await?;
    let target_branch = pick_merge_branch(&repo_root).await?;
    merge_branch(&repo_root, AGENTS_BRANCH, &target_branch).await?;
    Ok(format!("merged {} to {}", AGENTS_BRANCH, target_branch))
}

pub async fn auto_merge_task(branch_name: &str, worktree_path: &str) -> Result<String, String> {
    let task_summary = merge_task_branch_to_agents(branch_name, worktree_path).await?;
    let main_summary = merge_agents_to_main().await?;
    Ok(format!("{} {} (local only)", task_summary, main_summary))
}

pub async fn worktree_is_dirty(worktree_path: &str) -> Result<bool, String> {
    let out = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(worktree_path)
        .output()
        .await
        .map_err(|err| format!("status check failed {}", err))?;

    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }

    Ok(!String::from_utf8_lossy(&out.stdout).trim().is_empty())
}

pub async fn worktree_diff_text(worktree_path: &str) -> String {
    let diff = Command::new("git")
        .args(["-c", "color.ui=never", "diff", "--stat", "--patch"])
        .current_dir(worktree_path)
        .output()
        .await;

    match diff {
        Ok(out) => pretty_diff_output(&String::from_utf8_lossy(&out.stdout)),
        Err(_) => "could not load diff".to_string(),
    }
}

async fn commit_worktree_changes(
    worktree_path: &str,
    branch_name: &str,
) -> Result<Option<String>, String> {
    if !worktree_is_dirty(worktree_path).await? {
        return Ok(None);
    }

    let add = Command::new("git")
        .args(["add", "-A"])
        .current_dir(worktree_path)
        .output()
        .await
        .map_err(|err| format!("git add failed {}", err))?;

    if !add.status.success() {
        return Err(format!(
            "git add failed {}",
            String::from_utf8_lossy(&add.stderr).trim()
        ));
    }

    let message = format!("Save {}", branch_name);
    let commit = Command::new("git")
        .args([
            "-c",
            "user.name=Junie Agent",
            "-c",
            "user.email=junie-agent@local",
            "commit",
            "-m",
            &message,
        ])
        .current_dir(worktree_path)
        .output()
        .await
        .map_err(|err| format!("git commit failed {}", err))?;

    if !commit.status.success() {
        let stderr = String::from_utf8_lossy(&commit.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&commit.stdout).trim().to_string();
        let joined = [stdout, stderr]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ");

        if joined.contains("nothing to commit") {
            return Ok(None);
        }

        return Err(format!("git commit failed {}", joined));
    }

    let changed_files = changed_file_names(worktree_path).await;
    if changed_files.is_empty() {
        Ok(Some("saved task changes".to_string()))
    } else {
        Ok(Some(format!("saved {}", changed_files.join(" "))))
    }
}

async fn changed_file_names(worktree_path: &str) -> Vec<String> {
    let out = Command::new("git")
        .args(["show", "--pretty=", "--name-only", "HEAD"])
        .current_dir(worktree_path)
        .output()
        .await;

    match out {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .take(5)
            .map(ToOwned::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

pub async fn refresh_diff(worktree_path: &str, id: usize, tasks_ref: &SharedTasks) {
    let diff = Command::new("git")
        .args(["-c", "color.ui=never", "diff", "--stat", "--patch"])
        .current_dir(worktree_path)
        .output()
        .await;

    if let Ok(out) = diff {
        let diff_text = pretty_diff_output(&String::from_utf8_lossy(&out.stdout));
        set_task_diff(tasks_ref, id, diff_text).await;
    }
}

pub async fn summarize_task_result(worktree_path: &str, prompt: &str) -> String {
    let files = Command::new("git")
        .args(["diff", "--name-only"])
        .current_dir(worktree_path)
        .output()
        .await;

    if let Ok(out) = files {
        let names_output = String::from_utf8_lossy(&out.stdout).into_owned();
        let names = names_output
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .take(4)
            .collect::<Vec<_>>();

        if !names.is_empty() {
            return format!("done changed {}", names.join(" "));
        }
    }

    let short_prompt = short_prompt(prompt, 6);

    if short_prompt == "no prompt" {
        "done no file change".to_string()
    } else {
        format!("done {}", short_prompt)
    }
}

pub fn build_agent_prompt(prompt: &str, guidelines_path: &str, pseudocode_path: &str) -> String {
    format!(
        "user prompt: {}\nfollow the guidelines in {}\nread the shared flow in {}\nkeep any file tree short and focused\nprefer focused file checks over full repo tree dumps\nnever print huge file contents unless needed\nif many files change list the key files first then count the rest\nkeep the final task result short simple and direct\ncommit locally only; never run git push or push to any remote\ndo not pull or fetch from remote; the orchestrator merges everything locally",
        prompt.trim(),
        guidelines_path,
        pseudocode_path
    )
}

pub fn ensure_session_file(path: &str, text: &str) -> io::Result<()> {
    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    if !Path::new(path).exists() {
        std::fs::write(path, text)?;
    }

    Ok(())
}

pub fn ensure_guidelines_file(path: &str) -> io::Result<()> {
    ensure_session_file(path, GUIDELINES_TEXT)
}

pub fn ensure_pseudocode_file(path: &str) -> io::Result<()> {
    ensure_session_file(path, PSEUDOCODE_TEXT)
}

/// bundle a branch out of an overlay checkout and fetch it into the real repo.
/// needed because commits made inside an overlayfs mount live in the upper
/// dir and vanish on unmount unless we pull them out first.
pub async fn export_overlay_branch(
    overlay_dir: &str,
    repo_root: &str,
    branch: &str,
) -> Result<(), String> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let bundle_path = format!("{}/.git/agent-overlay-{}-{}.bundle", repo_root, branch, stamp);

    let make = Command::new("git")
        .args(["bundle", "create", &bundle_path, branch])
        .current_dir(overlay_dir)
        .output()
        .await
        .map_err(|err| format!("bundle spawn failed {}", err))?;

    if !make.status.success() {
        return Err(format!(
            "bundle create failed: {}",
            String::from_utf8_lossy(&make.stderr).trim()
        ));
    }

    let refspec = format!("{0}:{0}", branch);
    let fetch = Command::new("git")
        .args(["fetch", &bundle_path, &refspec])
        .current_dir(repo_root)
        .output()
        .await
        .map_err(|err| format!("fetch spawn failed {}", err))?;

    let _ = std::fs::remove_file(&bundle_path);

    if !fetch.status.success() {
        return Err(format!(
            "bundle fetch failed: {}",
            String::from_utf8_lossy(&fetch.stderr).trim()
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{build_agent_prompt, ensure_pseudocode_file};
    use std::{fs, time::{SystemTime, UNIX_EPOCH}};

    #[test]
    fn build_agent_prompt_keeps_file_tree_short() {
        let prompt = build_agent_prompt(
            "trim the file tree",
            ".junie/AGENTS.md",
            ".junie/psudocode.yaml",
        );

        assert!(prompt.contains("keep any file tree short and focused"));
    }

    #[test]
    fn build_agent_prompt_keeps_old_rules() {
        let prompt = build_agent_prompt(
            "trim the file tree",
            ".junie/AGENTS.md",
            ".junie/psudocode.yaml",
        );

        assert!(prompt.contains("user prompt: trim the file tree"));
        assert!(prompt.contains("follow the guidelines in .junie/AGENTS.md"));
        assert!(prompt.contains("read the shared flow in .junie/psudocode.yaml"));
        assert!(prompt.contains("keep the final task result short simple and direct"));
        assert!(prompt.contains("never run git push"));
        assert!(prompt.contains("commit locally only"));
    }

    #[test]
    fn build_agent_prompt_adds_focused_file_rules() {
        let prompt = build_agent_prompt(
            "trim the file tree",
            ".junie/AGENTS.md",
            ".junie/psudocode.yaml",
        );

        assert!(prompt.contains("prefer focused file checks over full repo tree dumps"));
        assert!(prompt.contains("never print huge file contents unless needed"));
        assert!(prompt.contains("list the key files first then count the rest"));
    }

    #[test]
    fn ensure_pseudocode_file_writes_default_text() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("junie-pseudocode-test-{unique}"));
        let path = root.join(".junie/psudocode.yaml");

        ensure_pseudocode_file(path.to_str().unwrap()).unwrap();

        let saved = fs::read_to_string(&path).unwrap();
        assert!(saved.contains("reuse the patch pattern when it fits"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn build_agent_prompt_trims_user_prompt_text() {
        let prompt = build_agent_prompt(
            "  trim the file tree  ",
            ".junie/AGENTS.md",
            ".junie/psudocode.yaml",
        );

        assert!(prompt.contains("user prompt: trim the file tree\n"));
    }
}
