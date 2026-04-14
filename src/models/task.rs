use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq)]
pub enum TaskStatus {
    Pending,
    Running,
    NeedsApproval,
    Merged,
    Failed,
}

#[derive(Debug, Clone)]
pub struct Task {
    pub id: usize,
    pub prompt: String,
    pub branch_name: String,
    pub worktree_path: String,
    pub status: TaskStatus,
    pub logs: Vec<String>,
    pub diff: String,
}

impl Task {
    pub fn new(id: usize, prompt: String) -> Self {
        Self {
            id,
            branch_name: format!("agent/task-{id}"),
            worktree_path: format!("../agent-worktree-{id}"),
            status: TaskStatus::Pending,
            logs: vec![format!("Queued: {prompt}")],
            diff: String::new(),
            prompt,
        }
    }
}

pub type SharedTasks = Arc<Mutex<Vec<Task>>>;

#[cfg(test)]
mod tests {
    use super::{Task, TaskStatus};

    #[test]
    fn new_task_sets_defaults() {
        let task = Task::new(7, "ship it".to_string());

        assert_eq!(task.id, 7);
        assert_eq!(task.prompt, "ship it");
        assert_eq!(task.branch_name, "agent/task-7");
        assert_eq!(task.worktree_path, "../agent-worktree-7");
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(task.logs, vec!["Queued: ship it"]);
        assert!(task.diff.is_empty());
    }
}
