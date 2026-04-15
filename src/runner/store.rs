use crate::models::{SharedTasks, TaskStatus};

pub async fn set_task_status(tasks_ref: &SharedTasks, id: usize, status: TaskStatus) {
    let mut tasks = tasks_ref.lock().await;
    if let Some(task) = tasks.iter_mut().find(|task| task.id == id) {
        task.status = status;
    }
}

pub async fn push_task_log(tasks_ref: &SharedTasks, id: usize, msg: impl Into<String>) {
    let mut tasks = tasks_ref.lock().await;
    if let Some(task) = tasks.iter_mut().find(|task| task.id == id) {
        task.logs.push(msg.into());
    }
}

pub async fn push_task_output(tasks_ref: &SharedTasks, id: usize, line: String) {
    let mut tasks = tasks_ref.lock().await;
    if let Some(task) = tasks.iter_mut().find(|task| task.id == id) {
        task.logs.push(line);
        if task.logs.len() > 1000 {
            task.logs.remove(0);
        }
    }
}

pub async fn set_task_result(tasks_ref: &SharedTasks, id: usize, result: String) {
    let mut tasks = tasks_ref.lock().await;
    if let Some(task) = tasks.iter_mut().find(|task| task.id == id) {
        task.result = result;
    }
}

pub async fn set_task_diff(tasks_ref: &SharedTasks, id: usize, diff: String) {
    let mut tasks = tasks_ref.lock().await;
    if let Some(task) = tasks.iter_mut().find(|task| task.id == id) {
        task.diff = diff;
    }
}

pub async fn get_task_status(tasks_ref: &SharedTasks, id: usize) -> Option<TaskStatus> {
    let tasks = tasks_ref.lock().await;
    tasks
        .iter()
        .find(|task| task.id == id)
        .map(|task| task.status.clone())
}

pub async fn start_merge(
    tasks_ref: &SharedTasks,
    _selected: usize,
) -> Option<(usize, String, String)> {
    None
}
