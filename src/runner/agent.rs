use crate::models::{AGENTS_BRANCH, GUIDELINES_PATH, PSEUDOCODE_PATH, SharedTasks, TaskStatus};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

use super::git_utils::*;
use super::store::{
    push_task_log, push_task_output, set_task_diff, set_task_result, set_task_status,
};
use super::text_utils::clean_log_line;

pub async fn run_agent_task(
    id: usize,
    prompt: String,
    branch_name: String,
    worktree_path: String,
    tasks_ref: SharedTasks,
) {
    set_task_status(&tasks_ref, id, TaskStatus::Running).await;
    set_task_result(&tasks_ref, id, "starting".to_string()).await;
    push_task_log(
        &tasks_ref,
        id,
        format!("Adding worktree at {worktree_path} with branch {branch_name}"),
    )
    .await;

    let repo_root = match repo_root().await {
        Ok(path) => path,
        Err(err) => {
            push_task_log(&tasks_ref, id, err).await;
            set_task_status(&tasks_ref, id, TaskStatus::Failed).await;
            return;
        }
    };

    if let Err(err) = ensure_agents_branch(&repo_root).await {
        push_task_log(&tasks_ref, id, err).await;
        set_task_status(&tasks_ref, id, TaskStatus::Failed).await;
        return;
    }

    let worktree_add = Command::new("git")
        .args([
            "worktree",
            "add",
            "-b",
            &branch_name,
            &worktree_path,
            AGENTS_BRANCH,
        ])
        .output()
        .await;

    match worktree_add {
        Ok(out) if out.status.success() => {}
        Ok(out) => {
            push_task_log(&tasks_ref, id, "Failed to add worktree").await;
            push_task_log(
                &tasks_ref,
                id,
                String::from_utf8_lossy(&out.stderr).into_owned(),
            )
            .await;
            set_task_status(&tasks_ref, id, TaskStatus::Failed).await;
            return;
        }
        Err(err) => {
            push_task_log(&tasks_ref, id, "Failed to execute git command").await;
            push_task_log(&tasks_ref, id, err.to_string()).await;
            set_task_status(&tasks_ref, id, TaskStatus::Failed).await;
            return;
        }
    }

    let guidelines_path = format!("{}/{}", worktree_path, GUIDELINES_PATH);
    let pseudocode_path = format!("{}/{}", worktree_path, PSEUDOCODE_PATH);
    let guide_ready = ensure_guidelines_file(&guidelines_path).is_ok();
    let pseudocode_ready = ensure_pseudocode_file(&pseudocode_path).is_ok();

    if guide_ready && pseudocode_ready {
        set_task_result(&tasks_ref, id, "run with guide".to_string()).await;
        push_task_log(
            &tasks_ref,
            id,
            format!("Guide ready at {} and {}", GUIDELINES_PATH, PSEUDOCODE_PATH),
        )
        .await;
    } else {
        push_task_log(&tasks_ref, id, "Could not write session guide files".to_string()).await;
    }

    push_task_log(&tasks_ref, id, "Spawning Junie agent...").await;
    set_task_result(&tasks_ref, id, "agent running".to_string()).await;

    let final_prompt = build_agent_prompt(&prompt, GUIDELINES_PATH, PSEUDOCODE_PATH);

    let child = Command::new("junie")
        .arg(&final_prompt)
        .current_dir(&worktree_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let Ok(mut child_proc) = child else {
        push_task_log(&tasks_ref, id, "Failed to spawn Junie process").await;
        set_task_status(&tasks_ref, id, TaskStatus::Failed).await;
        return;
    };

    let Some(stdout) = child_proc.stdout.take() else {
        push_task_log(&tasks_ref, id, "Missing agent stdout pipe").await;
        set_task_status(&tasks_ref, id, TaskStatus::Failed).await;
        return;
    };

    let Some(stderr) = child_proc.stderr.take() else {
        push_task_log(&tasks_ref, id, "Missing agent stderr pipe").await;
        set_task_status(&tasks_ref, id, TaskStatus::Failed).await;
        return;
    };

    let mut reader = BufReader::new(stdout).lines();
    let mut err_reader = BufReader::new(stderr).lines();

    let stdout_tasks = tasks_ref.clone();
    let stdout_loop = tokio::spawn(async move {
        while let Ok(Some(line)) = reader.next_line().await {
            let clean = clean_log_line(&line);
            if !clean.is_empty() {
                push_task_output(&stdout_tasks, id, clean).await;
            }
        }
    });

    let stderr_tasks = tasks_ref.clone();
    let stderr_loop = tokio::spawn(async move {
        while let Ok(Some(line)) = err_reader.next_line().await {
            let clean = clean_log_line(&line);
            if !clean.is_empty() {
                push_task_output(&stderr_tasks, id, format!("err {}", clean)).await;
            }
        }
    });

    let diff_tasks = tasks_ref.clone();
    let diff_worktree = worktree_path.clone();
    let _diff_loop = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(800)).await;

            let status = crate::runner::store::get_task_status(&diff_tasks, id).await;
            if matches!(status, Some(TaskStatus::Running)) {
                refresh_diff(&diff_worktree, id, &diff_tasks).await;
            } else {
                break;
            }
        }
    });

    let status = child_proc.wait().await;
    let _ = stdout_loop.await;
    let _ = stderr_loop.await;

    refresh_diff(&worktree_path, id, &tasks_ref).await;

    match status {
        Ok(exit) if exit.success() => {
            let result = summarize_task_result(&worktree_path, &prompt).await;
            set_task_result(&tasks_ref, id, result.clone()).await;
            push_task_log(
                &tasks_ref,
                id,
                format!("agent done auto merge {} to {}", branch_name, AGENTS_BRANCH),
            )
            .await;
            set_task_status(&tasks_ref, id, TaskStatus::Merging).await;
            set_task_diff(
                &tasks_ref,
                id,
                format!("merging {} into {}", branch_name, AGENTS_BRANCH),
            )
            .await;

            match auto_merge_task(&branch_name, &worktree_path).await {
                Ok(summary) => {
                    set_task_result(&tasks_ref, id, summary.clone()).await;
                    set_task_status(&tasks_ref, id, TaskStatus::Merged).await;
                    set_task_diff(&tasks_ref, id, summary.clone()).await;
                    push_task_log(&tasks_ref, id, summary).await;
                }
                Err(err) => {
                    set_task_result(&tasks_ref, id, "auto merge failed".to_string()).await;
                    set_task_status(&tasks_ref, id, TaskStatus::Failed).await;
                    set_task_diff(&tasks_ref, id, err.clone()).await;
                    push_task_log(&tasks_ref, id, err).await;
                }
            }
        }
        Ok(exit) => {
            let code = exit.code().unwrap_or(-1);
            set_task_result(&tasks_ref, id, format!("failed code {code}")).await;
            set_task_status(&tasks_ref, id, TaskStatus::Failed).await;
            push_task_log(&tasks_ref, id, format!("failed code {code}")).await;
        }
        Err(err) => {
            push_task_log(&tasks_ref, id, format!("Agent wait failed {err}")).await;
            set_task_result(&tasks_ref, id, "agent failed".to_string()).await;
            set_task_status(&tasks_ref, id, TaskStatus::Failed).await;
        }
    }
}
