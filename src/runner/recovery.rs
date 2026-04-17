use super::git_utils::*;
use crate::app::App;
use crate::models::{AGENTS_BRANCH, SharedTasks, Task, TaskStatus};
use std::sync::Arc;
use tokio::sync::Mutex;

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
            app_state.error_message = Some(format!(
                "loaded old tasks and kept base branch {}",
                current_branch
            ));
        }
    }

    let tasks_ref: SharedTasks = {
        let app_ref = app.lock().await;
        Arc::clone(&app_ref.tasks)
    };

    let recovered_tasks = {
        let tasks = tasks_ref.lock().await;
        tasks.clone()
    };

    for task in recovered_tasks {
        let tasks_ref: SharedTasks = Arc::clone(&tasks_ref);
        tokio::spawn(async move {
            {
                let mut tasks = tasks_ref.lock().await;
                if let Some(found) = tasks.iter_mut().find(|item| item.id == task.id) {
                    found.status = TaskStatus::Merging;
                    found.result = format!("auto merge {}", found.branch_name);
                    found.diff = format!("merging {} into {}", found.branch_name, AGENTS_BRANCH);
                    found.logs.push(format!(
                        "auto merge {} to {}",
                        found.branch_name, AGENTS_BRANCH
                    ));
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
            }
        });
    }
}
