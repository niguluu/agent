mod app;
<<<<<<< Updated upstream
mod models;
mod runner;
mod terminal;
mod ui;

use app::{App, run_app};
use std::{error::Error, sync::Arc};
use tokio::sync::Mutex;
use runner::bootstrap_existing_tasks;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut terminal = terminal::setup()?;
    let app = Arc::new(Mutex::new(App::new()));
    
    // Recovery of existing tasks
    bootstrap_existing_tasks(app.clone()).await;
=======
mod git;
mod tasks;
mod text;
mod ui;

use app::{App, AppMode, Task, TaskStatus};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::Backend, backend::CrosstermBackend, Terminal};
use std::{error::Error, io, sync::Arc, time::Duration};
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let app = Arc::new(Mutex::new(App::new()));
    let bootstrap_app = app.clone();
    tokio::spawn(async move {
        tasks::bootstrap_existing_tasks(bootstrap_app).await;
    });
>>>>>>> Stashed changes

    let res = run_app(&mut terminal, app).await;

    terminal::restore(&mut terminal)?;

    if let Err(err) = res {
        println!("{err:?}");
    }

    Ok(())
}
<<<<<<< Updated upstream
=======

async fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    app: Arc<Mutex<App>>,
) -> Result<(), Box<dyn Error>>
where
    <B as Backend>::Error: 'static,
{
    let tick_rate = Duration::from_millis(250);

    loop {
        let app_state = app.lock().await;
        let tasks = app_state.tasks.lock().await;
        
        terminal.draw(|f| ui::render(f, &app_state, &tasks))?;
        
        drop(tasks);
        drop(app_state);

        if crossterm::event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                let mut app_state = app.lock().await;
                app_state.error_message = None;
                
                match app_state.mode {
                    AppMode::Normal => match key.code {
                        KeyCode::Char('q') => {
                            // Quit
                            return Ok(());
                        }
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            return Ok(());
                        }
                        KeyCode::Char('i') | KeyCode::Char('n') => {
                            // Enter input mode
                            app_state.mode = AppMode::Input;
                            app_state.input.clear();
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            let len = app_state.tasks.lock().await.len();
                            app_state.next_task(len);
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            let len = app_state.tasks.lock().await.len();
                            app_state.previous_task(len);
                        }
                        KeyCode::Char('y') => {
                            let selected = {
                                let tasks_arc = Arc::clone(&app_state.tasks);
                                let tasks = tasks_arc.lock().await;
                                tasks.get(app_state.selected_task).cloned()
                            };

                            if let Some(task) = selected {
                                if matches!(task.status, TaskStatus::Merged | TaskStatus::Failed) {
                                    match git::cleanup_task_refs(&task.branch_name, &task.worktree_path).await {
                                        Ok(msg) => {
                                            let tasks_arc = Arc::clone(&app_state.tasks);
                                            let mut tasks = tasks_arc.lock().await;
                                            app_state.dismiss_selected_task(&mut tasks);
                                            app_state.error_message = Some(msg);
                                        }
                                        Err(err) => {
                                            app_state.error_message = Some(err);
                                        }
                                    }
                                } else {
                                    app_state.error_message = Some("pick a merged or failed task".to_string());
                                }
                            } else {
                                app_state.error_message = Some("pick a merged or failed task".to_string());
                            }
                        }
                        _ => {}
                    },
                    AppMode::Input => match key.code {
                        KeyCode::Enter => {
                            let prompt = app_state.input.clone();
                            if prompt.trim().is_empty() {
                                let _ = std::fs::write("README.md", "add ur changes here to init the workflow");
                                app_state.error_message = Some("Created README.md. Add ur changes here to init the workflow.".to_string());
                            } else {
                                let requested_id = app_state.next_id;
                                let (id, branch_name, worktree_path) =
                                    git::allocate_task_slot(requested_id).await;
                                app_state.next_id = id + 1;
                                
                                let new_task = Task {
                                    id,
                                    prompt: prompt.clone(),
                                    branch_name: branch_name.clone(),
                                    worktree_path: worktree_path.clone(),
                                    status: TaskStatus::Pending,
                                    logs: vec![format!("Queued: {}", prompt)],
                                    diff: String::new(),
                                    result: "queued".to_string(),
                                };
                                
                                let tasks_arc = Arc::clone(&app_state.tasks);
                                tasks_arc.lock().await.push(new_task);
                                
                                // Spawn task
                                let tasks_ref = Arc::clone(&app_state.tasks);
                                tokio::spawn(async move {
                                    tasks::run_agent_task(
                                        id,
                                        prompt,
                                        branch_name,
                                        worktree_path,
                                        tasks_ref,
                                    )
                                    .await;
                                });
                            }
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
                    },
                }
            }
        }

    }
}
>>>>>>> Stashed changes
