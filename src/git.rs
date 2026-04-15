use crate::{app::{Task, WorktreeEntry}, text::{pretty_diff_output, short_prompt}};
use std::{collections::HashSet, path::{Path, PathBuf}, sync::{Arc, OnceLock}};
use tokio::{process::Command, sync::Mutex};

pub const AGENTS_BRANCH: &str = "agents";

fn merge_guard() -> &'static Mutex<()> {
    static MERGE_GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    MERGE_GUARD.get_or_init(|| Mutex::new(()))
}

pub async fn refresh_diff(worktree_path: &str, id: usize, tasks_ref: &Arc<Mutex<Vec<Task>>>) {
    let diff = Command::new("git")
        .args(["-c", "color.ui=never", "diff", "--stat", "--patch"])
        .current_dir(worktree_path)
        .output()
        .await;

    if let Ok(out) = diff {
        let diff_text = pretty_diff_output(&String::from_utf8_lossy(&out.stdout));
        let mut tasks = tasks_ref.lock().await;
        if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
            task.diff = diff_text;
        }
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

pub async fn existing_task_worktrees() -> Vec<WorktreeEntry> {
    let out = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .output()
        .await;

    let mut entries = Vec::new();
    if let Ok(output) = out {
        if !output.status.success() {
            return entries;
        }

        let text = String::from_utf8_lossy(&output.stdout);
        let mut current_path: Option<String> = None;
        let mut current_branch: Option<String> = None;

        for line in text.lines() {
            if let Some(path) = line.strip_prefix("worktree ") {
                if let (Some(path), Some(branch)) = (current_path.take(), current_branch.take()) {
                    if is_task_branch(&branch) {
                        entries.push(WorktreeEntry { path, branch });
                    }
                }
                current_path = Some(path.trim().to_string());
                current_branch = None;
            } else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
                current_branch = Some(branch.trim().to_string());
            } else if line.trim().is_empty() {
                if let (Some(path), Some(branch)) = (current_path.take(), current_branch.take()) {
                    if is_task_branch(&branch) {
                        entries.push(WorktreeEntry { path, branch });
                    }
                }
            }
        }

        if let (Some(path), Some(branch)) = (current_path, current_branch) {
            if is_task_branch(&branch) {
                entries.push(WorktreeEntry { path, branch });
            }
        }
    }

    entries.sort_by_key(|entry| task_id_from_branch(&entry.branch).unwrap_or(usize::MAX));
    entries
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
            .filter_map(normalize_worktree_path)
            .collect(),
        _ => HashSet::new(),
    }
}

fn temp_merge_worktree_path(repo_root: &str, target_branch: &str) -> Result<String, String> {
    let repo_name = Path::new(repo_root)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("repo");
    let safe_branch = target_branch.replace('/', "-");
    let parent = Path::new(repo_root)
        .parent()
        .ok_or_else(|| "could not find repo parent for merge worktree".to_string())?;

    Ok(parent
        .join(format!("{}-merge-{}", repo_name, safe_branch))
        .to_string_lossy()
        .into_owned())
}

fn is_task_branch(branch: &str) -> bool {
    branch.starts_with("task-") || branch.starts_with("agent/task-")
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

async fn prepare_temp_merge_worktree(repo_root: &str, target_branch: &str) -> Result<String, String> {
    let temp_path = temp_merge_worktree_path(repo_root, target_branch)?;
    let temp_path_buf = PathBuf::from(&temp_path);

    if temp_path_buf.exists() {
        let remove = Command::new("git")
            .args(["worktree", "remove", "--force", &temp_path])
            .current_dir(repo_root)
            .output()
            .await
            .map_err(|err| format!("temp worktree cleanup failed {}", err))?;

        if !remove.status.success() {
            return Err(format!(
                "temp worktree cleanup failed {}",
                String::from_utf8_lossy(&remove.stderr).trim()
            ));
        }
    }

    let add = Command::new("git")
        .args(["worktree", "add", "--detach", &temp_path, target_branch])
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

    Ok(temp_path)
}

async fn merge_branch(repo_root: &str, source_branch: &str, target_branch: &str) -> Result<(), String> {
    let target_before = Command::new("git")
        .args(["rev-parse", target_branch])
        .current_dir(repo_root)
        .output()
        .await
        .map_err(|err| format!("target rev check failed {}", err))?;

    if !target_before.status.success() {
        return Err(format!(
            "target rev check failed {}",
            String::from_utf8_lossy(&target_before.stderr).trim()
        ));
    }

    let old_target = String::from_utf8_lossy(&target_before.stdout).trim().to_string();
    let merge_dir = prepare_temp_merge_worktree(repo_root, target_branch).await?;

    let merge = Command::new("git")
        .args([
            "merge",
            "--no-ff",
            "-m",
            &format!("Merge {}", source_branch),
            source_branch,
        ])
        .current_dir(&merge_dir)
        .output()
        .await
        .map_err(|err| format!("merge command failed {}", err))?;

    if !merge.status.success() {
        let _ = Command::new("git")
            .args(["worktree", "remove", "--force", &merge_dir])
            .current_dir(repo_root)
            .output()
            .await;
        return Err(format!(
            "merge to {} failed {}",
            target_branch,
            String::from_utf8_lossy(&merge.stderr).trim()
        ));
    }

    let merged_head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&merge_dir)
        .output()
        .await
        .map_err(|err| format!("merged head check failed {}", err))?;

    if !merged_head.status.success() {
        let _ = Command::new("git")
            .args(["worktree", "remove", "--force", &merge_dir])
            .current_dir(repo_root)
            .output()
            .await;
        return Err(format!(
            "merged head check failed {}",
            String::from_utf8_lossy(&merged_head.stderr).trim()
        ));
    }

    let new_target = String::from_utf8_lossy(&merged_head.stdout).trim().to_string();
    let update = Command::new("git")
        .args([
            "update-ref",
            &format!("refs/heads/{}", target_branch),
            &new_target,
            &old_target,
        ])
        .current_dir(repo_root)
        .output()
        .await
        .map_err(|err| format!("target ref update failed {}", err))?;

    let remove = Command::new("git")
        .args(["worktree", "remove", "--force", &merge_dir])
        .current_dir(repo_root)
        .output()
        .await
        .map_err(|err| format!("temp worktree remove failed {}", err))?;

    if !update.status.success() {
        return Err(format!(
            "merge to {} failed {}",
            target_branch,
            String::from_utf8_lossy(&update.stderr).trim()
        ));
    }

    if !remove.status.success() {
        return Err(format!(
            "merge to {} worked but temp worktree remove failed {}",
            target_branch,
            String::from_utf8_lossy(&remove.stderr).trim()
        ));
    }

    Ok(())
}

pub async fn can_merge_branch(repo_root: &str, source_branch: &str, target_branch: &str) -> Result<(), String> {
    let merge_dir = prepare_temp_merge_worktree(repo_root, target_branch).await?;

    let merge = Command::new("git")
        .args(["merge", "--no-commit", "--no-ff", source_branch])
        .current_dir(&merge_dir)
        .output()
        .await
        .map_err(|err| format!("merge check failed {}", err))?;

    if merge.status.success() {
        let _ = Command::new("git")
            .args(["merge", "--abort"])
            .current_dir(&merge_dir)
            .output()
            .await;
    }

    let remove = Command::new("git")
        .args(["worktree", "remove", "--force", &merge_dir])
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

    if merge.status.success() {
        Ok(())
    } else {
        Err(format!(
            "merge to {} needs review {}",
            target_branch,
            String::from_utf8_lossy(&merge.stderr).trim()
        ))
    }
}

async fn merge_task_branch_to_agents(branch_name: &str, worktree_path: &str) -> Result<String, String> {
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

async fn merge_agents_to_main() -> Result<String, String> {
    let repo_root = repo_root().await?;
    ensure_agents_branch(&repo_root).await?;
    let target_branch = pick_merge_branch(&repo_root).await?;
    merge_branch(&repo_root, AGENTS_BRANCH, &target_branch).await?;
    Ok(format!("merged {} to {}", AGENTS_BRANCH, target_branch))
}

pub async fn auto_merge_task(branch_name: &str, worktree_path: &str) -> Result<String, String> {
    let _merge_guard = merge_guard().lock().await;
    let task_summary = merge_task_branch_to_agents(branch_name, worktree_path).await?;
    let main_summary = merge_agents_to_main().await?;
    Ok(format!("{} then {}", task_summary, main_summary))
}

pub async fn cleanup_task_refs(branch_name: &str, worktree_path: &str) -> Result<String, String> {
    let repo_root = repo_root().await?;
    let mut cleaned = Vec::new();

    let worktrees = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(&repo_root)
        .output()
        .await
        .map_err(|err| format!("worktree list failed {}", err))?;

    if !worktrees.status.success() {
        return Err(format!(
            "worktree list failed {}",
            String::from_utf8_lossy(&worktrees.stderr).trim()
        ));
    }

    let listed = String::from_utf8_lossy(&worktrees.stdout);
    if listed.lines().any(|line| line.trim() == format!("worktree {}", worktree_path)) {
        let remove = Command::new("git")
            .args(["worktree", "remove", "--force", worktree_path])
            .current_dir(&repo_root)
            .output()
            .await
            .map_err(|err| format!("worktree remove failed {}", err))?;

        if !remove.status.success() {
            return Err(format!(
                "worktree remove failed {}",
                String::from_utf8_lossy(&remove.stderr).trim()
            ));
        }

        cleaned.push("worktree");
    }

    let branch = Command::new("git")
        .args(["rev-parse", "--verify", branch_name])
        .current_dir(&repo_root)
        .output()
        .await
        .map_err(|err| format!("branch check failed {}", err))?;

    if branch.status.success() {
        let branch_delete = Command::new("git")
            .args(["branch", "-D", branch_name])
            .current_dir(&repo_root)
            .output()
            .await
            .map_err(|err| format!("branch delete failed {}", err))?;

        if !branch_delete.status.success() {
            return Err(format!(
                "branch delete failed {}",
                String::from_utf8_lossy(&branch_delete.stderr).trim()
            ));
        }

        cleaned.push("branch");
    }

    if cleaned.is_empty() {
        Ok("task already cleared".to_string())
    } else {
        Ok(format!("removed {}", cleaned.join(" and ")))
    }
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

async fn commit_worktree_changes(worktree_path: &str, branch_name: &str) -> Result<Option<String>, String> {
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