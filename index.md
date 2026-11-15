---
layout: home

hero:
  name: myco
  text: project management, grown not built
  tagline: Agent-first project management — markdown ledger canonical, git the audit trail; a records + views + mutations engine underneath
  actions:
    - theme: brand
      text: Getting Started
      link: /tutorials/getting-started
    - theme: alt
      text: Daily agent loop
      link: /how-to/agent-loop

features:
  - icon: 🍄
    title: Work memory
    details: the ledger of intent — todos, backlog, issues, proposals, plans, ADRs, handoffs — markdown in project/ is canonical
    link: /explanation/architecture
    linkText: architecture
  - icon: 🤖
    title: Agent-first
    details: --json on every verb, myco manifest/schema self-discovery, status --prompt digest sized for a context window
    link: /reference/cli
    linkText: CLI reference
  - icon: 🌳
    title: Git is the audit trail
    details: every verb is one atomic commit on the orphan project branch — agents never touch git directly
  - icon: 🔗
    title: Continuity
    details: myco handoff attaches resumable state at session end; myco resume returns the minimal context to restart
    link: /how-to/handoff
    linkText: handoffs
---

## Where to go next

| You want to… | Go to |
| --- | --- |
| Install myco and track your first item | [Tutorial](/tutorials/getting-started) |
| Do a task (init a repo, handoffs, org views) | [How-to](/how-to/init-repo) |
| Understand the design (worktree model, ledger spec) | [Explanation](/explanation/worktree-model) |
| Look up a verb, config field, or file format | [Reference](/reference/cli) |
