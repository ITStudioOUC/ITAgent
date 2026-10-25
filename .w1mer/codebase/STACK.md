# STACK — Toolchain & Environment

> Stable layer, first in fixed read order. Edit rarely.

- Language: Rust, edition 2024, `rust-version = "1.85"`. Single binary `ita` from
  `crates/ita-cli`; every other crate is a library. Async only in I/O crates —
  never in `ita-kernel`.
- Verify: `bash .w1mer/scripts/verify.sh all` — runs the `fmt`, `lint` and `test`
  stages; `.w1mer/scripts/verify.sh msrv` is a separate stage (not in `all`).
- Build: `cargo build --workspace --locked`
- Format: `cargo fmt --all --check`
- Lint: `cargo clippy --workspace --all-targets --locked -- -D warnings`
- Test: `cargo nextest run --workspace --locked` (a run with zero tests is a failure)
- Docs: `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked` and
  `cargo test --doc --workspace --locked`; both run inside `verify.sh`.
- MSRV: `rust-version = "1.85"`. CI's `msrv` job installs 1.85.0 and runs
  `cargo +1.85.0 check --workspace --all-targets --locked`. Do not use let-chains
  (stable only since 1.88) at this MSRV.
- Kernel purity: `bash .w1mer/scripts/check_kernel_purity.sh` — asserts `ita-kernel` has
  no dependencies at all (including dev), no build script, and no IO/clock/env/thread
  usage. Runs before clippy in CI so it fails fast.
- Layering: `bash .w1mer/scripts/check_layers.sh` — asserts every member's declared
  `ita-*` dependencies match the allowlist in the script, that SQLite only appears in
  `ita-store`, HTTP/TLS only in `ita-server`, sandbox primitives only in `ita-sandbox`,
  and that `ita` is the only registered binary. Metadata-only, no compilation; runs
  before the purity gate and clippy. This is a layering guard, not a security boundary.
  `bash .w1mer/scripts/check_layers_test.sh` feeds it synthetic metadata (no manifest
  changes) to keep the gate's failure paths covered.
- Benchmark: `cargo bench` (criterion), local-only. No paid eval runs — see AGENTS.md §10
  and the `perf` archive.
- Storage: SQLite (bundled, WAL) via `rusqlite`, single writer confined to the server
- Sandbox: Linux first (landlock + seccomp); `Policy`/`Fidelity` must stay
  cross-platform-expressible
- Models: OpenAI + Anthropic wire formats, native tool calling only (single `command`
  string schema), all LLM traffic proxied by the server
- CI: `.github/workflows/ci.yml`, four parallel jobs — `fmt`, `lint`, `test` (the first
  three call the matching `verify.sh` stage) and `msrv` (installs 1.85.0). Build, clippy,
  doc and test steps pass `--locked`; `cargo fmt` does not accept that flag.
- Repo hygiene: `target/` and `.ita/` are ignored; `.w1mer/` and `AGENTS.md` are committed
  (worktrees check out from git, so an uncommitted AGENTS.md would vanish in them)
