use std::process::Command;

const BIN: &str = "aj";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const DESCRIPTION: &str = env!("CARGO_PKG_DESCRIPTION");

pub enum Action {
    RunApp,
    Exit(i32),
}

pub fn parse(args: &[String]) -> Action {
    // no args -> run the app
    let Some(first) = args.get(1) else {
        return Action::RunApp;
    };

    match first.as_str() {
        "-h" | "--help" | "help" => {
            print_help();
            Action::Exit(0)
        }
        "-V" | "--version" | "version" => {
            println!("{BIN} {VERSION}");
            Action::Exit(0)
        }
        "update" => Action::Exit(run_update()),
        other => {
            eprintln!("{BIN}: unknown command `{other}`\n");
            print_help();
            Action::Exit(2)
        }
    }
}

fn print_help() {
    println!("{BIN} {VERSION}");
    println!("{DESCRIPTION}");
    println!();
    println!("USAGE:");
    println!("    {BIN} [COMMAND]");
    println!();
    println!("COMMANDS:");
    println!("    (none)       open the TUI app");
    println!("    update       pull latest changes and reinstall");
    println!("    version      print version");
    println!("    help         print this help");
    println!();
    println!("FLAGS:");
    println!("    -h, --help       print help");
    println!("    -V, --version    print version");
}

fn run_update() -> i32 {
    println!("{BIN}: updating...");

    if !run("git", &["pull", "--ff-only"]) {
        eprintln!("{BIN}: git pull failed (not a git repo or no remote?)");
        return 1;
    }

    if !run("cargo", &["install", "--path", ".", "--force"]) {
        eprintln!("{BIN}: cargo install failed");
        return 1;
    }

    println!("{BIN}: update done");
    0
}

fn run(cmd: &str, args: &[&str]) -> bool {
    match Command::new(cmd).args(args).status() {
        Ok(s) => s.success(),
        Err(e) => {
            eprintln!("{BIN}: failed to run `{cmd}`: {e}");
            false
        }
    }
}
