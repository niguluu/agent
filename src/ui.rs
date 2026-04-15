use crate::{
    app::{App, AppMode, Task, TaskStatus},
    text::{short_prompt, task_status_text},
};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

pub fn render(f: &mut ratatui::Frame, app: &App, tasks: &[Task]) {
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

    let title = Paragraph::new("Junie Agent Orchestrator (TUI & Git Worktrees)")
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Headless Multi-Agent Factory"),
        );
    f.render_widget(title, chunks[0]);

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(24), Constraint::Percentage(76)].as_ref())
        .split(chunks[1]);

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
        (
            "no task yet\n\npress n to add a task\npress q to quit".to_string(),
            "this panel shows live diff once a task starts".to_string(),
            "idle press n for a new task",
        )
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

    match app.mode {
        AppMode::Normal => {
            let msg = if let Some(err) = &app.error_message {
                err.clone()
            } else {
                status_msg.to_string()
            };
            let footer = Paragraph::new(format!(
                "{} | n new | y clear done | q quit | j k move",
                msg
            ))
            .block(Block::default().borders(Borders::ALL).title("Status"));
            f.render_widget(footer, chunks[2]);
        }
        AppMode::Input => {
            let input_text = format!("> {}", app.input);
            let input_panel = Paragraph::new(input_text)
                .style(Style::default().fg(Color::Yellow))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Enter Prompt for New Agent"),
                );
            f.render_widget(input_panel, chunks[2]);
        }
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