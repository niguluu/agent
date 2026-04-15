# Junie Agent Orchestrator

Run many headless Junie tasks at once from a terminal UI.

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
4. The app writes `.junie/AGENTS.md` and `.junie/psudocode.yaml` inside the task worktree.
5. A headless `junie` CLI process is spawned inside that worktree with your prompt, both guide paths, the short file tree rules, and the short output rules.
6. The TUI streams the agent's stdout and stderr and polls `git diff` to show what changes the agent is making.
7. When the agent completes the task, the orchestrator auto merges the changes into `agents` and then into your current branch.
8. Press `y` on a merged or failed item to clear it from the list.

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

- `i` or `n` - Enter Input mode to create a new task.
- `Enter` (in Input mode) - Submit task prompt.
- `Esc` (in Input mode) - Leave Input mode.
- `Ctrl+v` or `Shift+Insert` (in Input mode) - Paste from the system clipboard.
- `Up` / `Down` or `k` / `j` - Navigate through the list of active agents.
- `Ctrl+Shift+c` - Copy the selected task prompt logs and diff to the system clipboard.
- `y` - Clear a merged or failed task from the list.
- `q` or `Ctrl+c` - Quit the application.

## Layout

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
4. The app writes `.junie/AGENTS.md` and `.junie/psudocode.yaml` in that worktree
5. The app starts `junie` in that worktree with your prompt plus both guide paths, short file tree rules, and short output rules
6. The app streams logs and updates the diff view
7. When the agent completes, the orchestrator auto merges the task into `agents` and then into your current branch
8. Press `y` to clear a merged or failed task from the list

If you press `Enter` on an empty prompt, the app does not create a task.

Each new task starts with one log line that says `Queued: <prompt>`.

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
# test sync Wed Apr 15 04:46:16 PM EDT 2026
