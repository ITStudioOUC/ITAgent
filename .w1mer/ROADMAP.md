# ITAgent — Roadmap

> Section order in the file = priority order; `list` and `re-rank` follow file order.
> Within a section, open rows are ranked by `Defer` (desc), then ID.
> `Defer` counts how many times the task was skipped (`w1mer defer <id>`).
> Each completed task: mark `[x]` + date + commit, add a one-line measured
> effect summary and `done`; full data goes to the results archive.
> Insert new items with derived sub-ids (`05.1`, `05.1.2`); never batch-renumber.

## ID rules

- Unbounded rolling hierarchical IDs: `01`, `05`, `05.1`, `05.1.1`, ...
- Children branch deeper on demand (sub-batch fixes, review follow-ups).
- Ordering: pre-order traversal (parent first, children right behind).
- Cross-references stay stable; derived ids link review/results/bug docs.

## Task granularity

Rows at this level are **big tasks** (weeks of work, one clear deliverable
each). Before a task is picked, an Explorer splits it into sub-tasks
(`NN.1`, `NN.2`, ...) with explicit file/contract boundaries. Do not add
top-level rows for work that fits inside an existing big task.

## Status states

`todo` `done` `reviewed` `issue`

## 一、Foundation

Foundation tasks are hard prerequisites: everything after them codes against the
contracts they freeze. Nothing else opens before 01 and 02 are done.

- **01** freezes the design in `docs/adr/` ADRs (including rejected
  alternatives) so later batches never re-litigate settled questions.
- **02** creates the workspace shape (`ita-kernel` pure layer first) so the
  separation is enforced by the compiler from day one.

| # | Task | Doc | Status | Defer | Effect |
|---|------|-----|--------|-------|--------|
<!-- w1mer:task:foundation -->
| 01 | 设计决策固化：docs/adr 架构决策记录全套 | — | done | 0 | 十三份架构决策记录落在 docs/adr，覆盖内核边界、单库单写者、上下文树与解析、压缩、两层回滚、内容寻址快照、受限文法、原生工具调用、租约分层、沙箱保真度、工作流事件溯源、工具信任链、unit 监督；README 已按决策重写 |
| 01.1 | 审查架构决策记录的内部一致性与可执行性 | — | todo | 0 | |
| 02 | 仓库骨架：cargo workspace + ita 单二进制 + CI | — | todo | 0 |



## 二、Kernel

Pure logic only (`ita-kernel`): no IO, no async, no DB. This is where the
"restrained core" claim is either true or false.

- **03** lexer: argv + pipe + redirect + heredoc `<<` (single command, quoted
  delimiter preferred), full-body-then-write, failure matrix as tests.
- **04** context: node tree (DAG, explicit head), `resolve()` as a pure function,
  frame weak refs, three-state drift notice, read-set tracking.
- **05** semantic compression: transactional head switch, keep last 3 turns,
  machine-filled facts + model-written prose, compression call logged.

| # | Task | Doc | Status | Defer | Effect |
|---|------|-----|--------|-------|--------|
<!-- w1mer:task:kernel -->
| 03 | 受限 bash 文法：Command AST + lexer + heredoc + 失败矩阵 | — | todo | 0 |
| 04 | 上下文模型：Node/Frame/resolve/三态 notice/读集 | — | todo | 0 |
| 05 | 语义压缩：事务性切换 + 保留 3 轮 + 事实层 | — | todo | 0 |


## 三、Store & Server

Single DB, server is the only writer, workers are stateless and DB-blind.

- **06** schema + CAS: `blobs` global content-addressed store, `snapshots` /
  `snapshot_entries` (with tombstones), baseline + squash, mark-sweep GC with grace.
- **07** units: dependency topology (`Requires`/`Wants`/`After`), `ready` vs
  `running`, required units gate the CLI, the rest load asynchronously.
- **08** server I/O: `Invocation` envelope (caller, session, cwd, argv, stdin,
  lease ids), two message classes (authoritative-sync vs telemetry), group commit.

| # | Task | Doc | Status | Defer | Effect |
|---|------|-----|--------|-------|--------|
<!-- w1mer:task:server -->
| 06 | 存储层：单 DB schema + CAS + 快照 squash + GC | — | todo | 0 |
| 07 | unit 监督者：依赖拓扑 + ready 定义 + 渐进加载 | — | todo | 0 |
| 08 | server 写路径与 IPC：Invocation 信封 + 两类消息 + group commit | — | todo | 0 |


## 四、Execution & Sandbox

The worker talks only to the executor; the executor consults the server for
leases and delegates to the sandbox. Nested calls (bash → tool) inherit the
outer lease and never acquire new paths.

- **09** lease: one tool call, atomic multi-path acquisition (all-or-nothing,
  sorted), TTL + heartbeat, stale-write hash check, cancellation to pgid,
  snapshot only after reap.
- **10** sandbox: `Policy` (declarative, cross-platform expressible) vs
  `Fidelity` (honest capability matrix, deny-by-default on missing dimensions).
- **11** tools: AUR-style manifest, `ita-tool-shim` ABI, install-time approval
  bound to (name, version, content hash, platform), permission diff on upgrade.

| # | Task | Doc | Status | Defer | Effect |
|---|------|-----|--------|-------|--------|
<!-- w1mer:task:execution -->
| 09 | 执行器：单次租约 + 多路径原子获取 + 取消传播 | — | todo | 0 |
| 10 | Linux 沙箱：landlock/seccomp + Policy/Fidelity 能力矩阵 | — | todo | 0 |
| 11 | 工具系统：manifest + shim ABI + 包管理器 + 首次批准 | — | todo | 0 |


## 五、Intelligence

- **12** models: OpenAI + Anthropic, single static `{command: string}` tool schema,
  native tool calling only, thinking blocks passed through opaque, usage logged,
  all requests proxied by the server (workers and sandboxes never reach the network).
- **13** tool retrieval (optional, default off): BM25 v1 → rerank → selector as a
  hint only, output frozen into the event log, feedback signal recorded.

| # | Task | Doc | Status | Defer | Effect |
|---|------|-----|--------|-------|--------|
<!-- w1mer:task:intelligence -->
| 12 | 模型层：双 provider 适配 + native codec + usage 账本 + server 代理 | — | todo | 0 |
| 13 | 工具检索（可选）：BM25→rerank→selector + 反馈落库 | — | todo | 0 |


## 六、Orchestration

DSL is optional for v1, but subagent semantics must be reserved in the kernel now
(sessions fork, results merge by explicit append, never auto-merge).

- **14** workflow: JSON schema, node completion recorded as events (state derived,
  signals are only latency optimization), static write-set conflict lint at load
  and pull time.
- **15** subagent: two inheritance modes (compressed brief / inherit-context +
  tail instruction), fork-not-append, depth and budget caps.
- **16** node scheduling: declared write-set resource classes (files/trees/ports/db),
  concurrency allowed until first write, declared-vs-actual audit.

| # | Task | Doc | Status | Defer | Effect |
|---|------|-----|--------|-------|--------|
<!-- w1mer:task:orchestration -->
| 14 | 工作流 DSL：JSON schema + 事件溯源状态 + 静态冲突 lint | — | todo | 0 |
| 15 | subagent：两种继承模式 + 分叉不合并 + 深度预算 | — | todo | 0 |
| 16 | 节点调度：独占声明 + 读写分离 + 声明/实际审计 | — | todo | 0 |


## 七、Quality & Ops

Only local, zero-cost measurement. No paid eval runs: real sessions are the dataset.

- **17** instrumentation: per-turn latency breakdown, cold-start-to-ready, tool path
  (executor→sandbox→shim→reap→snapshot), IPC RTT; usage fields are free from the
  provider and must be logged.
- **18** export/import: workspace bundle (metadata rows + referenced CAS blobs +
  snapshot manifests) — replaces "copy the directory" now that the DB is shared.

| # | Task | Doc | Status | Defer | Effect |
|---|------|-----|--------|-------|--------|
<!-- w1mer:task:quality -->
| 17 | 埋点与基准：turn 延迟分解 + 冷启动 ready + 工具路径 | — | todo | 0 |
| 18 | 工作区 export/import：搬移、备份与 replay | — | todo | 0 |


## 八、Backlog

(Items awaiting re-prioritization)

- 19 remote transport (WS/HTTP, cursor resume) — waits for the local IPC envelope
  to prove stable; topology (a) already fixed.
- 20 Postgres adapter — waits for real load data; operation-level `Store` trait only.
- 21 TUI — owner designs the frontend personally; CLI only until then.
- 22 event archival — only when a workspace exceeds 200 MB events or `load_context`
  exceeds 20 ms. `EventRange` trait is the seam left open; nothing else now.

<!-- w1mer:task:backlog -->
| 19 | 远程传输：WS/HTTP + 事件游标续传（拓扑 a） | — | todo | 0 |
| 20 | Postgres 适配层：操作级 Store trait | — | todo | 0 |
| 21 | TUI：owner 亲自设计前端 | — | todo | 0 |
| 22 | 事件归档：200MB / 20ms 判据触发后再做 | — | todo | 0 |
