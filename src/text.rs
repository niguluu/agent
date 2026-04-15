use crate::app::TaskStatus;
use std::{io, path::Path};

pub const MAX_LOG_LINES: usize = 400;
pub const GUIDELINES_PATH: &str = ".junie/AGENTS.md";
pub const GUIDELINES_TEXT: &str = "# agent rules\n\n- keep replies short\n- use short simple words\n- skip filler\n- skip heavy punctuation\n- say what changed\n- name touched files when useful\n";

pub fn trim_logs(logs: &mut Vec<String>) {
    if logs.len() > MAX_LOG_LINES {
        let overflow = logs.len() - MAX_LOG_LINES;
        logs.drain(0..overflow);
    }
}

pub fn build_agent_prompt(prompt: &str, guidelines_path: &str) -> String {
    format!(
        "user prompt: {}\nfollow the guidelines in {}\nkeep the final task result short simple and direct",
        prompt.trim(),
        guidelines_path
    )
}

pub fn ensure_guidelines_file(path: &str) -> io::Result<()> {
    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    let needs_write = match std::fs::read_to_string(path) {
        Ok(current) => current != GUIDELINES_TEXT,
        Err(_) => true,
    };

    if needs_write {
        std::fs::write(path, GUIDELINES_TEXT)?;
    }

    Ok(())
}

pub fn clean_log_line(line: &str) -> String {
    let cleaned = strip_ansi_sequences(line)
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect::<String>()
        .trim()
        .to_string();

    strip_log_timestamp(&cleaned)
}

fn strip_log_timestamp(line: &str) -> String {
    let trimmed = line.trim();

    for separator in ["] ", " - "] {
        if let Some((prefix, rest)) = trimmed.split_once(separator) {
            if looks_like_timestamp(prefix.trim_start_matches('[').trim_end_matches(']')) {
                return rest.trim().to_string();
            }
        }
    }

    if let Some(rest) = trimmed.strip_prefix("time ") {
        return rest.trim().to_string();
    }

    trimmed.to_string()
}

fn strip_ansi_sequences(line: &str) -> String {
    let mut cleaned = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if matches!(chars.peek(), Some('[')) {
                chars.next();
                while let Some(next) = chars.next() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
                continue;
            }
        }

        cleaned.push(ch);
    }

    cleaned
}

fn looks_like_timestamp(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() < 16 {
        return false;
    }

    let date_like = bytes.get(4) == Some(&b'-') && bytes.get(7) == Some(&b'-');
    let time_like = bytes.get(13) == Some(&b':') || bytes.get(16) == Some(&b':');
    let has_clock = text.contains(':');

    date_like && time_like && has_clock
}

pub fn short_prompt(prompt: &str, words: usize) -> String {
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

pub fn pretty_diff_output(diff: &str) -> String {
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

pub fn task_status_text(status: &TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "waiting",
        TaskStatus::Running => "running",
        TaskStatus::Merging => "merging",
        TaskStatus::Merged => "merged",
        TaskStatus::Failed => "failed",
    }
}

#[cfg(test)]
mod tests {
    use super::clean_log_line;

    #[test]
    fn strips_ansi_color_codes() {
        let line = "\u{1b}[38;5;247msrc/models/task.rs [1 - 101]\u{1b}[0m";
        assert_eq!(clean_log_line(line), "src/models/task.rs [1 - 101]");
    }

    #[test]
    fn strips_timestamp_prefix() {
        let line = "[2026-04-14 20:08:11] task done";
        assert_eq!(clean_log_line(line), "task done");
    }
}