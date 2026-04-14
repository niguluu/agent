use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
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
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    sync::Mutex,
};

const MAX_TASK_LOGS: usize = 1000;

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

fn push_log_line(task: &mut Task, line: String) {
    let Some(line) = normalize_log_line(&line) else {
        return;
    };

    task.logs.push(line);
    if task.logs.len() > MAX_TASK_LOGS {
        task.logs.remove(0);
    }
}

fn format_logs_for_panel(logs: &[String], panel_height: u16) -> String {
    let visible_lines = panel_height.saturating_sub(2) as usize;

    if visible_lines == 0 {
        return String::new();
    }

    logs.iter()
        .rev()
        .take(visible_lines)
        .rev()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalize_log_line(line: &str) -> Option<String> {
    let line = strip_ansi_sequences(line);
    let line = strip_time_prefix(&line);
    let line = line.trim();

    if line.is_empty() {
        None
    } else {
        Some(line.to_string())
    }
}

fn strip_ansi_sequences(input: &str) -> String {
    let mut cleaned = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if matches!(chars.peek(), Some('[')) {
                chars.next();
                while let Some(next) = chars.next() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            continue;
        }

        cleaned.push(ch);
    }

    cleaned
}

fn strip_time_prefix(input: &str) -> String {
    let trimmed = input.trim_start();

    if let Some(rest) = strip_bracketed_timestamp(trimmed) {
        return rest.to_string();
    }

    if let Some((token, rest)) = trimmed.split_once(char::is_whitespace) {
        if looks_like_timestamp(token) {
            return rest.trim_start_matches(|c: char| c.is_whitespace() || c == '-' || c == '|').to_string();
        }
    }

    trimmed.to_string()
}

fn strip_bracketed_timestamp(input: &str) -> Option<&str> {
    let rest = input.strip_prefix('[')?;
    let end = rest.find(']')?;
    let token = &rest[..end];

    if looks_like_timestamp(token) {
        Some(rest[end + 1..].trim_start_matches(|c: char| c.is_whitespace() || c == '-' || c == '|'))
    } else {
        None
    }
}

fn looks_like_timestamp(token: &str) -> bool {
    let token = token.trim();
    let colon_count = token.chars().filter(|ch| *ch == ':').count();
    let digit_count = token.chars().filter(|ch| ch.is_ascii_digit()).count();

    colon_count >= 1
        && digit_count >= 4
        && token
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, ':' | '.' | '-' | 'T' | 'Z' | '+' | ' '))
}

fn drain_log_lines(buffer: &mut String) -> Vec<String> {
    let mut lines = Vec::new();
    let mut start = 0;

    for (index, ch) in buffer.char_indices() {
        if ch == '\n' || ch == '\r' {
            lines.push(buffer[start..index].to_string());
            start = index + ch.len_utf8();
        }
    }

    *buffer = buffer[start..].to_string();
    lines
}

async fn append_task_log(tasks_ref: &Arc<Mutex<Vec<Task>>>, id: usize, line: impl Into<String>) {
    let mut tasks = tasks_ref.lock().await;
    if let Some(task) = tasks.iter_mut().find(|task| task.id == id) {
        push_log_line(task, line.into());
    }
}

async fn stream_task_output<R>(id: usize, mut reader: R, tasks_ref: Arc<Mutex<Vec<Task>>>)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut bytes = [0; 1024];
    let mut buffer = String::new();

    loop {
        let read = match reader.read(&mut bytes).await {
            Ok(read) => read,
            Err(err) => {
                append_task_log(&tasks_ref, id, format!("Failed to read Junie output: {err}")).await;
                return;
            }
        };

        if read == 0 {
            break;
        }

        buffer.push_str(&String::from_utf8_lossy(&bytes[..read]));

        for line in drain_log_lines(&mut buffer) {
            append_task_log(&tasks_ref, id, line).await;
        }
    }

    if !buffer.is_empty() {
        append_task_log(&tasks_ref, id, buffer).await;
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
            push_log_line(task, msg);
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
    let child = Command::new("junie")
        .arg(&prompt)
        .current_dir(&worktree_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    if let Ok(mut child_proc) = child {
        let stdout = child_proc.stdout.take().unwrap();
        let stderr = child_proc.stderr.take().unwrap();

        let stdout_loop = tokio::spawn(stream_task_output(id, stdout, Arc::clone(&tasks_ref)));
        let stderr_loop = tokio::spawn(stream_task_output(id, stderr, Arc::clone(&tasks_ref)));

        // Spawn periodic diff updater
        let tasks_clone2 = Arc::clone(&tasks_ref);
        let worktree_clone = worktree_path.clone();
        let diff_loop = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(2)).await;
                let tasks = tasks_clone2.lock().await;
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
        let _ = stdout_loop.await;
        let _ = stderr_loop.await;
        let _ = diff_loop.await;

        match status {
            Ok(exit_status) => {
                log(format!("Agent finished with status {exit_status}"))
                    .await;

                if exit_status.success() {
                    update_status(TaskStatus::NeedsApproval).await;
                } else {
                    update_status(TaskStatus::Failed).await;
                }
            }
            Err(err) => {
                log(format!("Failed while waiting for Junie: {err}")).await;
                update_status(TaskStatus::Failed).await;
            }
        }
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
        let logs = format_logs_for_panel(&task.logs, right_chunks[0].height);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_logs_for_panel_keeps_latest_lines_that_fit() {
        let logs = (1..=6).map(|n| format!("line {n}")).collect::<Vec<_>>();

        let visible = format_logs_for_panel(&logs, 5);

        assert_eq!(visible, "line 4\nline 5\nline 6");
    }

    #[test]
    fn push_log_line_removes_time_prefixes() {
        let mut task = Task {
            id: 1,
            prompt: String::new(),
            branch_name: String::new(),
            worktree_path: String::new(),
            status: TaskStatus::Running,
            logs: Vec::new(),
            diff: String::new(),
        };

        push_log_line(&mut task, "12:34:56 working on task".to_string());

        assert_eq!(task.logs, vec!["working on task"]);
    }

    #[test]
    fn drain_log_lines_handles_carriage_return_updates() {
        let mut buffer = "[12:00:00] first\r12:00:01 second\rdone".to_string();

        let drained = drain_log_lines(&mut buffer)
            .into_iter()
            .filter_map(|line| normalize_log_line(&line))
            .collect::<Vec<_>>();

        assert_eq!(drained, vec!["first", "second"]);
        assert_eq!(buffer, "done");
    }
}
