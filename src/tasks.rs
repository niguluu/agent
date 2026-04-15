use crate::{
    app::{App, Task, TaskStatus},
    git::{
        auto_merge_task, can_merge_branch, current_branch_name, ensure_agents_branch,
        existing_task_worktrees, refresh_diff, repo_root, summarize_task_result,
        task_id_from_branch, worktree_diff_text, worktree_is_dirty, AGENTS_BRANCH,
    },
    text::{
        build_agent_prompt, clean_log_line, ensure_guidelines_file, pretty_diff_output, trim_logs,
        GUIDELINES_PATH,
    },
};
use std::{process::Stdio, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::Mutex,
};

pub async fn run_agent_task(
    id: usize,
    prompt: String,
    branch_name: String,
    worktree_path: String,
    tasks_ref: Arc<Mutex<Vec<Task>>>,
) {
    let update_status = |status: TaskStatus| async {
        let mut tasks = tasks_ref.lock().await;
        if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
            task.status = status;
        }
    };

    let log = |msg: String| async {
        let mut tasks = tasks_ref.lock().await;
        if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
            task.logs.push(msg);
            trim_logs(&mut task.logs);
        }
    };

    let set_result = |msg: String| async {
        let mut tasks = tasks_ref.lock().await;
        if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
            task.result = msg;
        }
    };

    let set_merge_state = |label: String, diff: String| async {
        let mut tasks = tasks_ref.lock().await;
        if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
            task.status = TaskStatus::Merging;
            task.result = label.clone();
            task.diff = diff;
            task.logs.push(label);
            trim_logs(&mut task.logs);
        }
    };

    update_status(TaskStatus::Running).await;
    set_result("starting".to_string()).await;
    log(format!(
        "Adding worktree at {} with branch {}",
        worktree_path, branch_name
    ))
    .await;

    let repo_root = match repo_root().await {
        Ok(path) => path,
        Err(err) => {
            log(err).await;
            update_status(TaskStatus::Failed).await;
            return;
        }
    };

    if let Err(err) = ensure_agents_branch(&repo_root).await {
        log(err).await;
        update_status(TaskStatus::Failed).await;
        return;
    }

    let wt_res = Command::new("git")
        .args(["worktree", "add", "-b", &branch_name, &worktree_path, AGENTS_BRANCH])
        .output()
        .await;

    if let Ok(out) = wt_res {
        if !out.status.success() {
            log("Failed to add worktree".to_string()).await;
            log(String::from_utf8_lossy(&out.stderr).into_owned()).await;
            update_status(TaskStatus::Failed).await;
            return;
        }
    } else {
        log("Failed to execute git command".to_string()).await;
        update_status(TaskStatus::Failed).await;
        return;
    }

    let guidelines_path = format!("{}/{}", worktree_path, GUIDELINES_PATH);
    if ensure_guidelines_file(&guidelines_path).is_ok() {
        set_result("run with guide".to_string()).await;
        log(format!("Guide ready at {}", GUIDELINES_PATH)).await;
    } else {
        log("Could not write guide file".to_string()).await;
    }

    log("Spawning Junie agent...".to_string()).await;
    set_result("agent running".to_string()).await;

    let final_prompt = build_agent_prompt(&prompt, GUIDELINES_PATH);
    let child = Command::new("junie")
        .arg(&final_prompt)
        .current_dir(&worktree_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    if let Ok(mut child_proc) = child {
        let stdout = child_proc.stdout.take().unwrap();
        let stderr = child_proc.stderr.take().unwrap();

        let mut reader = BufReader::new(stdout).lines();
        let mut err_reader = BufReader::new(stderr).lines();

        let tasks_clone = Arc::clone(&tasks_ref);
        let log_loop = tokio::spawn(async move {
            while let Ok(Some(line)) = reader.next_line().await {
                let mut tasks = tasks_clone.lock().await;
                if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
                    let clean = clean_log_line(&line);
                    if clean.is_empty() {
                        continue;
                    }
                    task.logs.push(clean);
                    trim_logs(&mut task.logs);
                }
            }
        });

        let tasks_clone_err = Arc::clone(&tasks_ref);
        let err_loop = tokio::spawn(async move {
            while let Ok(Some(line)) = err_reader.next_line().await {
                let mut tasks = tasks_clone_err.lock().await;
                if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
                    let clean = clean_log_line(&line);
                    if clean.is_empty() {
                        continue;
                    }
                    task.logs.push(format!("err {}", clean));
                    trim_logs(&mut task.logs);
                }
            }
        });

        let tasks_clone2 = Arc::clone(&tasks_ref);
        let worktree_clone = worktree_path.clone();
        let diff_loop = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(800)).await;
                let tasks = tasks_clone2.lock().await;
                let status = tasks.iter().find(|t| t.id == id).map(|t| t.status.clone());
                drop(tasks);

                if status == Some(TaskStatus::Running) {
                    let diff = Command::new("git")
                        .args(["-c", "color.ui=never", "diff", "--stat", "--patch"])
                        .current_dir(&worktree_clone)
                        .output()
                        .await;

                    if let Ok(out) = diff {
                        let diff_str = pretty_diff_output(&String::from_utf8_lossy(&out.stdout));
                        let mut tasks = tasks_clone2.lock().await;
                        if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
                            task.diff = diff_str;
                        }
                    }
                } else {
                    break;
                }
            }
        });

        let status = child_proc.wait().await;
        let _ = log_loop.await;
        let _ = err_loop.await;
        let _ = diff_loop.await;

        refresh_diff(&worktree_path, id, &tasks_ref).await;

        match status {
            Ok(exit) if exit.success() => {
                let result = summarize_task_result(&worktree_path, &prompt).await;
                set_result(result.clone()).await;
                set_merge_state(
                    format!("agent done auto merge {} to {}", branch_name, AGENTS_BRANCH),
                    format!("merging {} into {}", branch_name, AGENTS_BRANCH),
                )
                .await;

                match auto_merge_task(&branch_name, &worktree_path).await {
                    Ok(summary) => {
                        let mut tasks = tasks_ref.lock().await;
                        if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
                            task.result = summary.clone();
                            task.status = TaskStatus::Merged;
                            task.diff = summary.clone();
                            task.logs.push(summary);
                            trim_logs(&mut task.logs);
                        }
                    }
                    Err(err) => {
                        let mut tasks = tasks_ref.lock().await;
                        if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
                            task.result = "auto merge failed".to_string();
                            task.status = TaskStatus::Failed;
                            task.diff = err.clone();
                            task.logs.push(err);
                            trim_logs(&mut task.logs);
                        }
                    }
                }
            }
            Ok(exit) => {
                let mut tasks = tasks_ref.lock().await;
                if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
                    task.result = format!("failed code {}", exit.code().unwrap_or(-1));
                    task.status = TaskStatus::Failed;
                    task.logs.push(task.result.clone());
                    trim_logs(&mut task.logs);
                }
            }
            Err(err) => {
                log(format!("Agent wait failed {}", err)).await;
                set_result("agent failed".to_string()).await;
                update_status(TaskStatus::Failed).await;
            }
        }
    } else {
        log("Failed to spawn Junie process".to_string()).await;
        set_result("spawn failed".to_string()).await;
        update_status(TaskStatus::Failed).await;
    }
}

pub async fn bootstrap_existing_tasks(app: Arc<Mutex<App>>) {
    let repo_root = match repo_root().await {
        Ok(path) => path,
        Err(_) => return,
    };

    let entries = existing_task_worktrees().await;
    if entries.is_empty() {
        return;
    }

    let mut recovered = Vec::new();
    let mut next_id = 1usize;

    for entry in entries {
        let id = task_id_from_branch(&entry.branch).unwrap_or(next_id);
        next_id = next_id.max(id + 1);

        let dirty = worktree_is_dirty(&entry.path).await.unwrap_or(false);
        let dirty_text = if dirty { "dirty" } else { "clean" };
        let diff = worktree_diff_text(&entry.path).await;
        let result = if dirty {
            "found old task with local changes".to_string()
        } else {
            "found old task ready to merge".to_string()
        };

        recovered.push(Task {
            id,
            prompt: format!("recovered {}", entry.branch),
            branch_name: entry.branch.clone(),
            worktree_path: entry.path.clone(),
            status: TaskStatus::Pending,
            logs: vec![
                format!("found worktree {}", entry.path),
                format!("branch {} {}", entry.branch, dirty_text),
                "old task found auto merge starts on open".to_string(),
            ],
            diff,
            result,
        });
    }

    let mut app_state = app.lock().await;
    {
        let mut tasks = app_state.tasks.lock().await;
        tasks.extend(recovered);
    }
    app_state.next_id = app_state.next_id.max(next_id);

    if let Some(current_branch) = current_branch_name(&repo_root).await {
        if current_branch != AGENTS_BRANCH {
            app_state.error_message =
                Some(format!("loaded old tasks and kept base branch {}", current_branch));
        }
    }

    drop(app_state);

    let tasks_ref = {
        let app_ref = app.lock().await;
        Arc::clone(&app_ref.tasks)
    };

    let recovered_tasks = {
        let tasks = tasks_ref.lock().await;
        tasks.clone()
    };

    for task in recovered_tasks {
        match can_merge_branch(&repo_root, &task.branch_name, AGENTS_BRANCH).await {
            Ok(()) => {}
            Err(err) => {
                let mut tasks = tasks_ref.lock().await;
                if let Some(found) = tasks.iter_mut().find(|item| item.id == task.id) {
                    found.status = TaskStatus::Failed;
                    found.result = "old task needs review".to_string();
                    found.diff = err.clone();
                    found.logs
                        .push("old task not auto merged on open".to_string());
                    found.logs.push(err);
                    found.logs
                        .push("press y to clear this old task".to_string());
                    trim_logs(&mut found.logs);
                }
                continue;
            }
        }

        let tasks_ref = Arc::clone(&tasks_ref);
        tokio::spawn(async move {
            {
                let mut tasks = tasks_ref.lock().await;
                if let Some(found) = tasks.iter_mut().find(|item| item.id == task.id) {
                    found.status = TaskStatus::Merging;
                    found.result = format!("auto merge {}", found.branch_name);
                    found.diff = format!("merging {} into {}", found.branch_name, AGENTS_BRANCH);
                    found.logs
                        .push(format!("auto merge {} to {}", found.branch_name, AGENTS_BRANCH));
                    trim_logs(&mut found.logs);
                }
            }

            let merge_result = auto_merge_task(&task.branch_name, &task.worktree_path).await;

            let mut tasks = tasks_ref.lock().await;
            if let Some(found) = tasks.iter_mut().find(|item| item.id == task.id) {
                match merge_result {
                    Ok(summary) => {
                        found.status = TaskStatus::Merged;
                        found.result = summary.clone();
                        found.diff = summary.clone();
                        found.logs.push(summary);
                    }
                    Err(err) => {
                        found.status = TaskStatus::Failed;
                        found.result = "auto merge failed".to_string();
                        found.diff = err.clone();
                        found.logs.push(err);
                    }
                }
                trim_logs(&mut found.logs);
            }
        });
    }
}