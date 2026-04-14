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
    collections::HashSet,
    error::Error,
    io,
    path::Path,
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::Mutex,
};

#[derive(Debug, Clone, PartialEq)]
enum TaskStatus {
    Pending,
    Running,
    Merging,
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
    result: String,
}

#[derive(Debug, Clone)]
struct WorktreeEntry {
    path: String,
    branch: String,
}

const MAX_LOG_LINES: usize = 400;

const AGENTS_BRANCH: &str = "agents";

const GUIDELINES_PATH: &str = ".junie/AGENTS.md";
const GUIDELINES_TEXT: &str = "# agent rules\n\n- keep replies short\n- use short simple words\n- skip filler\n- skip heavy punctuation\n- say what changed\n- name touched files when useful\n";

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
    bootstrap_existing_tasks(app.clone()).await;

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

    loop {
        let app_state = app.lock().await;
        let tasks = app_state.tasks.lock().await;
        
        terminal.draw(|f| { ui(f, &app_state, &tasks); })?;
        
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
                                    allocate_task_slot(requested_id).await;
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
            trim_logs(&mut task.logs);
        }
    };

    let set_result = |msg: String| async {
        let mut tasks = tasks_ref.lock().await;
        if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
            task.result = msg;
        }
    };

    let set_merge_state = |label: String, diff: String| async {
        let mut tasks = tasks_ref.lock().await;
        if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
            task.status = TaskStatus::Merging;
            task.result = label.clone();
            task.diff = diff;
            task.logs.push(label);
            trim_logs(&mut task.logs);
        }
    };

    update_status(TaskStatus::Running).await;
    set_result("starting".to_string()).await;
    log(format!("Adding worktree at {} with branch {}", worktree_path, branch_name)).await;

    let repo_root = match repo_root().await {
        Ok(path) => path,
        Err(err) => {
            log(err).await;
            update_status(TaskStatus::Failed).await;
            return;
        }
    };

    if let Err(err) = ensure_agents_branch(&repo_root).await {
        log(err).await;
        update_status(TaskStatus::Failed).await;
        return;
    }

    // Add worktree with a new branch
    let wt_res = Command::new("git")
        .args(["worktree", "add", "-b", &branch_name, &worktree_path, AGENTS_BRANCH])
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

    let guidelines_path = format!("{}/{}", worktree_path, GUIDELINES_PATH);
    if ensure_guidelines_file(&guidelines_path).is_ok() {
        set_result("run with guide".to_string()).await;
        log(format!("Guide ready at {}", GUIDELINES_PATH)).await;
    } else {
        log("Could not write guide file".to_string()).await;
    }

    log("Spawning Junie agent...".to_string()).await;
    set_result("agent running".to_string()).await;

    let final_prompt = build_agent_prompt(&prompt, GUIDELINES_PATH);
    
    // Spawn agent process (headless junie)
    let child = Command::new("junie")
        .arg(&final_prompt)
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
                    let clean = clean_log_line(&line);
                    if clean.is_empty() {
                        continue;
                    }
                    task.logs.push(clean);
                    trim_logs(&mut task.logs);
                }
            }
        });

        let tasks_clone_err = Arc::clone(&tasks_ref);
        let err_loop = tokio::spawn(async move {
            while let Ok(Some(line)) = err_reader.next_line().await {
                let mut tasks = tasks_clone_err.lock().await;
                if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
                    let clean = clean_log_line(&line);
                    if clean.is_empty() {
                        continue;
                    }
                    task.logs.push(format!("err {}", clean));
                    trim_logs(&mut task.logs);
                }
            }
        });

        // Spawn periodic diff updater
        let tasks_clone2 = Arc::clone(&tasks_ref);
        let worktree_clone = worktree_path.clone();
        let diff_loop = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(800)).await;
                let tasks = tasks_clone2.lock().await;
                let status = tasks.iter().find(|t| t.id == id).map(|t| t.status.clone());
                drop(tasks);
                
                if status == Some(TaskStatus::Running) {
                    let diff = Command::new("git")
                        .args(["-c", "color.ui=never", "diff", "--stat", "--patch"])
                        .current_dir(&worktree_clone)
                        .output()
                        .await;
                    
                    if let Ok(out) = diff {
                        let diff_str = pretty_diff_output(&String::from_utf8_lossy(&out.stdout));
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
        let _ = err_loop.await;
        let _ = diff_loop.await;

        refresh_diff(&worktree_path, id, &tasks_ref).await;

        match status {
            Ok(exit) if exit.success() => {
                let result = summarize_task_result(&worktree_path, &prompt).await;
                set_result(result.clone()).await;
                set_merge_state(
                    format!("agent done auto merge {} to {}", branch_name, AGENTS_BRANCH),
                    format!("merging {} into {}", branch_name, AGENTS_BRANCH),
                )
                .await;

                match auto_merge_task(&branch_name, &worktree_path).await {
                    Ok(summary) => {
                        let mut tasks = tasks_ref.lock().await;
                        if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
                            task.result = summary.clone();
                            task.status = TaskStatus::Merged;
                            task.diff = summary.clone();
                            task.logs.push(summary);
                            trim_logs(&mut task.logs);
                        }
                    }
                    Err(err) => {
                        let mut tasks = tasks_ref.lock().await;
                        if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
                            task.result = "auto merge failed".to_string();
                            task.status = TaskStatus::Failed;
                            task.diff = err.clone();
                            task.logs.push(err);
                            trim_logs(&mut task.logs);
                        }
                    }
                }
            }
            Ok(exit) => {
                let mut tasks = tasks_ref.lock().await;
                if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
                    task.result = format!("failed code {}", exit.code().unwrap_or(-1));
                    task.status = TaskStatus::Failed;
                    task.logs.push(task.result.clone());
                    trim_logs(&mut task.logs);
                }
            }
            Err(err) => {
                log(format!("Agent wait failed {}", err)).await;
                set_result("agent failed".to_string()).await;
                update_status(TaskStatus::Failed).await;
            }
        }
    } else {
        log("Failed to spawn Junie process".to_string()).await;
        set_result("spawn failed".to_string()).await;
        update_status(TaskStatus::Failed).await;
    }
}

async fn bootstrap_existing_tasks(app: Arc<Mutex<App>>) {
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
            app_state.error_message = Some(format!("loaded old tasks and kept base branch {}", current_branch));
        }
    }

    let tasks_ref = {
        let app_ref = app.lock().await;
        Arc::clone(&app_ref.tasks)
    };

    let recovered_tasks = {
        let tasks = tasks_ref.lock().await;
        tasks.clone()
    };

    for task in recovered_tasks {
        let tasks_ref = Arc::clone(&tasks_ref);
        tokio::spawn(async move {
            {
                let mut tasks = tasks_ref.lock().await;
                if let Some(found) = tasks.iter_mut().find(|item| item.id == task.id) {
                    found.status = TaskStatus::Merging;
                    found.result = format!("auto merge {}", found.branch_name);
                    found.diff = format!("merging {} into {}", found.branch_name, AGENTS_BRANCH);
                    found.logs.push(format!("auto merge {} to {}", found.branch_name, AGENTS_BRANCH));
                    trim_logs(&mut found.logs);
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
                trim_logs(&mut found.logs);
            }
        });
    }
}

fn trim_logs(logs: &mut Vec<String>) {
    if logs.len() > MAX_LOG_LINES {
        let overflow = logs.len() - MAX_LOG_LINES;
        logs.drain(0..overflow);
    }
}

fn build_agent_prompt(prompt: &str, guidelines_path: &str) -> String {
    format!(
        "user prompt: {}\nfollow the guidelines in {}\nkeep the final task result short simple and direct",
        prompt.trim(),
        guidelines_path
    )
}

fn ensure_guidelines_file(path: &str) -> io::Result<()> {
    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    if !Path::new(path).exists() {
        std::fs::write(path, GUIDELINES_TEXT)?;
    }

    Ok(())
}

fn clean_log_line(line: &str) -> String {
    line.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect::<String>()
        .trim()
        .to_string()
}

fn short_prompt(prompt: &str, words: usize) -> String {
    let text = prompt
        .split_whitespace()
        .take(words)
        .collect::<Vec<_>>()
        .join(" ");

    if text.is_empty() {
        "no prompt".to_string()
    } else {
        text
    }
}

fn pretty_diff_output(diff: &str) -> String {
    let cleaned = diff
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(120)
        .collect::<Vec<_>>()
        .join("\n");

    if cleaned.trim().is_empty() {
        "no diff yet".to_string()
    } else {
        cleaned
    }
}

fn task_status_text(status: &TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "waiting",
        TaskStatus::Running => "running",
        TaskStatus::Merging => "merging",
        TaskStatus::Merged => "merged",
        TaskStatus::Failed => "failed",
    }
}

async fn refresh_diff(worktree_path: &str, id: usize, tasks_ref: &Arc<Mutex<Vec<Task>>>) {
    let diff = Command::new("git")
        .args(["-c", "color.ui=never", "diff", "--stat", "--patch"])
        .current_dir(worktree_path)
        .output()
        .await;

    if let Ok(out) = diff {
        let diff_text = pretty_diff_output(&String::from_utf8_lossy(&out.stdout));
        let mut tasks = tasks_ref.lock().await;
        if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
            task.diff = diff_text;
        }
    }
}

async fn summarize_task_result(worktree_path: &str, prompt: &str) -> String {
    let files = Command::new("git")
        .args(["diff", "--name-only"])
        .current_dir(worktree_path)
        .output()
        .await;

    if let Ok(out) = files {
        let names_output = String::from_utf8_lossy(&out.stdout).into_owned();
        let names = names_output
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .take(4)
            .collect::<Vec<_>>();

        if !names.is_empty() {
            return format!("done changed {}", names.join(" "));
        }
    }

    let short_prompt = short_prompt(prompt, 6);

    if short_prompt == "no prompt" {
        "done no file change".to_string()
    } else {
        format!("done {}", short_prompt)
    }
}

async fn allocate_task_slot(start_id: usize) -> (usize, String, String) {
    let used_branches = existing_branch_names().await;
    let used_worktrees = existing_worktree_paths().await;
    let mut candidate = start_id.max(1);

    loop {
        let branch_name = format!("task-{}", candidate);
        let worktree_path = format!("../agent-worktree-{}", candidate);

        if !used_branches.contains(&branch_name) && !used_worktrees.contains(&worktree_path) {
            return (candidate, branch_name, worktree_path);
        }

        candidate += 1;
    }
}

async fn existing_task_worktrees() -> Vec<WorktreeEntry> {
    let out = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .output()
        .await;

    let mut entries = Vec::new();
    if let Ok(output) = out {
        if !output.status.success() {
            return entries;
        }

        let text = String::from_utf8_lossy(&output.stdout);
        let mut current_path: Option<String> = None;
        let mut current_branch: Option<String> = None;

        for line in text.lines() {
            if let Some(path) = line.strip_prefix("worktree ") {
                if let (Some(path), Some(branch)) = (current_path.take(), current_branch.take()) {
                    if is_task_branch(&branch) {
                        entries.push(WorktreeEntry { path, branch });
                    }
                }
                current_path = Some(path.trim().to_string());
                current_branch = None;
            } else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
                current_branch = Some(branch.trim().to_string());
            } else if line.trim().is_empty() {
                if let (Some(path), Some(branch)) = (current_path.take(), current_branch.take()) {
                    if is_task_branch(&branch) {
                        entries.push(WorktreeEntry { path, branch });
                    }
                }
            }
        }

        if let (Some(path), Some(branch)) = (current_path, current_branch) {
            if is_task_branch(&branch) {
                entries.push(WorktreeEntry { path, branch });
            }
        }
    }

    entries.sort_by_key(|entry| task_id_from_branch(&entry.branch).unwrap_or(usize::MAX));
    entries
}

fn is_task_branch(branch: &str) -> bool {
    branch.starts_with("task-") || branch.starts_with("agent/task-")
}

fn task_id_from_branch(branch: &str) -> Option<usize> {
    branch
        .rsplit('-')
        .next()
        .and_then(|value| value.parse::<usize>().ok())
}

async fn existing_branch_names() -> HashSet<String> {
    let out = Command::new("git").args(["branch", "--format=%(refname:short)"]).output().await;

    match out {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        _ => HashSet::new(),
    }
}

async fn existing_worktree_paths() -> HashSet<String> {
    let out = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .output()
        .await;

    match out {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.strip_prefix("worktree "))
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .filter_map(|path| normalize_worktree_path(path))
            .collect(),
        _ => HashSet::new(),
    }
}

fn normalize_worktree_path(path: &str) -> Option<String> {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!("../{}", name))
}

async fn repo_root() -> Result<String, String> {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .await
        .map_err(|err| format!("git root check failed {}", err))?;

    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }

    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

async fn current_branch_name(repo_root: &str) -> Option<String> {
    let out = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(repo_root)
        .output()
        .await
        .ok()?;

    if !out.status.success() {
        return None;
    }

    let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if branch.is_empty() {
        None
    } else {
        Some(branch)
    }
}

async fn pick_merge_branch(repo_root: &str) -> Result<String, String> {
    for branch in ["main", "master"] {
        let out = Command::new("git")
            .args(["rev-parse", "--verify", branch])
            .current_dir(repo_root)
            .output()
            .await
            .map_err(|err| format!("branch check failed {}", err))?;

        if out.status.success() {
            return Ok(branch.to_string());
        }
    }

    let out = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(repo_root)
        .output()
        .await
        .map_err(|err| format!("current branch check failed {}", err))?;

    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }

    let current = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if current.is_empty() {
        Err("could not find local main branch".to_string())
    } else {
        Ok(current)
    }
}

async fn ensure_agents_branch(repo_root: &str) -> Result<(), String> {
    let exists = Command::new("git")
        .args(["rev-parse", "--verify", AGENTS_BRANCH])
        .current_dir(repo_root)
        .output()
        .await
        .map_err(|err| format!("{} branch check failed {}", AGENTS_BRANCH, err))?;

    if exists.status.success() {
        return Ok(());
    }

    let base_branch = pick_merge_branch(repo_root).await?;
    let create = Command::new("git")
        .args(["branch", AGENTS_BRANCH, &base_branch])
        .current_dir(repo_root)
        .output()
        .await
        .map_err(|err| format!("{} branch create failed {}", AGENTS_BRANCH, err))?;

    if !create.status.success() {
        return Err(format!(
            "could not create {} {}",
            AGENTS_BRANCH,
            String::from_utf8_lossy(&create.stderr).trim()
        ));
    }

    Ok(())
}

async fn checkout_branch(repo_root: &str, branch_name: &str) -> Result<(), String> {
    let checkout = Command::new("git")
        .args(["checkout", branch_name])
        .current_dir(repo_root)
        .output()
        .await
        .map_err(|err| format!("checkout failed {}", err))?;

    if !checkout.status.success() {
        return Err(format!(
            "could not switch to {} {}",
            branch_name,
            String::from_utf8_lossy(&checkout.stderr).trim()
        ));
    }

    Ok(())
}

async fn merge_branch(repo_root: &str, source_branch: &str, target_branch: &str) -> Result<(), String> {
    checkout_branch(repo_root, target_branch).await?;

    let merge = Command::new("git")
        .args([
            "merge",
            "--no-ff",
            "-m",
            &format!("Merge {}", source_branch),
            source_branch,
        ])
        .current_dir(repo_root)
        .output()
        .await
        .map_err(|err| format!("merge command failed {}", err))?;

    if !merge.status.success() {
        return Err(format!(
            "merge to {} failed {}",
            target_branch,
            String::from_utf8_lossy(&merge.stderr).trim()
        ));
    }

    Ok(())
}

async fn merge_task_branch_to_agents(branch_name: &str, worktree_path: &str) -> Result<String, String> {
    let repo_root = repo_root().await?;
    ensure_agents_branch(&repo_root).await?;
    let commit_summary = commit_worktree_changes(worktree_path, branch_name).await?;
    merge_branch(&repo_root, branch_name, AGENTS_BRANCH).await?;

    let remove = Command::new("git")
        .args(["worktree", "remove", "--force", worktree_path])
        .current_dir(&repo_root)
        .output()
        .await
        .map_err(|err| format!("worktree remove failed {}", err))?;

    if !remove.status.success() {
        return Err(format!(
            "merged but worktree remove failed {}",
            String::from_utf8_lossy(&remove.stderr).trim()
        ));
    }

    let branch_delete = Command::new("git")
        .args(["branch", "-D", branch_name])
        .current_dir(&repo_root)
        .output()
        .await
        .map_err(|err| format!("branch delete failed {}", err))?;

    if !branch_delete.status.success() {
        return Err(format!(
            "merged but branch delete failed {}",
            String::from_utf8_lossy(&branch_delete.stderr).trim()
        ));
    }

    let worktrees = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(&repo_root)
        .output()
        .await
        .map_err(|err| format!("worktree list failed {}", err))?;

    if !worktrees.status.success() {
        return Err(format!(
            "merged but worktree check failed {}",
            String::from_utf8_lossy(&worktrees.stderr).trim()
        ));
    }

    let listed = String::from_utf8_lossy(&worktrees.stdout);
    if listed.contains(worktree_path) {
        return Err("merged but worktree still active".to_string());
    }

    if let Some(summary) = commit_summary {
        Ok(format!("{} then merged to {}", summary, AGENTS_BRANCH))
    } else {
        Ok(format!("merged to {}", AGENTS_BRANCH))
    }
}

async fn merge_agents_to_main() -> Result<String, String> {
    let repo_root = repo_root().await?;
    ensure_agents_branch(&repo_root).await?;
    let target_branch = pick_merge_branch(&repo_root).await?;
    merge_branch(&repo_root, AGENTS_BRANCH, &target_branch).await?;
    Ok(format!("merged {} to {}", AGENTS_BRANCH, target_branch))
}

async fn auto_merge_task(branch_name: &str, worktree_path: &str) -> Result<String, String> {
    let task_summary = merge_task_branch_to_agents(branch_name, worktree_path).await?;
    let main_summary = merge_agents_to_main().await?;
    Ok(format!("{} then {}", task_summary, main_summary))
}

async fn worktree_is_dirty(worktree_path: &str) -> Result<bool, String> {
    let out = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(worktree_path)
        .output()
        .await
        .map_err(|err| format!("status check failed {}", err))?;

    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }

    Ok(!String::from_utf8_lossy(&out.stdout).trim().is_empty())
}

async fn worktree_diff_text(worktree_path: &str) -> String {
    let diff = Command::new("git")
        .args(["-c", "color.ui=never", "diff", "--stat", "--patch"])
        .current_dir(worktree_path)
        .output()
        .await;

    match diff {
        Ok(out) => pretty_diff_output(&String::from_utf8_lossy(&out.stdout)),
        Err(_) => "could not load diff".to_string(),
    }
}

async fn commit_worktree_changes(worktree_path: &str, branch_name: &str) -> Result<Option<String>, String> {
    if !worktree_is_dirty(worktree_path).await? {
        return Ok(None);
    }

    let add = Command::new("git")
        .args(["add", "-A"])
        .current_dir(worktree_path)
        .output()
        .await
        .map_err(|err| format!("git add failed {}", err))?;

    if !add.status.success() {
        return Err(format!(
            "git add failed {}",
            String::from_utf8_lossy(&add.stderr).trim()
        ));
    }

    let message = format!("Save {}", branch_name);
    let commit = Command::new("git")
        .args([
            "-c",
            "user.name=Junie Agent",
            "-c",
            "user.email=junie-agent@local",
            "commit",
            "-m",
            &message,
        ])
        .current_dir(worktree_path)
        .output()
        .await
        .map_err(|err| format!("git commit failed {}", err))?;

    if !commit.status.success() {
        let stderr = String::from_utf8_lossy(&commit.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&commit.stdout).trim().to_string();
        let joined = [stdout, stderr]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ");

        if joined.contains("nothing to commit") {
            return Ok(None);
        }

        return Err(format!("git commit failed {}", joined));
    }

    let changed_files = changed_file_names(worktree_path).await;
    if changed_files.is_empty() {
        Ok(Some("saved task changes".to_string()))
    } else {
        Ok(Some(format!("saved {}", changed_files.join(" "))))
    }
}

async fn changed_file_names(worktree_path: &str) -> Vec<String> {
    let out = Command::new("git")
        .args(["show", "--pretty=", "--name-only", "HEAD"])
        .current_dir(worktree_path)
        .output()
        .await;

    match out {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .take(5)
            .map(ToOwned::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

fn auto_scroll_offset(text: &str, area_width: u16, area_height: u16) -> u16 {
    let inner_width = area_width.saturating_sub(2).max(1) as usize;
    let visible_lines = area_height.saturating_sub(2) as usize;

    if visible_lines == 0 {
        return 0;
    }

    let total_lines = text
        .lines()
        .map(|line| {
            let width = line.chars().count();
            width.max(1).div_ceil(inner_width)
        })
        .sum::<usize>()
        .max(1);

    total_lines.saturating_sub(visible_lines).min(u16::MAX as usize) as u16
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
                TaskStatus::Merging => "[>] ",
                TaskStatus::Merged => "[M] ",
                TaskStatus::Failed => "[X] ",
            };
            let style = match t.status {
                TaskStatus::Pending => Style::default().fg(Color::DarkGray),
                TaskStatus::Running => Style::default().fg(Color::Yellow),
                TaskStatus::Merging => Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
                TaskStatus::Merged => Style::default().fg(Color::Green),
                TaskStatus::Failed => Style::default().fg(Color::Red),
            };
            
            let mut line_style = style;
            if i == app.selected_task {
                line_style = line_style.add_modifier(Modifier::REVERSED);
            }

            ListItem::new(Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(
                    format!("Task #{} {}", t.id, short_prompt(&t.prompt, 4)),
                    line_style,
                ),
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
        let mut logs_lines = vec![format!("status {}", task_status_text(&task.status))];
        if !task.result.trim().is_empty() {
            logs_lines.push(format!("result {}", task.result));
        }
        logs_lines.extend(task.logs.iter().cloned());
        let logs = logs_lines.join("\n");

        let status = match task.status {
            TaskStatus::Running => "agent running",
            TaskStatus::Merging => "merge running",
            TaskStatus::Merged => "merged",
            TaskStatus::Failed => "failed",
            TaskStatus::Pending => "waiting",
        };
        (logs, task.diff.clone(), status)
    } else {
        ("no task".to_string(), "".to_string(), "idle")
    };

    let logs_scroll = auto_scroll_offset(&logs_text, right_chunks[0].width, right_chunks[0].height);
    let diff_scroll = auto_scroll_offset(&diff_text, right_chunks[1].width, right_chunks[1].height);

    let logs_panel = Paragraph::new(logs_text)
        .block(Block::default().borders(Borders::ALL).title("Task Logs"))
        .scroll((logs_scroll, 0))
        .wrap(Wrap { trim: true });
    f.render_widget(logs_panel, right_chunks[0]);

    let diff_panel = Paragraph::new(diff_text)
        .block(Block::default().borders(Borders::ALL).title("Live Diff"))
        .style(Style::default().fg(Color::Green))
        .scroll((diff_scroll, 0))
        .wrap(Wrap { trim: true });
    f.render_widget(diff_panel, right_chunks[1]);

    // Footer: Input / Status
    match app.mode {
        AppMode::Normal => {
            let msg = if let Some(err) = &app.error_message {
                err.clone()
            } else {
                status_msg.to_string()
            };
            let footer = Paragraph::new(format!("{} | n new | q quit | j k move", msg))
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
