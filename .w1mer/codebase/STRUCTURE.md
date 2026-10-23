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

**Status.** All eleven crates exist as directories with their manifests and a `lib.rs`
carrying only module documentation; `ita-cli` additionally has role dispatch and tests.
Except for that, the responsibilities listed above are the *target* for each crate, not
current behaviour. The kernel's first real content lands in epics 03–05. `ita-kernel` has no dependencies at all — not even
dev-dependencies — enforced by `.w1mer/scripts/check_kernel_purity.sh` plus
`crates/ita-kernel/clippy.toml`. `ita-cli` is the only member that produces a binary
(`ita`); role dispatch lives in `crates/ita-cli/src/role.rs`.

## Cross-module data flow

- Worker never touches the DB. Reads: batched `load_context` RPC. Writes:
  authoritative sync RPC (append event, CAS head, lease); telemetry may drop.
- `workspace` is a session property derived from the connection, never a request
  parameter.
- Two data classes: immutable event log (cacheable, replayable) and mutable
  authoritative state (heads, leases, catalog) — never mixed in one cache.
- Tool call path: worker → server(lease via executor) → sandbox → shim → binary;
  snapshot taken after pgid reap.
