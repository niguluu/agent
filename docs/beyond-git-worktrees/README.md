# beyond git worktrees

research on better ways to run many agents in parallel without a worktree per task

## why move on

- slow to clone a full tree per task
- heavy on disk for big repos
- leaves stale dirs on crash
- shared branch `agents` serializes merges
- no real isolation (same env, same network, same fs)

## options

### 1. overlay filesystem sandboxes (recommended)

each agent gets a thin overlayfs mount over the main repo

- lower dir = real repo (read only)
- upper dir = per task scratch
- near instant setup, near zero disk cost
- drop the upper dir to discard work
- diff upper dir to produce a patch

tools: `overlayfs` on linux, `bindfs` on mac, or a plain copy on write dir

### 2. patch based flow

agent never touches the repo, only returns a unified diff

- orchestrator holds one repo
- each task runs in a temp dir with a sparse checkout
- on finish, apply patch with `git apply --3way`
- conflicts stay local to the orchestrator

pairs well with option 1

### 3. cow snapshots (btrfs or zfs)

- `btrfs subvolume snapshot` per task
- instant clone, cheap teardown
- needs a cow fs on the host

### 4. container per task

- docker or podman, bind mount the repo read only
- adds real process, net, and fs isolation
- heavier than overlay, lighter than a vm

good when the agent runs shell or build steps that may break the host

### 5. jj or sapling stacked commits

- each agent = one commit on its own op
- rebase instead of merge
- no worktree at all, just one repo
- great ux for stacked work

needs the team to adopt `jj` or `sl`

## the real setup

no orchestrator here. this is many junie headless agents running wild in parallel on the same repo. no central writer, no patch queue, no human in the loop serializing merges.

that changes the problem. we can't "just return diffs to the orchestrator" because there is no orchestrator. each agent has to be able to:

- read a consistent view of the code
- write freely without corrupting another agent's run
- land its work somewhere that won't get trampled

so isolation is not optional, it's the whole point. the 5 options above are not overkill, they are the actual menu.

## suggested direction for wild agents

default: overlayfs per agent (option 1)

- each headless junie boots into its own overlay mount of the repo
- lower = shared read only repo, upper = agent's scratch
- agent does whatever it wants in there, including builds and tests
- on finish, diff the upper dir and commit to its own branch (or just leave the diff as the artifact)
- crash = drop the upper dir, no cleanup of worktrees needed

why this fits a no orchestrator world:

- no shared writer to bottleneck on
- no "apply patch to main" step that needs a referee
- agents land on their own branches, merging is a later/human problem
- near zero startup cost so spawning N junies stays cheap

escalate per agent when needed:

| swap in | when that agent needs |
|---|---|
| 4 containers | to run untrusted shell or network stuff safely |
| 3 btrfs or zfs snapshot | you already run a cow fs and want even cheaper clones |
| 5 jj or sapling | you want stacked commits instead of branch soup |

patch only flow (option 2) is still fine for agents that don't need to execute anything, but it's not the default anymore since most junie tasks do want to run code.

## what we're explicitly not doing

- no central orchestrator applying patches to `main`
- no serialized merge queue
- no shared `agents` branch
- no assumption that one writer resolves conflicts

conflicts between agents are resolved later, by whoever merges the branches, not during the run.

## next steps

- ship an overlay mount helper that junie headless can call on startup
- give each agent a unique upper dir and its own branch name
- on exit, commit the overlay diff to that branch and unmount
- add container mode as an opt in for agents that run untrusted code
- leave merging of agent branches to humans or a later pass, not the runtime
