use crate::{
    app::AppMode,
    models::Task,
    runner::{git_utils::allocate_task_slot, run_agent_task},
};
use arboard::Clipboard;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::Arc;

use super::App;

pub async fn handle_key_event(app_state: &mut App, key: KeyEvent) -> bool {
    match app_state.mode {
        AppMode::Normal => handle_normal_mode(app_state, key).await,
        AppMode::Input => handle_input_mode(app_state, key).await,
    }
}

pub fn handle_paste(app_state: &mut App, text: &str) {
    if matches!(app_state.mode, AppMode::Input) {
        app_state.input.push_str(text);
    }
}

async fn handle_normal_mode(app_state: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('q') => true,
        KeyCode::Char('c' | 'C') if is_copy_shortcut(key.modifiers) => {
            copy_selected_task(app_state).await;
            false
        }
        KeyCode::Char('c' | 'C') if key.modifiers.contains(KeyModifiers::CONTROL) => true,
        KeyCode::Char('i') | KeyCode::Char('n') => {
            app_state.mode = AppMode::Input;
            app_state.input.clear();
            app_state.error_message = None;
            false
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let tasks = Arc::clone(&app_state.tasks);
            let len = tasks.lock().await.len();
            app_state.next_task(len);
            false
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let tasks = Arc::clone(&app_state.tasks);
            let len = tasks.lock().await.len();
            app_state.previous_task(len);
            false
        }
        KeyCode::Char('y') => {
            let selected = app_state.selected_task;
            let tasks_ref = Arc::clone(&app_state.tasks);
            let mut tasks = tasks_ref.lock().await;
            if selected < tasks.len() {
                let status = tasks[selected].status.clone();
                if status == crate::models::TaskStatus::Merged
                    || status == crate::models::TaskStatus::Failed
                {
                    tasks.remove(selected);
                    let new_len = tasks.len();
                    drop(tasks);
                    app_state.clamp_selection(new_len);
                }
            }
            false
        }
        _ => false,
    }
}

async fn handle_input_mode(app_state: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('c' | 'C') if key.modifiers.contains(KeyModifiers::CONTROL) => true,
        KeyCode::Char('v' | 'V') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            paste_from_clipboard(app_state);
            false
        }
        KeyCode::Insert if key.modifiers.contains(KeyModifiers::SHIFT) => {
            paste_from_clipboard(app_state);
            false
        }
        KeyCode::Enter => {
            spawn_new_task(app_state).await;
            app_state.input.clear();
            app_state.mode = AppMode::Normal;
            app_state.error_message = None;
            false
        }
        KeyCode::Char(c) => {
            app_state.input.push(c);
            app_state.error_message = None;
            false
        }
        KeyCode::Backspace => {
            app_state.input.pop();
            app_state.error_message = None;
            false
        }
        KeyCode::Esc => {
            app_state.mode = AppMode::Normal;
            app_state.error_message = None;
            false
        }
        _ => false,
    }
}

fn paste_from_clipboard(app_state: &mut App) {
    match Clipboard::new().and_then(|mut clipboard| clipboard.get_text()) {
        Ok(text) => {
            handle_paste(app_state, &text);
            app_state.error_message = Some("pasted from clipboard".to_string());
        }
        Err(err) => {
            app_state.error_message = Some(format!("paste failed: {err}"));
        }
    }
}

fn is_copy_shortcut(modifiers: KeyModifiers) -> bool {
    modifiers.contains(KeyModifiers::CONTROL) && modifiers.contains(KeyModifiers::SHIFT)
}

async fn copy_selected_task(app_state: &mut App) {
    let selected_task = {
        let tasks = app_state.tasks.lock().await;
        tasks.get(app_state.selected_task).cloned()
    };

    let Some(task) = selected_task else {
        app_state.error_message = Some("nothing to copy".to_string());
        return;
    };

    let mut text = vec![
        format!("Task #{}", task.id),
        format!("Prompt: {}", task.prompt),
        format!("Status: {:?}", task.status),
    ];
    if !task.result.is_empty() {
        text.push(format!("Result: {}", task.result));
    }
    if !task.logs.is_empty() {
        text.push(String::new());
        text.push("Logs:".to_string());
        text.extend(task.logs);
    }
    if !task.diff.trim().is_empty() {
        text.push(String::new());
        text.push("Diff:".to_string());
        text.push(task.diff);
    }

    match Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text.join("\n"))) {
        Ok(()) => app_state.error_message = Some("copied task details".to_string()),
        Err(err) => app_state.error_message = Some(format!("copy failed: {err}")),
    }
}

async fn spawn_new_task(app_state: &mut App) {
    let prompt = app_state.input.trim().to_string();
    if prompt.is_empty() {
        return;
    }

    let requested_id = app_state.next_id;
    let (id, branch_name, worktree_path) = allocate_task_slot(requested_id).await;
    app_state.next_id = id + 1;

    let mut task = Task::new(id, prompt.clone());
    task.branch_name = branch_name.clone();
    task.worktree_path = worktree_path.clone();

    let tasks = Arc::clone(&app_state.tasks);
    tasks.lock().await.push(task);

    let tasks_ref = Arc::clone(&app_state.tasks);
    tokio::spawn(async move {
        run_agent_task(id, prompt, branch_name, worktree_path, tasks_ref).await;
    });
}

#[cfg(test)]
mod tests {
    use super::{handle_paste, is_copy_shortcut};
    use crate::app::{App, AppMode};
    use crossterm::event::KeyModifiers;

    #[test]
    fn paste_appends_text_in_input_mode() {
        let mut app = App::new();
        app.mode = AppMode::Input;
        app.input = "hello".to_string();

        handle_paste(&mut app, " world");

        assert_eq!(app.input, "hello world");
    }

    #[test]
    fn paste_is_ignored_outside_input_mode() {
        let mut app = App::new();

        handle_paste(&mut app, "hello");

        assert!(app.input.is_empty());
    }

    #[test]
    fn copy_shortcut_needs_ctrl_and_shift() {
        assert!(is_copy_shortcut(KeyModifiers::CONTROL | KeyModifiers::SHIFT));
        assert!(!is_copy_shortcut(KeyModifiers::CONTROL));
    }
}
