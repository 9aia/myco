+++
id = "ADR-0001"
title = "ndjson serve wire for peregrin embedding"
status = "done"
created = "2026-10-29"
priority = "medium"
+++

# ndjson serve wire for peregrin embedding

Status: **accepted** · Created: 2026-10-29

## Context

peregrin orchestrates campaigns across 9aia repos; its nodes need
programmatic access to the myco ledger. Exec-per-verb works —
every verb is idempotent, `--json`-shaped, and commits atomically —
but each call pays process spawn + discovery + store open, and
concurrent `myco` spawns race on the redb `index.lock`. A single
long-lived process per repo makes verbs cheap and, more
importantly, *serialized*: one process = one mutation at a time.

sepia set the precedent with `sepia serve`, an ndjson JSON-RPC
wire on stdio so peregrin embeds session lifecycle without
shell-per-call.

## Options

Wire:

- **JSON-RPC 2.0 over ndjson** — id-correlated results and
  errors, well-known error codes, trivially produced and parsed
  in any language.
- **REST/HTTP** — needs a port, a listener, and an auth story;
  oversized for one-repo embedding.
- **Bare ndjson `{method, params}`** — minimal, but reinvents
  correlation and the error taxonomy.
- **LSP-style Content-Length framing** — tolerates embedded
  newlines in params, but ndjson is sufficient (requests are
  single-line JSON) and keeps the debug surface `echo`-able.

Transport:

- **stdio** — the embedder already owns the child's pipes; zero
  config, no port discovery, dies with the parent.
- **unix socket / TCP** — enables multi-client and remote, but
  adds addressing, lifecycle, and auth questions myco doesn't
  want to answer yet.

## Decision

`myco serve` — a sequential request/response verb surface, built
as *two* abstractions so neither choice above is baked in:

- `ServeTransport` — one request line in, one response line out.
  `StdioTransport` (stdin `BufRead` lines → stdout `writeln!` +
  flush) is the first impl.
- `WireProtocol` — `parse(line) → Request`, `encode(Response) →
  String`. `JsonRpc2` is the default impl: id-correlated
  result/error envelopes; `-32700` parse, `-32600` invalid or
  missing-id (notifications aren't served), `-32601` unknown
  method, `-32602` bad params, `-32000` verb error, `-32001`
  invalid status transition with `data:{from,to,valid:[]}`.
- `dispatch(method, params) → Value` — the transport- and
  wire-agnostic verb surface.

Sequential, not concurrent: one process serializes mutations —
that serialization *is* the point (it is what fixes the
index.lock races), so writes are first-class methods, not a
later bolt-on. The store re-opens per request — no stale state —
and every write ends in the same atomic `chore:` commit the CLI
performs (`--no-commit`/`MYCO_NO_COMMIT` honored).

v1 methods: `initialize`, `manifest`, `schema`; reads — `status`
(+`prompt`), `board`, `item.show`, `todo.list`, `backlog.list`,
`issue|proposal|plan|adr.list`, `report`, `resume`, `check`;
writes — `todo.add`, `todo.done`, `todo.defer`, `backlog.add`,
`backlog.promote`, `item.set`, `handoff`, `interrupt`. Params are
the same fields the CLI flags carry (`note: "-"` is refused —
stdin IS the wire, no served method reads content from it).

Explicitly out: notifications/events, org/index methods, auth,
sockets, batch, init/migrate/`check --fix`/absorb/ui,
multi-request transactions.

## Consequences

- peregrin spawns `myco serve` per repo and gets the whole verb
  surface at pipe cost — no exec-per-verb, no lock contention.
- The CLI stays canonical: `serve` is a shim over the same ops,
  and method payloads match the `--json` shapes.
- Swapping or adding a wire (LSP framing) or transport (unix
  socket) is additive — `dispatch` doesn't change.
- Deferred to later ADRs: notifications/events (ledger-changed
  push), multi-verb transactions, batch requests, unix-socket/TCP
  transports, `org.*`/`index.*` methods.
