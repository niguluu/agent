use crate::models::{SharedTasks, TaskStatus};
use tokio::process::Command;

use super::store::{push_task_log, set_task_status, start_merge};

pub async fn approve_task(selected: usize, tasks_ref: SharedTasks) {
    let Some((id, branch_name, worktree_path)) = start_merge(&tasks_ref, selected).await else {
        return;
    };

    let merge_res = Command::new("git")
        .args([
            "merge",
            "--no-ff",
            "-m",
            &format!("Merge {branch_name}"),
            &branch_name,
        ])
        .output()
        .await;

    let _ = Command::new("git")
        .args(["worktree", "remove", "--force", &worktree_path])
        .output()
        .await;

    let _ = Command::new("git")
        .args(["branch", "-D", &branch_name])
        .output()
        .await;

    match merge_res {
        Ok(out) if out.status.success() => {
            set_task_status(&tasks_ref, id, TaskStatus::Merged).await;
            push_task_log(&tasks_ref, id, "Merged successfully.").await;
        }
        Ok(out) => {
            set_task_status(&tasks_ref, id, TaskStatus::Failed).await;
            push_task_log(&tasks_ref, id, "Merge failed.").await;
            push_task_log(
                &tasks_ref,
                id,
                String::from_utf8_lossy(&out.stderr).to_string(),
            )
            .await;
        }
        Err(err) => {
            set_task_status(&tasks_ref, id, TaskStatus::Failed).await;
            push_task_log(&tasks_ref, id, "Merge command failed.").await;
            push_task_log(&tasks_ref, id, err.to_string()).await;
        }
    }
}
