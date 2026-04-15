# Junie Agent Orchestrator

Run many headless Junie tasks at once from a terminal UI.

Each task gets its own Git worktree and branch. The app streams live logs and diffs, then auto merges finished work back into your branch.

## Requirements

- Rust and Cargo
- Git with worktree support
- `junie` CLI in your `PATH`

Start the app from inside a Git repo.

## Run

```bash
cargo run
```

```bash
cargo build --release
```

## Keys

- `i` or `n` - new task
- `Enter` - submit prompt
- `Esc` - cancel input
- `Up` / `Down` or `k` / `j` - navigate tasks
- `y` - clear merged or failed task
- `q` or `Ctrl+c` - quit

## Layout

- left: task list
- top right: agent logs
- bottom right: live git diff

## Task states

- `[P]` Pending
- `[R]` Running
- `[>]` Merging
- `[M]` Merged
- `[X]` Failed

## Task flow

1. press `n`, type a prompt, press `Enter`
2. app creates a worktree and branch from `agents`
3. app writes `.junie/AGENTS.md` in the worktree
4. app starts `junie` with your prompt
5. logs and diff stream live
6. on finish, merges task branch into `agents` then into your current branch
7. press `y` to clear

## Merge

- commits any dirty changes first
- merges task branch into `agents` with `--no-ff`
- merges `agents` into your current branch with `--no-ff`
- removes worktree and deletes task branch

If merge fails, the error goes to the task log.

## Recovery

Old task worktrees found on startup are loaded and auto merged.

## Crates

- `crossterm` - terminal input and raw mode
- `ratatui` - TUI layout
- `tokio` - async, processes, timers
- `futures`, `notify` - listed in Cargo.toml, not yet used
