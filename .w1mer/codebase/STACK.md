# STACK — Toolchain & Environment

> Stable layer, first in fixed read order. Edit rarely.

- Language: Rust, edition 2024, `rust-version = "1.85"`. Single binary `ita` from
  `crates/ita-cli`; every other crate is a library. Async only in I/O crates —
  never in `ita-kernel`.
- Build: `cargo build --workspace --locked`
- Format: `cargo fmt --all --check`
- Lint: `cargo clippy --workspace --all-targets --locked -- -D warnings`
- Test: `cargo nextest run --workspace --locked` (a run with zero tests is a failure)
- Kernel purity: `bash .w1mer/scripts/check_kernel_purity.sh` — asserts `ita-kernel` has
  no dependencies at all (including dev), no build script, and no IO/clock/env/thread
  usage. Runs before clippy in CI so it fails fast.
- Layering: `bash .w1mer/scripts/check_layers.sh` — asserts every member's declared
  `ita-*` dependencies match the allowlist in the script, that SQLite only appears in
  `ita-store`, HTTP/TLS only in `ita-server`, sandbox primitives only in `ita-sandbox`,
  and that `ita` is the only registered binary. Metadata-only, no compilation; runs
  before the purity gate and clippy. This is a layering guard, not a security boundary.
- Benchmark: `cargo bench` (criterion), local-only. No paid eval runs — see AGENTS.md §10
  and the `perf` archive.
- Storage: SQLite (bundled, WAL) via `rusqlite`, single writer confined to the server
- Sandbox: Linux first (landlock + seccomp); `Policy`/`Fidelity` must stay
  cross-platform-expressible
- Models: OpenAI + Anthropic wire formats, native tool calling only (single `command`
  string schema), all LLM traffic proxied by the server
- CI: `.github/workflows/ci.yml`, three parallel jobs — `fmt`, `lint` (layering gate,
  purity gate, then clippy), `test` (build then nextest). Every cargo step passes
  `--locked`.
- Repo hygiene: `target/` and `.ita/` are ignored; `.w1mer/` and `AGENTS.md` are committed
  (worktrees check out from git, so an uncommitted AGENTS.md would vanish in them)
