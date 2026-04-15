# Junie Agent Orchestrator

A small terminal app for running many headless Junie tasks at once.

It lets you queue prompts, starts each task in its own Git worktree and branch, shows live agent output and diffs, recovers old task worktrees on open, and auto merges finished work back into your current branch.

The crate name is `junie`. Running `cargo run` starts the TUI app.

## What this app does

The app gives you one place to:

- **Headless Multi-Agent Factory:** Queue tasks by providing prompts. The orchestrator automatically creates a new Git worktree and branch (`agent/task-<id>`) for each task.
- **Live Monitoring:** Monitor each agent's logs, thoughts, and live code diffs in real-time right from the terminal.
- **Task Isolation:** Agents run in isolated worktrees (`../agent-worktree-<id>`), preventing them from interfering with your active development or with each other.
- **Easy Review and Merge:** When an agent finishes its task, the orchestrator merges the task branch into `agents`, then merges `agents` back into your current branch. It then removes the worktree and deletes the task branch. You can clear the finished item from the TUI with one key.
- **Recovery on Open:** If old task worktrees are still there when the app starts, it loads them into the list and starts auto merge for them.

This keeps your main working tree clean while agents work in parallel.

## How it Works

1. You input a prompt for a task using the TUI.
2. The orchestrator spins up a new Git worktree and a branch for the task.
3. The app makes sure the shared `agents` branch exists.
4. The app writes `.junie/AGENTS.md` inside the task worktree.
5. A headless `junie` CLI process is spawned inside that worktree with your prompt, the guide path, and the short output rules.
6. The TUI streams the agent's stdout and stderr and polls `git diff` to show what changes the agent is making.
7. When the agent completes the task, the orchestrator auto merges the changes into `agents` and then into your current branch.
8. Press `y` on a merged or failed item to clear it from the list.

## Requirements

You need these tools before you start:

- Rust and Cargo
- Git with `git worktree` support
- `junie` CLI in your `PATH`

The app should be started from inside a Git repo. For each task it creates:

- a branch named `agent/task-<id>`
- a worktree at `../agent-worktree-<id>`

The app also keeps a shared branch named `agents`. New task worktrees are created from this branch.

## Current imports and crates

The app currently depends on these crates:

- `crossterm` for raw terminal mode, screen switching, and key input
- `ratatui` for the TUI layout and widgets
- `tokio` for async tasks, process handling, timers, and shared state
- `futures` is listed in `Cargo.toml` but is not imported in `src` yet
- `notify` is listed in `Cargo.toml` but is not imported in `src` yet

The Rust source also imports standard library parts like `Arc`, `Mutex`, `Error`, `io`, `Path`, `Stdio`, `Duration`, and `SystemTime`.

## Run the app

From the project root run:

```bash
cargo run
```

To build a release binary run:

```bash
cargo build --release
```

## Keys

- `i` or `n` - Enter Input mode to create a new task.
- `Enter` (in Input mode) - Submit task prompt.
- `Esc` (in Input mode) - Leave Input mode.
- `Up` / `Down` or `k` / `j` - Navigate through the list of active agents.
- `y` - Clear a merged or failed task from the list.
- `q` or `Ctrl+c` - Quit the application.

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
   - refreshes by polling while the task is running

If there is no selected task yet, the right side shows `No task selected` and an empty diff.

The footer changes by mode:

- **Normal mode** shows task status and key hints
- **Input mode** lets you type a new prompt

## Task flow

Each task moves through this flow:

1. You press `n` or `i`
2. You type a prompt and press `Enter`
3. The app creates a new worktree and branch from `agents`
4. The app writes `.junie/AGENTS.md` in that worktree
5. The app starts `junie` in that worktree with your prompt plus the guide path and short output rules
6. The app streams logs and updates the diff view
7. When the agent completes, the orchestrator auto merges the task into `agents` and then into your current branch
8. Press `y` to clear a merged or failed task from the list

If you press `Enter` on an empty prompt, the app does not create a task.

Each new task starts with one log line that says `Queued: <prompt>`.

## Task states

The left list uses short state marks:

- `[P]` Pending
- `[R]` Running
- `[>]` Merging
- `[M]` Merged
- `[X]` Failed

## What you see while it runs

- the log panel keeps the newest lines from agent stdout and stderr
- the task stores up to 1000 log lines
- the log view shows the newest lines for the selected task
- the diff panel refreshes about every 800 ms while the task runs

## Merge behavior

When an agent finishes its task, the app:

- commits any dirty task worktree changes first
- merges the task branch into `agents` with `git merge --no-ff`
- merges `agents` into your current branch with `git merge --no-ff`
- force removes the task worktree after the merge steps finish
- deletes the task branch after the merge steps finish
- marks the task as merged or failed in the UI

If the merge fails, the error text is added to the task log.

## Recovery on open

If old task worktrees already exist when the app starts, it loads them into the list and starts auto merge for them at once.

## Notes

- The app auto merges work into your current branch by way of `agents`.
- If `junie` cannot start, the task is marked failed.
- If Git worktree setup fails, the task is marked failed and logs show the error.
- Press `y` on merged/failed tasks to clear them from the list.
