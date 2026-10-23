# STACK — Toolchain & Environment

> Stable layer, first in fixed read order. Edit rarely.

- Language: Rust (edition 2024), single binary `ita`, cargo workspace
- Build: `cargo build` / `cargo test`; async only in I/O crates (never in `ita-kernel`)
- Test runner: `cargo test`; `cargo bench` (criterion) for local benchmarks
- Benchmark tooling: local-only spans/benches (no paid eval runs). See `perf` archive.
- Storage: SQLite (bundled, WAL) via `rusqlite`, single writer confined to the server
- Sandbox: Linux first (landlock + seccomp, optional bwrap); `Policy`/`Fidelity`
  matrix must stay cross-platform-expressible
- Models: OpenAI + Anthropic wire formats, native tool calling only
  (single `command` string schema), all LLM traffic proxied by the server
- Platform note: `target/`, `.ita/`, `AGENTS.md` are ignored; `.w1mer/` is committed
