---
layout: home

hero:
  name: myco
  text: project management, grown not built
  tagline: a mycorrhizal network for your agents — tasks, signals, handoffs
  actions:
    - theme: brand
      text: Getting Started
      link: /getting-started
    - theme: alt
      text: Tutorial
      link: /tutorials/first-item

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
