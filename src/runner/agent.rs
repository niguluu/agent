use crate::models::{SharedTasks, TaskStatus};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

use super::store::{
    get_task_status, push_task_log, push_task_output, set_task_diff, set_task_status,
};

pub async fn run_agent_task(
    id: usize,
    prompt: String,
    branch_name: String,
    worktree_path: String,
    tasks_ref: SharedTasks,
) {
    set_task_status(&tasks_ref, id, TaskStatus::Running).await;
    push_task_log(
        &tasks_ref,
        id,
        format!("Adding worktree at {worktree_path} with branch {branch_name}"),
    )
    .await;

    let worktree_add = Command::new("git")
        .args(["worktree", "add", "-b", &branch_name, &worktree_path])
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

    push_task_log(&tasks_ref, id, "Spawning Junie agent...").await;

    let child = Command::new("junie")
        .arg(&prompt)
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
            push_task_output(&stdout_tasks, id, line).await;
        }
    });

    let stderr_tasks = tasks_ref.clone();
    let stderr_loop = tokio::spawn(async move {
        while let Ok(Some(line)) = err_reader.next_line().await {
            push_task_output(&stderr_tasks, id, line).await;
        }
    });

    let diff_tasks = tasks_ref.clone();
    let diff_worktree = worktree_path.clone();
    let _diff_loop = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;

            let status = get_task_status(&diff_tasks, id).await;
            if matches!(
                status,
                Some(TaskStatus::Running) | Some(TaskStatus::NeedsApproval)
            ) {
                if let Ok(out) = Command::new("git")
                    .args(["diff"])
                    .current_dir(&diff_worktree)
                    .output()
                    .await
                {
                    let diff = String::from_utf8_lossy(&out.stdout).to_string();
                    set_task_diff(&diff_tasks, id, diff).await;
                }
            } else {
                break;
            }
        }
    });

    let status = child_proc.wait().await;
    let _ = stdout_loop.await;
    let _ = stderr_loop.await;

    push_task_log(
        &tasks_ref,
        id,
        format!("Agent finished with status {status:?}"),
    )
    .await;
    set_task_status(&tasks_ref, id, TaskStatus::NeedsApproval).await;
}
