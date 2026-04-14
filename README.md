# junie

Small terminal app that runs Junie tasks in git worktrees.

## What it does

- lets you queue a prompt from the TUI
- makes a new git worktree and branch for each task
- runs `junie <prompt>` inside that worktree
- shows task logs and a live `git diff`
- lets you approve and merge a finished task

## Requirements

- Rust and Cargo
- Git
- `junie` on your `PATH`
- a git repo with a clean enough state to add worktrees

## Run

```bash
cargo run
```

## Keys

- `n` or `i` new task
- `j` or Down select next task
- `k` or Up select last task
- `y` approve and merge a task that is waiting
- `q` or `Ctrl+C` quit

## Task flow

1. Enter a prompt.
2. The app makes a branch like `agent/task-1`.
3. The app makes a worktree like `../agent-worktree-1`.
4. Junie runs in that worktree.
5. When Junie ends the task moves to approval.
6. Press `y` to merge the branch and remove the worktree.

## Screen layout

- left panel task list and status
- top right recent agent logs
- bottom right current diff for the selected task
- footer status or prompt input

## Status marks

- `[P]` pending
- `[R]` running
- `[?]` needs approval
- `[M]` merged
- `[X]` failed