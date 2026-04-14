use crate::{
    app::{App, AppMode},
    models::{Task, TaskStatus},
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

pub fn render(frame: &mut Frame, app: &App, tasks: &[Task]) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(frame.area());

    let title = Paragraph::new("Junie Agent Orchestrator (TUI & Git Worktrees)")
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Headless Multi-Agent Factory"),
        );
    frame.render_widget(title, chunks[0]);

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(chunks[1]);

    let items: Vec<ListItem> = tasks
        .iter()
        .enumerate()
        .map(|(index, task)| build_task_item(index, task, app.selected_task))
        .collect();

    let tasks_list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Active Agents"),
    );
    frame.render_widget(tasks_list, main_chunks[0]);

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_chunks[1]);

    let (logs_text, diff_text, status_msg) = current_task_view(app, tasks);

    let logs_panel = Paragraph::new(logs_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Agent Action & Thoughts"),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(logs_panel, right_chunks[0]);

    let diff_panel = Paragraph::new(diff_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Live Diff / File Watcher"),
        )
        .style(Style::default().fg(Color::Green))
        .wrap(Wrap { trim: true });
    frame.render_widget(diff_panel, right_chunks[1]);

    match app.mode {
        AppMode::Normal => {
            let footer = Paragraph::new(format!(
                "{status_msg} | (n) New Task | (y) Approve & Merge | (q) Quit | (Up/Down) Select"
            ))
            .block(Block::default().borders(Borders::ALL).title("Status"));
            frame.render_widget(footer, chunks[2]);
        }
        AppMode::Input => {
            let input_panel = Paragraph::new(format!("> {}", app.input))
                .style(Style::default().fg(Color::Yellow))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Enter Prompt for New Agent"),
                );
            frame.render_widget(input_panel, chunks[2]);
        }
    }
}

fn build_task_item(index: usize, task: &Task, selected_task: usize) -> ListItem<'static> {
    let prefix = match task.status {
        TaskStatus::Pending => "[P] ",
        TaskStatus::Running => "[R] ",
        TaskStatus::NeedsApproval => "[?] ",
        TaskStatus::Merged => "[M] ",
        TaskStatus::Failed => "[X] ",
    };

    let style = match task.status {
        TaskStatus::Pending => Style::default().fg(Color::DarkGray),
        TaskStatus::Running => Style::default().fg(Color::Yellow),
        TaskStatus::NeedsApproval => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
        TaskStatus::Merged => Style::default().fg(Color::Green),
        TaskStatus::Failed => Style::default().fg(Color::Red),
    };

    let mut line_style = style;
    if index == selected_task {
        line_style = line_style.add_modifier(Modifier::REVERSED);
    }

    ListItem::new(Line::from(vec![
        Span::styled(prefix, style),
        Span::styled(
            format!("Task #{} ({})", task.id, task.branch_name),
            line_style,
        ),
    ]))
}

fn current_task_view<'a>(app: &App, tasks: &'a [Task]) -> (String, String, &'a str) {
    if let Some(task) = tasks.get(app.selected_task) {
        let logs = task
            .logs
            .iter()
            .rev()
            .take(20)
            .rev()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");

        let status = match task.status {
            TaskStatus::NeedsApproval => "Agent finished. Press 'y' to merge.",
            TaskStatus::Running => "Agent is working...",
            TaskStatus::Merged => "Task merged successfully.",
            TaskStatus::Failed => "Task failed.",
            TaskStatus::Pending => "Waiting to start...",
        };

        (logs, task.diff.clone(), status)
    } else {
        ("No task selected".to_string(), String::new(), "Idle")
    }
}
