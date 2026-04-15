use crate::{
    app::{App, AppMode},
    models::{Task, TaskStatus},
    runner::text_utils::short_prompt,
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

pub fn render(frame: &mut Frame, app: &App, tasks: &[Task]) {
    let panel_style = Style::default().fg(Color::Gray);
    let title_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let status_style = Style::default().fg(Color::White);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(frame.area());

    let title = Paragraph::new("Agent")
        .style(title_style)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(panel_style),
        );
    frame.render_widget(title, chunks[0]);

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Max(28), Constraint::Min(0)])
        .split(chunks[1]);

    let items: Vec<ListItem> = tasks
        .iter()
        .enumerate()
        .map(|(index, task)| build_task_item(index, task, app.selected_task))
        .collect();

    let tasks_list = List::new(items).style(status_style).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Tasks")
            .title_style(title_style)
            .border_style(panel_style),
    );
    frame.render_widget(tasks_list, main_chunks[0]);

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_chunks[1]);

    let (logs_text, diff_text, status_msg) = current_task_view(app, tasks);

    let logs_panel = Paragraph::new(logs_text)
        .style(status_style)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Agent Logs")
                .title_style(title_style)
                .border_style(panel_style),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(logs_panel, right_chunks[0]);

    let diff_panel = Paragraph::new(diff_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Live Diff / File Watcher")
                .title_style(title_style)
                .border_style(panel_style),
        )
        .style(Style::default().fg(Color::Green))
        .wrap(Wrap { trim: true });
    frame.render_widget(diff_panel, right_chunks[1]);

    match app.mode {
        AppMode::Normal => {
            let msg = app.error_message.as_deref().unwrap_or(status_msg);
            let footer = Paragraph::new(format!(
                "{msg} | (n) New | (y) Clear Done | (ctrl+shift+c) Copy | (q ctrl+c) Quit | (j/k) Move"
            ))
            .style(status_style)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Status")
                    .title_style(title_style)
                    .border_style(panel_style),
            );
            frame.render_widget(footer, chunks[2]);
        }
        AppMode::Input => {
            let input_panel = Paragraph::new(format!(
                "> {}\n\nPaste with ctrl+v or shift+insert. Enter sends. Esc leaves. Ctrl+c quits.",
                visible_prompt_input(&app.input, chunks[2].width)
            ))
            .style(Style::default().fg(Color::Yellow))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Prompt")
                    .title_style(title_style)
                    .border_style(panel_style),
            );
            frame.render_widget(input_panel, chunks[2]);
        }
    }
}

fn build_task_item(index: usize, task: &Task, selected_task: usize) -> ListItem<'static> {
    let prefix = match task.status {
        TaskStatus::Pending => "[P] ",
        TaskStatus::Running => "[R] ",
        TaskStatus::Merging => "[>] ",
        TaskStatus::Merged => "[M] ",
        TaskStatus::Failed => "[X] ",
    };

    let style = match task.status {
        TaskStatus::Pending => Style::default().fg(Color::DarkGray),
        TaskStatus::Running => Style::default().fg(Color::Yellow),
        TaskStatus::Merging => Style::default()
            .fg(Color::Blue)
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
        Span::styled(build_task_label(task), line_style),
    ]))
}

fn current_task_view<'a>(app: &App, tasks: &'a [Task]) -> (String, String, &'a str) {
    if let Some(task) = tasks.get(app.selected_task) {
        let mut logs_lines = vec![format!("status: {:?}", task.status)];
        if !task.result.is_empty() {
            logs_lines.push(format!("result: {}", task.result));
        }
        logs_lines.extend(
            task.logs
                .iter()
                .rev()
                .take(40)
                .rev()
                .map(|line| display_log_line(line)),
        );
        let logs = logs_lines.join("\n");

        let status = match task.status {
            TaskStatus::Running => "Agent is working...",
            TaskStatus::Merging => "Merging to agents...",
            TaskStatus::Merged => "Task merged successfully.",
            TaskStatus::Failed => "Task failed.",
            TaskStatus::Pending => "Waiting to start...",
        };

        (logs, task.diff.clone(), status)
    } else {
        ("No task selected".to_string(), String::new(), "Idle")
    }
}

fn build_task_label(task: &Task) -> String {
    format!("#{} {}", task.id, short_prompt(&task.prompt, 4))
}

fn visible_prompt_input(input: &str, area_width: u16) -> String {
    let visible_width = area_width.saturating_sub(4) as usize;
    if visible_width == 0 {
        return String::new();
    }

    let input_chars: Vec<char> = input.chars().collect();
    if input_chars.len() <= visible_width {
        return input.to_string();
    }

    input_chars[input_chars.len() - visible_width..]
        .iter()
        .collect()
}

fn display_log_line(line: &str) -> String {
    let trimmed = line.trim();
    strip_timestamp_prefix(trimmed).unwrap_or_else(|| trimmed.to_string())
}

fn strip_timestamp_prefix(line: &str) -> Option<String> {
    if let Some(rest) = strip_bracketed_timestamp(line) {
        return Some(rest);
    }

    let mut parts = line.splitn(3, ' ');
    let first = parts.next()?;
    let second = parts.next()?;
    let rest = parts.next()?;
    let second = second.trim_end_matches([',', ':']);
    if looks_like_date(first) && looks_like_time(second) {
        return Some(rest.trim_start_matches([' ', '-']).to_string());
    }

    let (prefix, rest) = line.split_once(' ')?;
    let prefix = prefix.trim_end_matches([',', ':']);
    if looks_like_time(prefix) || looks_like_iso_datetime(prefix) {
        return Some(rest.trim_start_matches([' ', '-']).to_string());
    }

    None
}

fn strip_bracketed_timestamp(line: &str) -> Option<String> {
    let stripped = line.strip_prefix('[')?;
    let (prefix, rest) = stripped.split_once(']')?;
    if looks_like_time(prefix.trim()) || looks_like_iso_datetime(prefix.trim()) {
        return Some(rest.trim_start_matches([' ', '-']).to_string());
    }

    None
}

fn looks_like_time(value: &str) -> bool {
    let value = value.trim_end_matches('Z');
    let value = value.split('.').next().unwrap_or(value);
    let parts: Vec<_> = value.split(':').collect();
    if !(parts.len() == 2 || parts.len() == 3) {
        return false;
    }

    parts
        .iter()
        .all(|part| part.len() == 2 && part.chars().all(|ch| ch.is_ascii_digit()))
}

fn looks_like_date(value: &str) -> bool {
    let parts: Vec<_> = value.split('-').collect();
    if parts.len() != 3 {
        return false;
    }

    parts[0].len() == 4
        && parts[1].len() == 2
        && parts[2].len() == 2
        && parts
            .iter()
            .all(|part| part.chars().all(|ch| ch.is_ascii_digit()))
}

fn looks_like_iso_datetime(value: &str) -> bool {
    let (date, time) = match value.split_once('T') {
        Some(parts) => parts,
        None => return false,
    };

    looks_like_date(date) && looks_like_time(time)
}

#[cfg(test)]
mod tests {
    use super::{build_task_label, display_log_line, visible_prompt_input};
    use crate::models::Task;

    #[test]
    fn long_prompt_keeps_the_end_visible() {
        let visible = visible_prompt_input("abcdefghijklmnopqrstuvwxyz", 12);

        assert_eq!(visible, "stuvwxyz");
    }

    #[test]
    fn bracketed_log_timestamps_are_removed() {
        let visible = display_log_line("[12:34:56] thinking about the fix");

        assert_eq!(visible, "thinking about the fix");
    }

    #[test]
    fn date_time_log_timestamps_are_removed() {
        let visible = display_log_line("2026-04-14 20:43:00 wrote the patch");

        assert_eq!(visible, "wrote the patch");
    }

    #[test]
    fn task_labels_use_short_prompt_text() {
        let task = Task::new(3, "remove old title and fix input scroll now".to_string());

        assert_eq!(build_task_label(&task), "#3 remove old title and");
    }
}
