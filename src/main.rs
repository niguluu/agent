use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Terminal,
};
use std::{
    error::Error,
    io,
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::{mpsc, Mutex},
};

#[derive(Debug, Clone, PartialEq)]
enum TaskStatus {
    Pending,
    Running,
    NeedsApproval,
    Merged,
    Failed,
}

#[derive(Debug, Clone)]
struct Task {
    id: usize,
    prompt: String,
    branch_name: String,
    worktree_path: String,
    status: TaskStatus,
    logs: Vec<String>,
    diff: String,
}

enum AppMode {
    Normal,
    Input,
}

struct App {
    tasks: Arc<Mutex<Vec<Task>>>,
    input: String,
    mode: AppMode,
    selected_task: usize,
    next_id: usize,
    error_message: Option<String>,
}

impl App {
    fn new() -> App {
        App {
            tasks: Arc::new(Mutex::new(Vec::new())),
            input: String::new(),
            mode: AppMode::Normal,
            selected_task: 0,
            next_id: 1,
            error_message: None,
        }
    }

    fn next_task(&mut self, tasks_len: usize) {
        if tasks_len == 0 {
            return;
        }
        self.selected_task = (self.selected_task + 1) % tasks_len;
    }

    fn previous_task(&mut self, tasks_len: usize) {
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let app = Arc::new(Mutex::new(App::new()));

    // Run the app loop
    let res = run_app(&mut terminal, app.clone()).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{:?}", err);
    }

    Ok(())
}

async fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    app: Arc<Mutex<App>>,
) -> Result<(), Box<dyn Error>>
where
    <B as Backend>::Error: 'static,
{
    let tick_rate = Duration::from_millis(250);
    let mut last_tick = Instant::now();

    loop {
        let app_state = app.lock().await;
        let tasks = app_state.tasks.lock().await;
        
        terminal.draw(|f| { ui(f, &app_state, &tasks); })?;
        
        drop(tasks);
        drop(app_state);

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if crossterm::event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                let mut app_state = app.lock().await;
                
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
                            // Approve and merge task
                            let selected = app_state.selected_task;
                            let tasks_ref = Arc::clone(&app_state.tasks);
                            tokio::spawn(async move {
                                let mut tasks = tasks_ref.lock().await;
                                if selected < tasks.len() && tasks[selected].status == TaskStatus::NeedsApproval {
                                    tasks[selected].status = TaskStatus::Running;
                                    let branch_name = tasks[selected].branch_name.clone();
                                    let worktree_path = tasks[selected].worktree_path.clone();
                                    drop(tasks);
                                    
                                    // Merge logic
                                    let merge_res = Command::new("git")
                                        .args(["merge", "--no-ff", "-m", &format!("Merge {}", branch_name), &branch_name])
                                        .output()
                                        .await;
                                        
                                    // Cleanup worktree
                                    let _ = Command::new("git")
                                        .args(["worktree", "remove", "--force", &worktree_path])
                                        .output()
                                        .await;
                                        
                                    // Delete branch
                                    let _ = Command::new("git")
                                        .args(["branch", "-D", &branch_name])
                                        .output()
                                        .await;
                                        
                                    let mut tasks = tasks_ref.lock().await;
                                    if let Ok(out) = merge_res {
                                        if out.status.success() {
                                            tasks[selected].status = TaskStatus::Merged;
                                            tasks[selected].logs.push("Merged successfully.".into());
                                        } else {
                                            tasks[selected].status = TaskStatus::Failed;
                                            tasks[selected].logs.push("Merge failed.".into());
                                            tasks[selected].logs.push(String::from_utf8_lossy(&out.stderr).to_string());
                                        }
                                    }
                                }
                            });
                        }
                        _ => {}
                    },
                    AppMode::Input => match key.code {
                        KeyCode::Enter => {
                            let prompt = app_state.input.clone();
                            if !prompt.is_empty() {
                                let id = app_state.next_id;
                                app_state.next_id += 1;
                                let branch_name = format!("agent/task-{}", id);
                                let worktree_path = format!("../agent-worktree-{}", id);
                                
                                let new_task = Task {
                                    id,
                                    prompt: prompt.clone(),
                                    branch_name: branch_name.clone(),
                                    worktree_path: worktree_path.clone(),
                                    status: TaskStatus::Pending,
                                    logs: vec![format!("Queued: {}", prompt)],
                                    diff: String::new(),
                                };
                                
                                let tasks_arc = Arc::clone(&app_state.tasks);
                                tasks_arc.lock().await.push(new_task);
                                
                                // Spawn task
                                let tasks_ref = Arc::clone(&app_state.tasks);
                                tokio::spawn(async move {
                                    run_agent_task(id, prompt, branch_name, worktree_path, tasks_ref).await;
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

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }
}

async fn run_agent_task(id: usize, prompt: String, branch_name: String, worktree_path: String, tasks_ref: Arc<Mutex<Vec<Task>>>) {
    let update_status = |status: TaskStatus| async {
        let mut tasks = tasks_ref.lock().await;
        if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
            task.status = status;
        }
    };
    
    let log = |msg: String| async {
        let mut tasks = tasks_ref.lock().await;
        if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
            task.logs.push(msg);
        }
    };

    update_status(TaskStatus::Running).await;
    log(format!("Adding worktree at {} with branch {}", worktree_path, branch_name)).await;

    // Add worktree with a new branch
    let wt_res = Command::new("git")
        .args(["worktree", "add", "-b", &branch_name, &worktree_path])
        .output()
        .await;

    if let Ok(out) = wt_res {
        if !out.status.success() {
            log("Failed to add worktree".to_string()).await;
            log(String::from_utf8_lossy(&out.stderr).into_owned()).await;
            update_status(TaskStatus::Failed).await;
            return;
        }
    } else {
        log("Failed to execute git command".to_string()).await;
        update_status(TaskStatus::Failed).await;
        return;
    }

    log("Spawning Junie agent...".to_string()).await;
    
    // Spawn agent process (headless junie)
    let mut child = Command::new("junie")
        .arg(&prompt)
        .current_dir(&worktree_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    if let Ok(mut child_proc) = child {
        let stdout = child_proc.stdout.take().unwrap();
        let stderr = child_proc.stderr.take().unwrap();

        let mut reader = BufReader::new(stdout).lines();
        let mut err_reader = BufReader::new(stderr).lines();

        let tasks_clone = Arc::clone(&tasks_ref);
        let log_loop = tokio::spawn(async move {
            while let Ok(Some(line)) = reader.next_line().await {
                let mut tasks = tasks_clone.lock().await;
                if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
                    task.logs.push(line);
                    if task.logs.len() > 1000 {
                        task.logs.remove(0);
                    }
                }
            }
        });

        // Spawn periodic diff updater
        let tasks_clone2 = Arc::clone(&tasks_ref);
        let worktree_clone = worktree_path.clone();
        let diff_loop = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(2)).await;
                let mut tasks = tasks_clone2.lock().await;
                let status = tasks.iter().find(|t| t.id == id).map(|t| t.status.clone());
                drop(tasks);
                
                if status == Some(TaskStatus::Running) || status == Some(TaskStatus::NeedsApproval) {
                    let diff = Command::new("git")
                        .args(["diff"])
                        .current_dir(&worktree_clone)
                        .output()
                        .await;
                    
                    if let Ok(out) = diff {
                        let diff_str = String::from_utf8_lossy(&out.stdout).to_string();
                        let mut tasks = tasks_clone2.lock().await;
                        if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
                            task.diff = diff_str;
                        }
                    }
                } else {
                    break;
                }
            }
        });

        let status = child_proc.wait().await;
        let _ = log_loop.await;
        
        log(format!("Agent finished with status {:?}", status)).await;
        update_status(TaskStatus::NeedsApproval).await;
    } else {
        log("Failed to spawn Junie process".to_string()).await;
        update_status(TaskStatus::Failed).await;
    }
}

fn ui(f: &mut ratatui::Frame, app: &App, tasks: &[Task]) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints(
            [
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(3),
            ]
            .as_ref(),
        )
        .split(f.area());

    // Title / Instructions
    let title = Paragraph::new("Junie Agent Orchestrator (TUI & Git Worktrees)")
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::ALL).title("Headless Multi-Agent Factory"));
    f.render_widget(title, chunks[0]);

    // Main Body
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)].as_ref())
        .split(chunks[1]);

    // Left Panel: Active Agents
    let items: Vec<ListItem> = tasks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let prefix = match t.status {
                TaskStatus::Pending => "[P] ",
                TaskStatus::Running => "[R] ",
                TaskStatus::NeedsApproval => "[?] ",
                TaskStatus::Merged => "[M] ",
                TaskStatus::Failed => "[X] ",
            };
            let style = match t.status {
                TaskStatus::Pending => Style::default().fg(Color::DarkGray),
                TaskStatus::Running => Style::default().fg(Color::Yellow),
                TaskStatus::NeedsApproval => Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                TaskStatus::Merged => Style::default().fg(Color::Green),
                TaskStatus::Failed => Style::default().fg(Color::Red),
            };
            
            let mut line_style = style;
            if i == app.selected_task {
                line_style = line_style.add_modifier(Modifier::REVERSED);
            }

            ListItem::new(Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(format!("Task #{} ({})", t.id, t.branch_name), line_style),
            ]))
        })
        .collect();

    let tasks_list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Active Agents"));
    f.render_widget(tasks_list, main_chunks[0]);

    // Right Panel: Split into Logs and Diff
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(main_chunks[1]);

    let (logs_text, diff_text, status_msg) = if let Some(task) = tasks.get(app.selected_task) {
        let logs = task.logs.iter().rev().take(20).rev().cloned().collect::<Vec<_>>().join("\n");
        let status = match task.status {
            TaskStatus::NeedsApproval => "Agent finished. Press 'y' to merge.",
            TaskStatus::Running => "Agent is working...",
            TaskStatus::Merged => "Task merged successfully.",
            TaskStatus::Failed => "Task failed.",
            TaskStatus::Pending => "Waiting to start...",
        };
        (logs, task.diff.clone(), status)
    } else {
        ("No task selected".to_string(), "".to_string(), "Idle")
    };

    let logs_panel = Paragraph::new(logs_text)
        .block(Block::default().borders(Borders::ALL).title("Agent Action & Thoughts"))
        .wrap(Wrap { trim: true });
    f.render_widget(logs_panel, right_chunks[0]);

    let diff_panel = Paragraph::new(diff_text)
        .block(Block::default().borders(Borders::ALL).title("Live Diff / File Watcher"))
        .style(Style::default().fg(Color::Green))
        .wrap(Wrap { trim: true });
    f.render_widget(diff_panel, right_chunks[1]);

    // Footer: Input / Status
    match app.mode {
        AppMode::Normal => {
            let footer = Paragraph::new(format!("{} | (n) New Task | (y) Approve & Merge | (q) Quit | (Up/Down) Select", status_msg))
                .block(Block::default().borders(Borders::ALL).title("Status"));
            f.render_widget(footer, chunks[2]);
        }
        AppMode::Input => {
            let input_text = format!("> {}", app.input);
            let input_panel = Paragraph::new(input_text)
                .style(Style::default().fg(Color::Yellow))
                .block(Block::default().borders(Borders::ALL).title("Enter Prompt for New Agent"));
            f.render_widget(input_panel, chunks[2]);
            // Not setting cursor position to keep it simple, but we could
        }
    }
}
