use crate::models::SharedTasks;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Normal,
    Input,
}

pub struct App {
    pub tasks: SharedTasks,
    pub input: String,
    pub mode: AppMode,
    pub selected_task: usize,
    pub next_id: usize,
}

impl App {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(Vec::new())),
            input: String::new(),
            mode: AppMode::Normal,
            selected_task: 0,
            next_id: 1,
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
}

#[cfg(test)]
mod tests {
    use super::App;

    #[test]
    fn next_task_wraps_to_start() {
        let mut app = App::new();
        app.selected_task = 2;

        app.next_task(3);

        assert_eq!(app.selected_task, 0);
    }

    #[test]
    fn previous_task_wraps_to_end() {
        let mut app = App::new();

        app.previous_task(3);

        assert_eq!(app.selected_task, 2);
    }

    #[test]
    fn task_selection_stays_on_empty_list() {
        let mut app = App::new();
        app.selected_task = 4;

        app.next_task(0);
        app.previous_task(0);

        assert_eq!(app.selected_task, 4);
    }
}
