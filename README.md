# Junie Agent Orchestrator

A terminal-based multi-agent orchestrator for managing headless Junie agents. Built with Rust, Ratatui, and Tokio.

## Overview

This application provides a Terminal User Interface (TUI) to spawn, manage, and monitor multiple independent Junie agents working concurrently. Instead of having agents manipulate your current working directory directly, the orchestrator isolates each agent into its own Git worktree on a dedicated branch.

## Features

- **Headless Multi-Agent Factory:** Queue tasks by providing prompts. The orchestrator automatically creates a new Git worktree and branch (`agent/task-<id>`) for each task.
- **Live Monitoring:** Monitor each agent's logs, thoughts, and live code diffs in real-time right from the terminal.
- **Task Isolation:** Agents run in isolated worktrees (`../agent-worktree-<id>`), preventing them from interfering with your active development or with each other.
- **Easy Review and Merge:** Once an agent finishes its task, you can review the diff. If approved, the orchestrator handles merging the branch (`git merge --no-ff`), deleting the worktree, and cleaning up the branch with a single keystroke.

## How it Works

1. You input a prompt for a task using the TUI.
2. The orchestrator spins up a new Git worktree and a branch for the task.
3. A headless `junie` CLI process is spawned inside that worktree with your prompt.
4. The TUI streams the agent's stdout/stderr and periodically polls `git diff` to show you what changes the agent is making.
5. When the agent completes the task, it waits for your approval.
6. Pressing `y` merges the changes into your current branch and cleans up the worktree.

## Usage

Run the orchestrator using Cargo:

```bash
cargo run
```

### Keybindings

- `i` or `n` - Enter Input mode to create a new task.
- `Enter` (in Input mode) - Submit task prompt.
- `Up` / `Down` or `k` / `j` - Navigate through the list of active agents.
- `y` - Approve and merge a completed task.
- `q` or `Ctrl+c` - Quit the application.

## Dependencies

- **Rust / Cargo**
- **Git** (requires `git worktree` support)
- **junie** CLI (must be available in your `PATH`)
