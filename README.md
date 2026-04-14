# Junie

Small TUI app to run Junie tasks in git worktrees.

## What it does

- adds a new git worktree for each task
- runs `junie` in that worktree
- shows logs and live diff
- lets you merge with one key

## What you need

- Rust and Cargo
- Git
- `junie` on your PATH
- a git repo to run in

## Run

```bash
cargo run
```

## Keys

- `n` or `i` new task
- `j` or `Down` next task
- `k` or `Up` last task
- `y` merge approved task
- `q` or `Ctrl+C` quit

## Flow

1. start the app
2. add a prompt
3. wait for the agent to finish
4. check logs and diff
5. press `y` to merge
