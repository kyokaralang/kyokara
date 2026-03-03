# AGENTS

## Default Git Workflow (Required)

For every requested task, use an isolated branch + isolated worktree flow:

1. Sync first:
   - `git fetch origin`
   - base all work from latest `origin/main`
2. Create a dedicated worktree and branch from `origin/main`:
   - branch names must be prefixed with `enhancement/`
3. Do all edits only inside that task worktree.
4. Before PR/merge:
   - sync/rebase against latest `origin/main`
   - verify clean merge target
5. After merge:
   - delete the task branch (local + remote)
   - remove/prune the task worktree
6. Never implement task changes in an already-dirty shared worktree.

## Task Start Confirmation

Before editing files, report:

- worktree path
- branch name
- base commit SHA from `origin/main`
- clean/dirty status
