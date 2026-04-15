use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq)]
pub enum TaskStatus {
    Pending,
    Running,
    Merging,
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
    pub result: String,
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
            result: "queued".to_string(),
            prompt,
        }
    }
}

pub type SharedTasks = Arc<Mutex<Vec<Task>>>;

pub const AGENTS_BRANCH: &str = "agents";

pub const GUIDELINES_PATH: &str = ".junie/AGENTS.md";
pub const GUIDELINES_TEXT: &str = "# agent rules\n\n- keep replies short\n- use short simple words\n- skip filler\n- skip heavy punctuation\n- say what changed\n- name touched files when useful\n- keep any file tree short and focused\n- prefer focused file checks over full repo tree dumps\n- never print huge file contents unless needed\n- if many files change list the key files first then count the rest\n- keep the final task result short simple and direct\n";
pub const PSEUDOCODE_PATH: &str = ".junie/psudocode.yaml";
pub const PSEUDOCODE_TEXT: &str = "version: 1\nstyle:\n  replies:\n    - keep replies short\n    - use short simple words\n    - skip filler\n    - skip heavy punctuation\n    - say what changed\n    - name touched files when useful\nflow:\n  - read the task first\n  - make a short plan\n  - keep changes small\n  - reuse the patch pattern when it fits\n  - focus on implementation details that keep code simple\noutput:\n  final:\n    - say what changed\n    - name touched files\n    - keep the result direct\n";

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
