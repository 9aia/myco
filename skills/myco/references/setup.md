# How to Setup Myco on a Repository

To configure a repository to use `myco` project management for the first time, you must initialize a `project` branch, set up a git worktree at `project/`, and update `.gitignore`.

This skill provides an automated script to handle this configuration.

## Automated Setup (Recommended)
Run the bundled setup script from the skill directory or path:
```bash
bash <skill-path>/scripts/setup.sh
```

## Manual Setup Steps
If you need to perform the setup manually:

1. **Create the `project` branch**:
   We recommend an orphaned branch (no shared history with the codebase):
   ```bash
   git checkout --orphan project
   git rm -rf .
   echo "# Project Management" > README.md
   git add README.md
   git commit -m "Initialize project branch"
   git checkout -  # Switch back to your previous branch
   ```

2. **Mount the worktree**:
   Mount the `project` branch onto the `project/` directory:
   ```bash
   git worktree add project project
   ```

3. **Initialize the files**:
   Inside `project/`, create the folder structures and base templates:
   - `project/proposals/`
   - `project/issues/`
   - `project/TODO.md` (initial task list)
   - `project/BACKLOG.md` (empty backlog file)

   Commit these files inside the worktree directory:
   ```bash
   cd project
   git add .
   git commit -m "Initialize myco file structure"
   cd ..
   ```

4. **Ignore the worktree folder**:
   Add `project/` to the parent repository's `.gitignore` file to ensure the worktree files are not tracked in other code branches:
   ```bash
   echo "project/" >> .gitignore
   git add .gitignore
   git commit -m "Ignore myco project/ worktree mount point"
   ```
