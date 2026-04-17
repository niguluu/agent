mod app;
mod cli;
mod models;
mod runner;
mod terminal;
mod ui;

use app::{App, run_app};
use runner::bootstrap_existing_tasks;
use std::{env, error::Error, process, sync::Arc};
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    match cli::parse(&args) {
        cli::Action::RunApp => {}
        cli::Action::Exit(code) => process::exit(code),
    }

    let mut terminal = terminal::setup()?;
    let app = Arc::new(Mutex::new(App::new()));

    // Recovery of existing tasks
    bootstrap_existing_tasks(app.clone()).await;

    let res = run_app(&mut terminal, app).await;

    terminal::restore(&mut terminal)?;

    if let Err(err) = res {
        println!("{err:?}");
    }

    Ok(())
}
