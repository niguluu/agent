pub fn clean_log_line(line: &str) -> String {
    let cleaned = strip_ansi_sequences(line)
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect::<String>();

    cleaned
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !looks_like_terminal_artifact(line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_ansi_sequences(line: &str) -> String {
    let mut cleaned = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            match chars.peek().copied() {
                Some('[') => {
                    chars.next();
                    while let Some(next) = chars.next() {
                        if ('@'..='~').contains(&next) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    let mut prev_was_escape = false;
                    for next in chars.by_ref() {
                        if prev_was_escape && next == '\\' {
                            break;
                        }
                        prev_was_escape = next == '\u{1b}';
                    }
                }
                _ => {}
            }
            continue;
        }

        cleaned.push(ch);
    }

    cleaned
}

fn looks_like_terminal_artifact(line: &str) -> bool {
    if line.is_empty() {
        return true;
    }

    let mut chars = line.chars();
    let Some(first) = chars.next() else {
        return true;
    };

    if !first.is_ascii_digit() {
        return false;
    }

    chars.all(|ch| ch.is_ascii_digit() || matches!(ch, ';' | ':' | '?' | 'H' | 'J' | 'K' | 'm' | 'n'))
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

pub fn task_status_text(status: &crate::models::TaskStatus) -> &'static str {
    use crate::models::TaskStatus;
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
    fn clean_log_line_removes_ansi_color_codes() {
        let line = "\u{1b}[38;5;4mPlan\u{1b}[0m";

        assert_eq!(clean_log_line(line), "Plan");
    }

    #[test]
    fn clean_log_line_drops_terminal_cursor_artifacts() {
        assert_eq!(clean_log_line("12;45H"), "");
        assert_eq!(clean_log_line("6n"), "");
    }
}
