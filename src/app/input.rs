use crate::{
    models::Task,
    runner::{approve_task, run_agent_task},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::Arc;

use super::{App, AppMode};

pub async fn handle_key_event(app_state: &mut App, key: KeyEvent) -> bool {
    match app_state.mode {
        AppMode::Normal => handle_normal_mode(app_state, key).await,
        AppMode::Input => {
            handle_input_mode(app_state, key).await;
            false
        }
    }
}

async fn handle_normal_mode(app_state: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('q') => true,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => true,
        KeyCode::Char('i') | KeyCode::Char('n') => {
            app_state.mode = AppMode::Input;
            app_state.input.clear();
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
            tokio::spawn(async move {
                approve_task(selected, tasks_ref).await;
            });
            false
        }
        _ => false,
    }
}

async fn handle_input_mode(app_state: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter => {
            spawn_new_task(app_state).await;
            app_state.input.clear();
            app_state.mode = AppMode::Normal;
        }
        KeyCode::Char(c) => {
            app_state.input.push(c);
        }
        KeyCode::Backspace => {
            app_state.input.pop();
        }
        KeyCode::Esc => {
            app_state.mode = AppMode::Normal;
        }
        _ => {}
    }
}

async fn spawn_new_task(app_state: &mut App) {
    let prompt = app_state.input.trim().to_string();
    if prompt.is_empty() {
        return;
    }

    let id = app_state.next_id;
    app_state.next_id += 1;

    let task = Task::new(id, prompt.clone());
    let branch_name = task.branch_name.clone();
    let worktree_path = task.worktree_path.clone();

    let tasks = Arc::clone(&app_state.tasks);
    tasks.lock().await.push(task);

    let tasks_ref = Arc::clone(&app_state.tasks);
    tokio::spawn(async move {
        run_agent_task(id, prompt, branch_name, worktree_path, tasks_ref).await;
    });
}
