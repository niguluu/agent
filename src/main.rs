mod app;
mod models;
mod runner;
mod terminal;
mod ui;

use app::{App, run_app};
use std::{error::Error, sync::Arc};
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut terminal = terminal::setup()?;
    let app = Arc::new(Mutex::new(App::new()));

    let res = run_app(&mut terminal, app).await;

    terminal::restore(&mut terminal)?;

    if let Err(err) = res {
        println!("{err:?}");
    }

    Ok(())
}
