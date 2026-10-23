# CONVENTIONS — Conventions & Naming

> Stable layer, fifth in fixed read order. Edit rarely.

## Code

- `ita-kernel` must stay pure: no `async`, no IO, no clock, no RNG. If a type there
  needs an `&mut Store` or a socket, the separation has broken — stop and fix the design.
- Business queries are only reachable through `WorkspaceScope`; never expose a raw
  DB connection to business code.
- Schema migrations are **additive only** (add table / add column). Never rename or drop:
  the event log is immutable truth and must stay replayable.
- Timestamps: INTEGER epoch millis (UTC). IDs: ULID as `VARCHAR(26)`. No autoincrement,
  no `RETURNING` — those are the three things that keep the Postgres/MySQL door open.

## Vocabulary (use consistently)

| Term | Meaning |
|---|---|
| 回滚 / restore | move the workspace files pointer (snapshot) |
| 回退 / rewind | move the context pointer (node head) |
| 折叠 / fold | compress a frame into a summary and advance head |
| lease | write-permission token with TTL; one per tool call |
| frame | summarized context segment kept in place of raw history |
| unit | server process-lifecycle supervisor entry (systemd-like) |
| checkpoint | (context node, snapshot) pair referenced weakly by a frame |

## Docs

- Design decisions live in `docs/adr/NNNN-*.md` (ADR): status, decision, rationale,
  consequences (including cost), and rejected alternatives.
- A changed decision requires a **new** ADR; the old and new ones point at each other
  in their status sections. Editorial corrections (wrong fact, missing definition,
  broken cross-reference) may be applied directly, with a revision line in the status
  section. Test: would an already-implemented component become incompatible?
- Version `0.<N>.<P>` lives in `Cargo.toml`; `N` bumps per epic, `P` per finished
  batch. One bump per batch, in the last commit before `batch-end`.
- Planning docs live in `.w1mer/` and are driven only through the `w1mer` CLI.
