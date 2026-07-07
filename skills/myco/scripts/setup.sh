#!/usr/bin/env bash
set -euo pipefail

# myco project management setup script
# Automates setting up a 'project' branch as a git worktree at 'project/'

echo "=== Initializing myco Project Management ==="

# 1. Verify we are in a Git repository
if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "Error: Current directory is not a Git repository." >&2
  exit 1
fi

# 2. Check if project directory or worktree already exists
if [ -d "project" ]; then
  echo "Error: 'project' directory already exists. Setup aborted." >&2
  exit 1
fi

# Store the current branch name to return to it later
CURRENT_BRANCH=$(git branch --show-current)

# 3. Create 'project' branch if it does not exist
if git show-ref --verify --quiet refs/heads/project; then
  echo "Branch 'project' already exists."
else
  echo "Creating orphaned branch 'project'..."
  # Create an orphaned branch (no shared history with main codebase)
  git checkout --orphan project
  
  # Clear the index/staging area
  git rm -rf . >/dev/null || true
  
  # Create initial README
  echo "# Project Management Branch" > README.md
  git add README.md
  git commit -m "Initialize project branch" >/dev/null
  
  # Switch back to the original branch
  git checkout "$CURRENT_BRANCH" >/dev/null
fi

# 4. Add the worktree mounted at project/
echo "Adding git worktree 'project' branch at 'project/'..."
git worktree add project project

# 5. Populate initial files in project/
echo "Initializing directory structure inside project/..."
mkdir -p project/proposals project/issues

if [ ! -f "project/TODO.md" ]; then
  cat << 'EOF' > project/TODO.md
# TODO

- [ ] Initial task
EOF
fi

if [ ! -f "project/BACKLOG.md" ]; then
  cat << 'EOF' > project/BACKLOG.md
# Backlog
EOF
fi

# Commit the initial files to the project worktree
echo "Committing initial project files..."
(
  cd project
  git add TODO.md BACKLOG.md proposals/ issues/
  git commit -m "Initialize myco file structure" >/dev/null || echo "Nothing to commit in project worktree."
)

# 6. Add 'project/' to parent .gitignore
if [ -f ".gitignore" ] && grep -q "^project/\$" .gitignore; then
  echo "'project/' is already in .gitignore."
else
  echo "Adding 'project/' to .gitignore..."
  echo "" >> .gitignore
  echo "# myco worktree mount point" >> .gitignore
  echo "project/" >> .gitignore
  
  # Prompt to commit .gitignore
  git add .gitignore
  git commit -m "Ignore myco project/ worktree mount point" >/dev/null || true
fi

echo "=== myco initialization complete! ==="
echo "You can now manage project tasks in project/TODO.md and project/BACKLOG.md"
