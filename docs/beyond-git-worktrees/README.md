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

## suggested direction

blend 1 and 2:

1. spawn agent in an overlay mount of the repo
2. on finish, run `git diff` against the base
3. apply the patch to the real repo with `--3way`
4. drop the overlay

gains

- one real repo, one real branch
- no worktree cleanup
- fast start, cheap parallel tasks
- conflicts surface in one place

keep option 4 for tasks that need to run untrusted code

## next steps

- prototype overlay mount helper in `src`
- swap worktree setup for overlay setup
- replace branch merge with `git apply --3way`
- keep the current ui, only the backend changes
