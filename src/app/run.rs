use crate::{app::input::handle_key_event, ui::render};
use crossterm::event::{self, Event};
use ratatui::{Terminal, backend::Backend};
use std::{
    error::Error,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

use super::App;

pub async fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    app: Arc<Mutex<App>>,
) -> Result<(), Box<dyn Error>>
where
    <B as Backend>::Error: 'static,
{
    let tick_rate = Duration::from_millis(250);
    let mut last_tick = Instant::now();

    loop {
        {
            let app_state = app.lock().await;
            let tasks = app_state.tasks.lock().await;
            terminal.draw(|frame| render(frame, &app_state, &tasks))?;
        }

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
        {
            let mut app_state = app.lock().await;
            if handle_key_event(&mut app_state, key).await {
                return Ok(());
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }
}
