# Junie Agent Orchestrator

A small terminal app for running many headless Junie tasks at once.

It starts each task in its own Git worktree and branch, shows live agent output, and lets you merge finished work from the TUI.

## What this app does

The app gives you one place to:

- queue new Junie prompts
- keep each task in its own branch and worktree
- watch agent logs and live diffs while it runs
- review finished work before you merge it

This keeps your main working tree clean while agents work in parallel.

## Requirements

You need these tools before you start:

- Rust and Cargo
- Git with `git worktree` support
- `junie` CLI in your `PATH`

The app should be started from inside a Git repo. For each task it creates:

- a branch named `agent/task-<id>`
- a worktree at `../agent-worktree-<id>`

## Run the app

From the project root run:

```bash
cargo run
```

To build a release binary run:

```bash
cargo build --release
```

The app starts in normal mode with an empty task list.

## Screen layout

The TUI has three main parts:

1. **Active Agents** on the left
   - shows all tasks
   - each row shows the task id and branch name
2. **Agent Action & Thoughts** on the top right
   - shows recent agent output
   - keeps the newest log lines visible
3. **Live Diff / File Watcher** on the bottom right
   - shows the current `git diff` from the task worktree
   - refreshes while the task is running

The footer changes by mode:

- **Normal mode** shows task status and key hints
- **Input mode** lets you type a new prompt

## Task flow

Each task moves through this flow:

1. You press `n` or `i`
2. You type a prompt and press `Enter`
3. The app creates a new worktree and branch
4. The app starts `junie <prompt>` in that worktree
5. The app streams logs and updates the diff view
6. When the agent exits the task waits for review
7. You press `y` to merge the task branch into your current branch
8. The app removes the task worktree and deletes the task branch

If you press `Enter` on an empty prompt, the app does not create a task.

## Task states

The left list uses short state marks:

- `[P]` Pending
- `[R]` Running
- `[?]` Needs approval
- `[M]` Merged
- `[X]` Failed

## Keys

### Normal mode

- `n` or `i` start a new task
- `Up` or `k` move up the task list
- `Down` or `j` move down the task list
- `y` merge the selected task when it is ready
- `q` or `Ctrl+c` quit the app

### Input mode

- type to enter the prompt
- `Enter` submit the task
- `Backspace` delete one character
- `Esc` cancel and go back

## What you see while it runs

- the log panel keeps the newest lines from agent stdout and stderr
- the task stores up to 1000 log lines
- the log view shows the newest 20 lines for the selected task
- the diff panel refreshes about every 2 seconds while the task runs
- `y` only starts a merge when the task state is `[?]`

## Merge behavior

When you approve a finished task, the app:

- runs a `git merge --no-ff` for the task branch
- removes the task worktree
- deletes the task branch
- marks the task as merged or failed in the UI

If the merge fails, the error text is added to the task log.

## Notes

- The app does not auto merge work. You review first.
- If `junie` cannot start, the task is marked failed.
- If Git worktree setup fails, the task is marked failed and logs show the error.
- If you press `y` on a task that is not ready, nothing happens.
