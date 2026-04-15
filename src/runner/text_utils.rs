pub fn clean_log_line(line: &str) -> String {
    line.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect::<String>()
        .trim()
        .to_string()
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
