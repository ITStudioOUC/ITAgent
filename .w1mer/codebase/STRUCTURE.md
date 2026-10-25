# STRUCTURE — Layout & Module Map

> Stable layer, second in fixed read order. Edit rarely.

## Crate layout

```
ita-kernel         pure logic: message tree, resolve(), Command AST/lexer,
                   checkpoint & lease types, Policy/Fidelity. No IO, no async, no DB.
ita-store          SQLite impl behind operation-level traits (not SQL-level);
                   WorkspaceScope, seq cursor, CAS layout, snapshots
ita-proto          Invocation envelope, frame codec, version negotiation
ita-sandbox        Sandbox trait + Linux impl + fidelity probe
ita-exec           per-workspace executor: permission precheck, leases, shim host,
                   cancellation, pgid reaping
ita-tools          manifest parsing, shim ABI, tool registry, installer
ita-model          provider codecs (OpenAI/Anthropic), usage accounting, streaming
ita-workflow       workflow DSL, event-sourced node state, static conflict lint
ita-server         units/supervisor, router, DB+queue owner, LLM proxy, event log
ita-worker         workspace worker process (stateless)
ita-cli            the `ita` binary (role dispatch: master|worker|tool-host)
```

**Status.** Epics 01–06 are complete. The kernel holds the command grammar, the context
tree with pure `resolve()`, and the semantic-compression planning modules. `ita-store`
has completed the full storage epic: database handle, migration runner, workspace scope,
initial schema, event append with seq allocation and preview truncation, frame write with
head compare-and-swap, content-addressed storage with dedup and atomic rename, snapshot
manifest with entry-shape validation and blobs assertion, baseline-plus-incremental squash
with rebaseline, rollback alignment with write-back and residual list, and mark-sweep GC
with snapshot/lease roots and the two-observation grace period. The remaining crates are
shells whose responsibilities above are still targets. `ita-kernel` has no dependencies
at all — not even dev-dependencies — enforced by
`.w1mer/scripts/check_kernel_purity.sh` plus `crates/ita-kernel/clippy.toml`. Role
dispatch lives in `crates/ita-cli/src/role.rs`.

## Dependency edges

`.w1mer/scripts/check_layers.sh` is the executable contract; this section is the
summary. A member may depend only on the crates listed below. `dev` and `build`
dependencies are also checked for `ita-*` edges, because a dev-dependency can create a
hidden back-edge that the compiler would otherwise allow.

| crate | allowed `ita-*` dependencies |
|---|---|
| `ita-kernel` | none |
| `ita-store` | `ita-kernel` |
| `ita-proto` | `ita-kernel`, `ita-model` |
| `ita-model` | `ita-kernel` |
| `ita-workflow` | `ita-kernel` |
| `ita-sandbox` | `ita-kernel` |
| `ita-tools` | `ita-kernel`, `ita-proto`, `ita-sandbox` |
| `ita-exec` | `ita-kernel`, `ita-proto`, `ita-sandbox`, `ita-tools` |
| `ita-worker` | `ita-kernel`, `ita-proto`, `ita-model` |
| `ita-server` | `ita-kernel`, `ita-store`, `ita-proto`, `ita-model`, `ita-workflow`, `ita-tools` |
| `ita-cli` | all ten library crates (composition root) |

The consequences worth remembering:

- No library crate may depend on `ita-cli`; the binary is the only composition root.
- `ita-worker`, `ita-exec` and `ita-sandbox` never depend on `ita-store`. The database
  is reachable only through `ita-server`.
- `ita-proto -> ita-model` is one-way; `ita-model` never depends on `ita-proto`.
- Only `ita-cli` produces a binary (`ita`). A new binary target must be registered in
  the script's `ALLOWED_BINS` with a stated purpose.
- Capability ownership: SQLite drivers only in `ita-store`; HTTP/TLS stacks only in
  `ita-server`; landlock/seccomp only in `ita-sandbox`; `libc` only in `ita-sandbox`
  and `ita-exec`; `nix`/`rustix` also in `ita-server` for unit supervision.

The gate is a layering guard, not a security boundary: it cannot see `std::net`,
`tokio::net`, build-script network access, transitive dependencies, or `pub use`
re-exports, and crates allowed to use `libc` can still issue raw system calls.
Sandbox policy remains the real network boundary (ADR 0010).

## Cross-module data flow

- Worker never touches the DB. Reads: batched `load_context` RPC. Writes:
  authoritative sync RPC (append event, CAS head, lease); telemetry may drop.
- `workspace` is a session property derived from the connection, never a request
  parameter.
- Two data classes: immutable event log (cacheable, replayable) and mutable
  authoritative state (heads, leases, catalog) — never mixed in one cache.
- Tool call path: worker → server(lease via executor) → sandbox → shim → binary;
  snapshot taken after pgid reap.
