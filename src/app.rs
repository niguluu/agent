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

#[derive(Debug, Clone)]
pub struct WorktreeEntry {
    pub path: String,
    pub branch: String,
}

pub enum AppMode {
    Normal,
    Input,
}

pub struct App {
    pub tasks: Arc<Mutex<Vec<Task>>>,
    pub input: String,
    pub mode: AppMode,
    pub selected_task: usize,
    pub next_id: usize,
    pub error_message: Option<String>,
}

impl App {
    pub fn new() -> App {
        App {
            tasks: Arc::new(Mutex::new(Vec::new())),
            input: String::new(),
            mode: AppMode::Normal,
            selected_task: 0,
            next_id: 1,
            error_message: None,
        }
    }

    pub fn next_task(&mut self, tasks_len: usize) {
        if tasks_len == 0 {
            return;
        }
        self.selected_task = (self.selected_task + 1) % tasks_len;
    }

    pub fn previous_task(&mut self, tasks_len: usize) {
        if tasks_len == 0 {
            return;
        }
        if self.selected_task > 0 {
            self.selected_task -= 1;
        } else {
            self.selected_task = tasks_len - 1;
        }
    }

    pub fn dismiss_selected_task(&mut self, tasks: &mut Vec<Task>) -> bool {
        if self.selected_task >= tasks.len() {
            return false;
        }

        let can_dismiss = matches!(
            tasks[self.selected_task].status,
            TaskStatus::Merged | TaskStatus::Failed
        );

        if !can_dismiss {
            return false;
        }

        tasks.remove(self.selected_task);

        if tasks.is_empty() {
            self.selected_task = 0;
        } else if self.selected_task >= tasks.len() {
            self.selected_task = tasks.len() - 1;
        }

        true
    }
}