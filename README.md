# Junie Agent Orchestrator

A small terminal app for running many headless Junie tasks at once.

It starts each task in its own Git worktree and branch, shows live agent output, and auto merges finished work.

## What this app does

The app gives you one place to:

- **Headless Multi-Agent Factory:** Queue tasks by providing prompts. The orchestrator automatically creates a new Git worktree and branch (`agent/task-<id>`) for each task.
- **Live Monitoring:** Monitor each agent's logs, thoughts, and live code diffs in real-time right from the terminal.
- **Task Isolation:** Agents run in isolated worktrees (`../agent-worktree-<id>`), preventing them from interfering with your active development or with each other.
- **Easy Review and Merge:** When an agent finishes its task, the orchestrator merges the task branch, removes the worktree, and cleans up the branch. You can then clear the finished item from the TUI with one key.

This keeps your main working tree clean while agents work in parallel.

## How it Works

1. You input a prompt for a task using the TUI.
2. The orchestrator spins up a new Git worktree and a branch for the task.
3. A headless `junie` CLI process is spawned inside that worktree with your prompt.
4. The TUI streams the agent's stdout/stderr and periodically polls `git diff` to show you what changes the agent is making.
5. When the agent completes the task, the orchestrator auto merges the changes into your main work branch.
6. Press `y` on a merged or failed item to clear it from the list.

## Requirements

You need these tools before you start:

- Rust and Cargo
- Git with `git worktree` support
- `junie` CLI in your `PATH`

The app should be started from inside a Git repo. For each task it creates:

- a branch named `task-<id>`
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

## Keys

- `i` or `n` - Enter Input mode to create a new task.
- `Enter` (in Input mode) - Submit task prompt.
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
6. When the agent completes, the orchestrator auto merges the task
7. Press `y` to clear a merged or failed task from the list

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
- the diff panel refreshes about every 2 seconds while the task runs

## Merge behavior

When an agent finishes its task, the app:

- runs a `git merge --no-ff` for the task branch
- removes the task worktree
- deletes the task branch
- marks the task as merged or failed in the UI

If the merge fails, the error text is added to the task log.

## Notes

- The app auto merges work into your main branch.
- If `junie` cannot start, the task is marked failed.
- If Git worktree setup fails, the task is marked failed and logs show the error.
- Press `y` on merged/failed tasks to clear them from the list.
