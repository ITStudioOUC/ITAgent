# ita-cli — the `ita` binary (composition root)

Path: `crates/ita-cli`. The only crate in the workspace that produces a binary,
named `ita` (registered in `crates/ita-cli/Cargo.toml` under `[[bin]]`).
Everything else is a library; this crate is where a process becomes a role.

## Responsibilities

- Parse the single role argument and dispatch to the corresponding subsystem.
  Role parsing lives in `crates/ita-cli/src/role.rs`; dispatch lives in
  `crates/ita-cli/src/main.rs`.
- Handle `--version` / `-V`, printing `ita <version>` where the version comes
  from `env!("CARGO_PKG_VERSION")` (the workspace-inherited version in
  `crates/ita-cli/Cargo.toml`).
- Own the binary's exit-code contract: version and known roles exit 0;
  unknown role exits 2 with usage on stderr.
- Be the composition root: when a role's subsystem lands, wire it in here.
  The three roles are currently placeholders printing
  `角色 <name> 尚未实现` to stderr and exiting 0.

## Key types & abstractions

- `Role` (`crates/ita-cli/src/role.rs`) — `enum Role { Master, Worker, ToolHost }`,
  `Debug + Clone + Copy + PartialEq + Eq`.
  - `Role::parse(arg: Option<&str>) -> Option<Role>` — `None` or `"master"`
    default to `Master`; `"worker"` and `"tool-host"` map to their variants;
    anything else returns `None`. The caller prints usage and exits 2.
  - `Role::as_str(self) -> &'static str` — the command name for error
    messages.
- `main() -> ExitCode` (`crates/ita-cli/src/main.rs`) — reads
  `std::env::args().nth(1)` only. No env vars, no config files, no flags
  beyond the version pair.
- `USAGE` (`crates/ita-cli/src/main.rs`) — the one-line usage string
  `用法：ita [master|worker|tool-host]`, printed on unknown role.

## Internal data flow

1. `main` reads `std::env::args().nth(1)`.
2. If it is `--version` or `-V`, print `ita {CARGO_PKG_VERSION}` to stdout,
   return `ExitCode::SUCCESS`.
3. Otherwise `Role::parse` maps the argument to `Role`; on `None`, print
   `未知角色。{USAGE}` to stderr and return `ExitCode::from(2)`.
4. The `match` dispatches per role. All three arms are currently the same
   placeholder: `eprintln!("角色 {} 尚未实现", role.as_str())`, then
   `ExitCode::SUCCESS`.

When a subsystem lands, replace the placeholder arm with a call into that
crate's entry point (e.g. `ita_server::run()` for `Master`). Keep the
`match` exhaustive over `Role` so a new role variant cannot silently vanish.

## Contracts

### Provides

- The `ita` binary — the single process entry point for all three roles.
  No library crate may depend on `ita-cli` (enforced by
  `.w1mer/scripts/check_layers.sh`); the binary is the composition root.
- Exit-code contract: 0 for version and every known role (including the
  current placeholders); 2 for unknown role. Usage text goes to stderr.
- `tests/cli.rs` pins the contract end-to-end by running the compiled binary
  via `CARGO_BIN_EXE_ita`: version must equal the workspace version read from
  the root `Cargo.toml` (not the test crate's own version, so a missed bump
  fails); `-V` and `--version` must agree; no-args starts as master;
  placeholder notices go to stderr with stdout empty; unknown role exits 2
  with `用法：ita` on stderr.

### Consumes

- Nothing at present — `[dependencies]` in `crates/ita-cli/Cargo.toml` is
  empty. The layering allowlist grants `ita-cli` edges to all ten library
  crates, but none are used yet.
- When wiring roles, consume only library crates' entry points:
  `ita-server` for `Master`, `ita-worker` for `Worker`, `ita-tools` for
  `ToolHost`. Do not reach into a subsystem's internals from here.

## Invariants

- One binary, one role argument. Role comes from the argument only — never
  from environment variables, which are shared across workspaces and would
  make the role unpredictable (`crates/ita-cli/src/role.rs` module comment).
- Default role is `Master`: `ita` with no args is the server, not an error.
- Unknown role is a hard error (exit 2 with usage), never a silent default.
- Placeholder notices go to stderr; stdout stays clean for piping.
- The binary carries no business logic: dispatch and version only. If a
  subsystem grows, it grows in its own crate, not here.
- `crates/ita-cli/Cargo.toml` inherits `version`, `edition`, `rust-version`,
  `license` from the workspace; the `[[bin]]` target name stays `ita`.
- MSRV 1.85: `let-else` is fine (1.65); let-chains are not (1.88).
