# FEATURE — 功能实现的验证结果

> 每完成一个 ROADMAP 行追加一条记录。ROADMAP 行只留一行摘要，完整数据写在这里。
> 字段固定：任务号、提交与日期、验证命令与退出码、测试数量、结论。

## 06.3 事件追加、seq 分配与预览截断

结论：事件追加与 seq 计数器分配落地，预览按头 4 KiB 尾 1 KiB 截断并回退到字符边界。
seq 从 `workspace_seq` 计数器读改写分配（不用 `MAX(seq)+1`），分配与插入在同一立即事务内；
事件引用的会话与节点在事务内用 `SELECT EXISTS` 校验归属，不通过则回滚、计数器不消耗。
事件 id 放内核 `ids.rs`，复用既有 `id_type!` 校验，存储层用 `ulid` crate 生成。
计划第四节的 14 条验收全部写成测试并通过，五项验证退出码全为 0。

### 提交

| 提交 | 主题 | 改动 |
|---|---|---|
| `6aa9885` | 事件正文预览截断与事件 id 类型 | `preview.rs` 新建 `PreviewLimit` 与截断函数（含 4 个单元测试）；内核 `ids.rs` 加 `EventId` 并在 `context/mod.rs` 导出；`lib.rs` 声明 `preview` 模块 |
| `6a4a150` | 事件追加、seq 计数器分配与归属校验 | `seq.rs` 计数器读改写、`events.rs` 追加与读路径、`db.rs` 补 `conn_mut` 与 `append_event`/`events_after` 及两个归属错误变体、`lib.rs` 接线；`tests/events.rs` 14 条集成测试 |

### 预览截断的边界

| 正文长度 | 头 | 尾 | 说明 |
|---|---|---|---|
| ≤ 5120 字节 | 完整正文 | 空 | 不截断，`preview_tail` 为 NULL |
| 5120 字节 | 完整正文 | 空 | 恰好等于总预算，不截断 |
| 5121 字节 | 4096 字节 | 1024 字节 | 超过总预算，切两段 |
| 9000 字节（全中文） | 4095 字节 | 1023 字节 | 4096 不是 3 的倍数，首段退到 4095；尾段从 7977 字节起 |

判据是"头加尾"总预算（默认 5120 字节），不是两段各自的上限。正文落在 4096 与 5120 之间
时不机械取"前 4096 后继 1024"，而是整段留下不切两段。

### seq 分配

seq 从 `workspace_seq` 计数器读改写分配，不用 `MAX(seq) + 1`。第 3 条验收把计数器预置成
100，正确实现得到 100，`MAX` 实现得到 4。分配与插入事件在同一立即事务内，写事务失败时
计数器不被消耗（第 12 条），取不到作用域时计数器不变（第 13 条）。

### 验证

五项验证都在最终提交内容上跑过，退出码全为 0：

| 命令 | 退出码 | 结果 |
|---|---|---|
| `cargo nextest run --workspace --locked` | 0 | 283 项通过，0 项跳过 |
| `cargo build --workspace --locked` | 0 | 全工作区构建通过 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 零告警 |
| `cargo fmt --all --check` | 0 | 无需格式化 |
| `bash .w1mer/scripts/check_kernel_purity.sh` | 0 | 内核纯净门通过 |
| `bash .w1mer/scripts/verify.sh msrv`（1.85.0） | 0 | MSRV 检查通过 |

新增 18 个测试：`preview.rs` 4 个单元测试、`tests/events.rs` 14 个集成测试。`ita-store`
从 34 项增至 52 项，全仓从 265 项增至 283 项。

### 自查

14 条验收逐条对应测试：第 1 至 4 条与第 10 至 13 条测 seq 与 id，第 5 至 9 条测预览截断，
第 14 条测会话与节点归属。事件 id 用 `ulid` crate 生成（26 字符 Crockford base32，首位落在
0 到 7），能被 `EventId::parse` 接受，两条事件 id 不同。无正文的事件四列全 NULL 而非空串。

## 06.2.2 测试用例命名英文化与命名规范入册（第一批：规范与命令文法）

结论：规范已入册，命令文法模块的 80 个中文用例名全部改为英文 snake_case 并各补一行
中文行为说明；改名前后测试总数都是 261 项，逐 crate 数量不变，五项验证全部通过。
本行只完成四个模块组中的第一组，其余三组由同一批次的后续提交推进，因此 ROADMAP 状态
暂为 active，不写 done。

### 提交

| 提交 | 主题 | 改动 |
|---|---|---|
| `1a418d6` | 规定测试函数名用英文并在上方写一行中文行为说明 | `COMMENT_GUIDELINES.md` 新增第六节并把原第六节顺延为第七节，第一节与第二节各加一处指向；`HANDOFF.md` 的旧口径替换为新规则 |
| `9a5a237` | 命令文法用例改为英文名并补行为说明 | 命令文法六个文件共 80 个用例改名并补说明行 |

### 各文件用例数

| 文件 | 改名前中文名 | 改名后英文名 |
|---|---|---|
| `crates/ita-kernel/src/command/error.rs` | 4 | 4 |
| `crates/ita-kernel/src/command/ast.rs` | 9 | 9 |
| `crates/ita-kernel/src/command/lex.rs` | 21 | 21 |
| `crates/ita-kernel/src/command/heredoc.rs` | 20 | 20 |
| `crates/ita-kernel/src/command/parse.rs` | 21 | 21 |
| `crates/ita-kernel/tests/command_matrix.rs` | 5 | 5 |
| 合计 | 80 | 80 |

### 验证

五项验证都在最终提交内容上跑过，退出码全为 0：

| 命令 | 退出码 | 结果 |
|---|---|---|
| `cargo nextest run --workspace --locked` | 0 | 261 项通过，0 项跳过 |
| `cargo build --workspace --locked` | 0 | 全工作区构建通过 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 零告警 |
| `cargo fmt --all --check` | 0 | 无需格式化 |
| `bash .w1mer/scripts/check_kernel_purity.sh` | 0 | 内核纯净门通过 |

用例总数在改名前后都是 261 项，逐 crate 分布也一致：`ita-kernel` 217 项、
`ita-kernel::command_matrix` 5 项、`ita-store` 14 项、`ita-store::schema` 16 项、
`ita-cli::cli` 6 项、`ita-cli::bin/ita` 3 项。

### 自查

改名前用 `grep -rnP '^\s*fn .*[\x{4e00}-\x{9fff}]' crates --include=*.rs` 取到 249 行清单，
改名后同一命令在 `crates/ita-kernel/src/command` 与 `crates/ita-kernel/tests` 下零命中，
全仓剩余 169 条属于上下文核心、语义压缩、存储与命令行四组，留给本批次的后续提交。
另核对 80 个改名后的 `#[test]` 上方都紧贴一行 `///`，且函数名都是 `[a-z0-9_]` 组成的
合法 snake_case 标识符，无一遗漏。

## 07.1 拓扑类型与依赖无环校验

结论：unit 拓扑类型与依赖无环校验落地，三种依赖（`Requires`/`Wants`/`After`）方向语义
一致，无环校验对三种边的并集判定，任何环都致命并返回闭合环路径，稳定启动顺序是词典序
最小的拓扑序。类型落在 `ita-server` 而非内核，unit 类型与工作流节点类型靠类型分开。
`lease ↔ tools` 的双向记号落成单方向边（`lease` 依赖 `tools`），在模块文档注释里写明这条
裁决。07.1 零新增依赖、不建表、不碰 tokio。计划第四节的十条验收全部写成测试并通过，
验证命令退出码全为 0。

### 提交

| 提交 | 主题 | 改动 |
|---|---|---|
| `eba64fc` | unit 拓扑类型与三种依赖的无环校验和稳定启动顺序 | `units/mod.rs` 新建 `UnitId` 标签名义类型（解析校验、`Ord`/`Hash` 派生）与 `DepKind` 三变体，模块文档注释写明 `lease ↔ tools` 落单方向的裁决；`units/topology.rs` 新建 `Topology`（unit 集合加边集合）、`add_requires`/`add_wants`/`add_after`、`find_cycle`（DFS 三色标记）、`startup_order`（Kahn 加最小堆）、`CycleError`（带环路径），内联测试覆盖十条验收；`lib.rs` 声明 `units` 模块 |

### 类型与算法

`UnitId` 是校验过的标签名义类型：小写字母数字加连字符，非空，首尾不连字符，不连续连字符，
与 systemd unit 命名同型。`Ord` 是标签词典序，供最小堆排序；`Hash` 供边的集合去重。内核的
`id_type!` 宏是 crate 私有的，`ita-server` 不能复用，`UnitId` 在 `units/mod.rs` 自定义。

`DepKind` 是三个变体：`Requires`（硬依赖，失败则连带失败）、`Wants`（软依赖，失败只是降级）、
`After`（仅排序，不隐含依赖）。三种边在图结构里方向完全相同：`X` 依赖 `Y`（无论哪种种类）
都产生 `Y → X` 的边，即被依赖者 `Y` 先启动。种类只记录在边上，供失败传播与启动门消费，
无环校验与启动排序不消费种类。

`Topology` 是 unit 集合加有向边集合，边是三元组 `(target, dependent, kind)`，含义是
`dependent` 依赖 `target`，`target` 先启动。`find_cycle` 用 DFS 三色标记在三种边的并集图上
找环，撞灰节点即从栈顶回溯到灰节点、再补回起点，形成闭合环（首元素等于尾元素）。环路径按
依赖方向表达，归一为从环中最小标签起，使同一个环无论由哪个灰节点触发检测都给出同样的表示。
`startup_order` 先调 `find_cycle`，有环则返回 `CycleError`（带环路径），无环则做 Kahn 拓扑排序
加按 unit 标签排序的最小堆，输出是所有合法拓扑序里词典序最小的那一个。

### 边界与裁决

- `lease ↔ tools` 的双向记号落成单方向边：`lease` 依赖 `tools`，`tools` 先启动。按字面建成
  互相 `Requires` 会自己成环，被自己的无环校验拒绝。
- 无环校验对三种边的并集判定，任何环都致命。`Wants` 环与 `After` 环同样被检出，不是只对
  `Requires`。
- 自依赖是环，环路径为 `[A, A]`。
- 重复边幂等：同一对 unit 同种类的边加两次不重复计数。
- 同一对 unit 多种类边合法：不冲突。
- 空拓扑与孤立 unit 合法。

### 验证

| 命令 | 退出码 | 结果 |
|---|---|---|
| `cargo build --workspace --locked` | 0 | 全工作区构建通过 |
| `cargo nextest run --workspace --locked` | 0 | 413 项通过，0 项跳过（新增 18 项） |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 零告警 |
| `cargo fmt --all --check` | 0 | 无需格式化 |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked` | 0 | 文档构建通过 |
| `cargo test --doc --workspace --locked` | 0 | doctest 通过 |
| `bash .w1mer/scripts/check_layers.sh` | 0 | 分层门通过 |
| `bash .w1mer/scripts/check_kernel_purity.sh` | 0 | 内核纯净门通过 |
| `cargo +1.85.0 check --workspace --all-targets --locked` | 0 | MSRV 通过 |

`shellcheck` 在本环境缺失，而本批未改动任何 shell 脚本，故 lint 阶段的 shellcheck 子步骤
未执行；其余 lint 子步骤（分层门、纯净门、clippy、rustdoc）全部通过。

### 新增测试分布

新增 18 项测试，全部内联在 `ita-server` 的 `units` 模块：`mod.rs` 的 `UnitId` 解析校验与
`Ord`/`Hash` 派生 8 项，`topology.rs` 的十条验收 10 项。全仓测试总数从 395 项增至 413 项。

## 07.2 状态机、就绪探针与停止顺序

结论：unit 状态机、就绪探针与监督者落地。六状态机是纯转移函数，非法转移返回 `None` 而不
是原地不动，"就绪不等于运行"由 `Starting`（进程活着但未就绪）与 `Ready`（探针通过、功能
可用）的区分承载。就绪探针是三值结果（就绪/未就绪/失败）加可注入 trait，探针是"功能可用"
判定，不是"进程活着"判定。监督者是同步的，按拓扑序启动（一个 unit 全部依赖 `Ready` 才启
动）、按逆拓扑序停止；失败的单位永不重启、其依赖者永不启动（状态机层面与监督者层面双层
保障）。07.2 零新增依赖、不建表、不碰 tokio。计划第四节的十条验收全部写成测试并通过，验
证命令退出码全为 0。

### 提交

| 提交 | 主题 | 改动 |
|---|---|---|
| `4c2742b` | unit 状态机、就绪探针与监督者按拓扑序启动逆序停止 | `units/state.rs` 新建 `UnitState` 六变体、`UnitEvent` 六变体与纯转移函数 `next`；`units/probe.rs` 新建 `ProbeResult` 三变体与 `ReadinessProbe` trait；`units/supervisor.rs` 新建 `Supervisor`（拓扑、状态表、探针）、`step`、`run_until_blocked`、`shutdown`、查询方法、`StepOutcome`/`ShutdownResult`；`units/mod.rs` 声明三模块并再导出关键类型；`units/topology.rs` 加只读 `dependencies_of` 访问器 |

### 状态机、就绪探针与监督者

`UnitState` 是六个变体：`NotStarted`、`Starting`、`Ready`、`Failed`、`Stopping`、`Stopped`。
转移函数 `next(state, event) -> Option<UnitState>` 是纯函数，非法转移返回 `None`。合法转移
边是：未启动加 `Start` 到启动中；启动中加 `ProbeReady` 到就绪；启动中加 `ProbeFailed` 到失
败；启动中加 `ProbeNotReady` 到启动中（原地重试）；就绪加 `Stop` 到停止中；停止中加
`Stopped` 到已停止。其余组合都返回 `None`。

`ProbeResult` 是三个变体：`Ready`、`NotReady`、`Failed`。`ReadinessProbe` trait 的唯一方法
是 `probe(&self, unit: &UnitId) -> ProbeResult`。监督者持有 `Box<dyn ReadinessProbe>`，测试
注入假探针。探针是"功能可用"判定，不是"进程活着"判定，真实实现留后。

`Supervisor` 持有 `topology: Topology`、`states: HashMap<UnitId, UnitState>`、
`probe: Box<dyn ReadinessProbe>`。构造时所有 unit 初始化为 `NotStarted`。`step` 按启动顺序
遍历，找到第一个 `NotStarted` 且全部依赖都 `Ready` 的 unit，转移到 `Starting` 并探针，按探针
结果转移；`Starting` 的 unit 会被重新探针。`StepOutcome` 是 `Progressed(UnitId)` 或
`Blocked`。`run_until_blocked` 反复调 `step` 直到 `Blocked`。`shutdown` 按启动顺序的逆序遍历，
对每个 `Ready` 的 unit 执行 `Ready → Stopping → Stopped`。

### 边界与裁决

- `Failed` unit 永不重启、其依赖者永不启动。双层保障：状态机层面 `next(Failed, Start)` 返回
  `None`；监督者层面 `Failed` unit 不是 `NotStarted`，不会被重新选中，其依赖者因依赖未
  `Ready` 而永不启动。
- 失败不阻塞独立 unit：一个 unit 的失败只影响其依赖者，不影响独立 unit。
- 一个 unit 只有在全部依赖都 `Ready` 时才启动。依赖是 `Starting`（未就绪）时依赖者不启动。
- `shutdown` 只停 `Ready` 的 unit；`Failed`、`NotStarted`、`Stopped` 的 unit 跳过。
- 监督者需要读依赖边，而 07.1 的 `Topology` 不暴露边。因此在 `topology.rs` 加只读
  `dependencies_of` 访问器。这是对 07.1 产出的最小增补，不改环检测与启动顺序的既有行为。

### 验证

| 命令 | 退出码 | 结果 |
|---|---|---|
| `cargo build --workspace --locked` | 0 | 全工作区构建通过 |
| `cargo nextest run --workspace --locked` | 0 | 430 项通过，0 项跳过（新增 17 项） |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 零告警 |
| `cargo fmt --all --check` | 0 | 无需格式化 |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked` | 0 | 文档构建通过 |
| `cargo test --doc --workspace --locked` | 0 | doctest 通过 |
| `bash .w1mer/scripts/check_layers.sh` | 0 | 分层门通过 |
| `bash .w1mer/scripts/check_kernel_purity.sh` | 0 | 内核纯净门通过 |

### 新增测试分布

新增 17 项测试，全部内联在 `ita-server` 的 `units` 模块：`state.rs` 的转移函数 8 项（含非法
转移被拒、失败 unit 不可重启），`probe.rs` 的假探针 1 项，`supervisor.rs` 的十条验收 8 项。
全仓测试总数从 413 项增至 430 项。

## 07.3 必需与非必需启动门与降级

结论：启动门与启动计划落地，unit 状态落库。`StartupPlan` 声明必需与非必需 unit 集并与拓扑
交叉校验（必需集非空、两集不相交、全部 unit 在拓扑里），校验失败返回显式错误不静默继续。
`activate` 是纯函数，只认 `Ready` 不认 `Starting`：必需 unit 的 `Starting` 与 `Failed` 计入未
就绪，`NotStarted` 是被依赖阻塞不计入未就绪；非必需 unit 的 `Failed`/`NotStarted`/`Starting`
计入降级不阻塞激活。`units` 表迁移只加不改，`ita-store` 加 unit 状态读写操作（`set_unit_state`/
`read_unit_state`/`read_all_unit_states`），`UnitState` 三值域（`ready`/`degraded`/`failed`）是
六状态状态机的投影。十条验收全过，449 项测试全绿，验证命令退出码全为 0。

### 提交

| 提交 | 主题 | 改动 |
|---|---|---|
| `f87af45` | 必需与非必需启动门与降级，unit 状态落库 | `crates/ita-server/src/units/plan.rs` 新建 `StartupPlan`（`required`/`optional` 两集合、构造与校验）、`crates/ita-server/src/units/activation.rs` 新建 `ActivationResult` 两变体与 `activate` 纯函数、`crates/ita-server/src/units/mod.rs` 声明两模块并再导出关键类型、`crates/ita-store/migrations/0003_units.sql` 新建 `units` 表迁移、`crates/ita-store/src/units.rs` 新建 unit 状态读写操作与 `UnitState` 三值域、`crates/ita-store/src/db.rs` 加 `InvalidUnitState` 变体与三个 `Store` 方法、`crates/ita-store/src/lib.rs` 声明 `units` 模块并再导出 `UnitState`、`crates/ita-store/src/migrate.rs` 加 version 3 迁移条目 |

### 启动计划与启动门

`StartupPlan` 是两个集合：`required: BTreeSet<UnitId>`（必需 unit，激活门等待它们全部就绪）
与 `optional: BTreeSet<UnitId>`（非必需 unit，后台异步加载，失败只标记降级）。计划是静态声明，
在启动前建好，不动态建行。计划与拓扑交叉校验：计划里的每个 unit 必须在拓扑里（必需集非空、
两集不相交、全部 unit 在拓扑里），校验失败返回显式错误。

`activate(supervisor: &Supervisor, plan: &StartupPlan) -> ActivationResult` 是纯函数：对每个
必需 unit，状态是 `Ready` 计入就绪列表，`Starting` 与 `Failed` 计入未就绪列表，`NotStarted` 是
被依赖阻塞不计入未就绪列表；对每个非必需 unit，状态是 `Failed`/`NotStarted`/`Starting` 计入
降级列表，`Ready` 不计。未就绪列表为空则返回 `Ready`（带降级列表），否则返回 `NotReady`（带未
就绪列表）。门不改变状态，只判定。降级标记是门之后的独立步骤（server 拿到 `Ready` 结果后写库），
与判定分离。

### `units` 表与 unit 状态落库

`units` 表结构是 `id TEXT PRIMARY KEY`、`state TEXT NOT NULL`、`updated_at INTEGER NOT NULL`。
`state` 是字符串不是枚举，取值域是 `ready`/`degraded`/`failed`。`updated_at` 是 epoch 毫秒。
`UnitState` 是三个变体：`Ready`、`Degraded`、`Failed`，是六状态状态机的投影（未启动、停止中、
已停止是瞬态，不落库）。`set_unit_state` 是 upsert（`ON CONFLICT(id) DO UPDATE`），
`read_unit_state` 返回 `Option<UnitState>`（缺失 unit 返回 `None`），`read_all_unit_states` 返回
`Vec<(String, UnitState)>` 按 id 排序。`StoreError::InvalidUnitState(String)` 变体用于防御性解析
域外状态字符串。

### 边界与裁决

- 门只认 `Ready`，不认 `Starting`。一个必需 unit 进程活着（`Starting`）但探针未通过，门返回
  未就绪。非必需 unit 的 `Starting` 计入降级（还在启动中，功能未可用），不是未就绪。
- `NotStarted` 的必需 unit 是被依赖阻塞，不是独立未就绪，不计入未就绪列表。这与计划第四节
  第 1 条验收一致：`router` 与 `ipc` 因依赖未就绪保持 `NotStarted`，不计入未就绪列表。
- 七 unit 最小拓扑的 `telemetry` 用 `Failed` 而非 `NotReady`：`NotReady` 保持 `Starting`，而
  `step()` 按启动顺序先碰到 `Starting` 的 telemetry 就重探针并返回，`tools` 与 `lease` 永远启动
  不了，`run_until_blocked` 也永不终止。`Failed` 是终态，不重探针，`tools` 与 `lease` 能推进到
  就绪。非必需 unit 的 `Starting` 计入降级由 `optional_not_ready_counts_as_degraded` 单独覆盖。
  这是计划第四节第 10 条验收里 `telemetry` 返回 `NotReady` 的口径与监督者 `step()` 行为的冲突，
  裁决为 `Failed` 以保住"全部必需 unit 就绪"的验收预期。

### 验证

| 命令 | 退出码 | 结果 |
|---|---|---|
| `cargo build --workspace --locked` | 0 | 全工作区构建通过 |
| `cargo nextest run --workspace --locked` | 0 | 449 项通过，0 项跳过（新增 19 项） |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 零告警 |
| `cargo fmt --all --check` | 0 | 无需格式化 |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked` | 0 | 文档构建通过 |
| `cargo test --doc --workspace --locked` | 0 | doctest 通过 |
| `bash .w1mer/scripts/check_layers.sh` | 0 | 分层门通过 |
| `bash .w1mer/scripts/check_kernel_purity.sh` | 0 | 内核纯净门通过 |
| `shellcheck --version` | 0 | shellcheck 0.10.0 可用 |
| `shellcheck .w1mer/scripts/*.sh` | 0 | shell 脚本静态检查通过 |

### 新增测试分布

新增 19 项测试：`ita-server` 的 `plan.rs` 6 项（计划与拓扑交叉校验、必需集非空、两集不相交、
七 unit 最小计划通过校验）、`activation.rs` 6 项（必需未就绪返回未就绪、全部必需就绪返回就绪、
非必需失败只标记降级、非必需未就绪计入降级、就绪不等于运行、七 unit 最小拓扑激活与降级）；
`ita-store` 的 `units.rs` 7 项（unit 状态落库、读回、缺失 unit 返回 None、全部 unit 状态按 id
排序、域外状态字符串被拒、upsert 覆盖、`Store` 方法接线）。全仓测试总数从 430 项增至 449 项。

## 07.4 能力集快照与变更推送

结论：能力集快照与进程内变更推送落地。`CapabilitySet` 带 `version`/`ready`/`degraded` 三字段，
`build_capability_set` 纯函数把 `units` 表读出的 unit 状态投影成能力集（`ready` 进就绪列表，
`degraded` 与 `failed` 进降级列表，两列表按标签排序）。`CapabilityWatcher` 持 `Store` 句柄，
`subscribe` 注册订阅者并立即推完整能力集（版本 0），`notify_change` 读表重建、版本号递增、
向全部订阅者推带版本号的完整能力集。计划第四节的十条验收全部写成测试并通过，五条裁决逐条
守住：能力集读 `units` 表不读内存；版本号是进程内计数器不落库（本批零新增迁移）；推送走
进程内 watch 通道不引入 tokio；"带版本号的增量"是完整能力集加递增版本号，不是真 delta；
工具目录追加不混进本批，watch 通道设计成可承载（消息类型是通用 `CapabilityUpdate` 枚举，
能力集变更是其中一种消息）。

### 提交

| 提交 | 主题 | 改动 |
|---|---|---|
| `5e46617` | 能力集快照与进程内变更推送，unit 状态投影成能力集 | `capability.rs` 新建 `CapabilitySet` 与 `build_capability_set`（含 6 项内联测试）；`watch.rs` 新建 `CapabilityWatcher`、`CapabilityUpdate` 与 `subscribe`/`notify_change`（含 6 项内联测试）；`mod.rs` 声明两模块并再导出关键类型，模块头注释同步 |

### 验收覆盖

| 验收 | 用例 |
|---|---|
| 能力集是 unit 就绪状态的投影 | `capability_set_projects_unit_states` |
| 订阅者先收完整能力集 | `subscribe_pushes_full_capability_set_at_version_zero` |
| 降级后收带版本号的增量 | `notify_change_pushes_version_incremented_set` |
| 版本号单调递增 | `version_is_monotonically_increasing` |
| 就绪与降级区分 | `ready_and_degraded_lists_are_disjoint` |
| 失败 unit 计入降级 | `failed_unit_counts_as_degraded` |
| 能力集变更推送给已连接客户端 | `all_subscribers_receive_the_push` |
| 推送与工具目录追加是同一套机制 | `channel_carries_generic_capability_update` |
| 七 unit 最小拓扑能力集 | `seven_unit_minimal_topology_capability_set` |
| 能力集读 `units` 表 | `capability_set_is_rebuilt_from_units_table` |

### 验证

| 命令 | 退出码 | 结果 |
|---|---|---|
| `cargo build --workspace --locked` | 0 | 全工作区构建通过 |
| `cargo nextest run --workspace --locked` | 0 | 461 项通过，0 项跳过（新增 12 项） |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 零告警 |
| `cargo fmt --all --check` | 0 | 无需格式化 |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked` | 0 | 文档构建通过 |
| `cargo test --doc --workspace --locked` | 0 | doctest 通过 |
| `bash .w1mer/scripts/check_layers.sh` | 0 | 分层门通过 |
| `bash .w1mer/scripts/check_kernel_purity.sh` | 0 | 内核纯净门通过 |
| `bash .w1mer/scripts/verify.sh msrv` | 0 | 1.85.0 检查通过 |

### 新增测试分布

新增 12 项测试，全部在 `ita-server` 的 `units` 模块内联：`capability.rs` 6 项（投影、就绪与
降级区分、失败计入降级、七 unit 最小拓扑、空记录得空能力集、输出按标签排序）、`watch.rs`
6 项（订阅先收完整能力集、降级后收带版本号的增量、版本号单调递增、能力集读 `units` 表、
两个订阅者都收到推送、通道承载通用能力变更消息）。全仓测试总数从 449 项增至 461 项。

## 08.1 Invocation 信封与版本协商

结论：信封与版本协商落地。`Invocation` 七个字段（`version`/`caller`/`session`/`cwd`/`argv`/
`stdin`/`lease_id`）里没有 workspace 字段，工作区由 server 从连接身份推导，不是请求参数；
`Handler` 的入参类型只有 `Invocation`，handler 拿不到 workspace，这条由编译器保证。
编解码是长度前缀文本：每个字段先写大端 4 字节长度前缀再写内容，与帧正文的长度前缀文本
同型，不引入 serde。版本协商是纯函数，请求版本落在闭区间 `min..=max` 内返回 `Accepted`，
否则
返回 `Rejected { supported }`，回显支持范围。计划第四节的十条验收全部写成测试并通过，
五处裁决逐条守住：`argv` 是内核的 `Command` AST 不是裸命令串；`stdin` 是 heredoc 正文之外的
独立输入流；`lease_id` 是内核的 `LeaseId` 名义类型；版本是 `u32` 主版本号不是 semver 三元组；
支持范围是 `SupportedRange` 类型与默认值常量，读取走配置结构。本批不建 socket、不引入
tokio、不碰网络，零新增外部依赖。

### 提交

| 提交 | 主题 | 改动 |
|---|---|---|
| `c850ccf` | 内核新增租约标识符名义类型，嵌套调用继承外层租约有类型可传 | `ids.rs` 用既有 `id_type!` 宏加 `LeaseId`（与 `NodeId`/`SessionId` 同型），`context/mod.rs` 重新导出，内联测试 |
| `bcb2176` | 协议版本协商纯函数，区间外拒绝并回显支持范围 | `version.rs` 新建 `ProtocolVersion`、`SupportedRange`（构造期校验 `min <= max`）、`negotiate`、`Accepted`、`Rejected` 与默认支持范围常量（含 6 项内联测试）；`lib.rs` 声明模块并再导出 |
| `5943ecb` | Invocation 信封与长度前缀编解码，handler 入参无 workspace 字段 | `envelope.rs` 新建 `Invocation`、`Caller`、`encode`/`decode`、`Handler` trait、`Response` 与 `HandlerError`（含 12 项内联测试）；`lib.rs` 声明模块并再导出；`Cargo.toml` 加 `ita-kernel` 一条依赖 |

### 验收覆盖

| 验收 | 用例 |
|---|---|
| 同版本往返一致 | `round_trip_is_identical` |
| 不兼容版本明确报错，回显支持范围 | `incompatible_version_is_rejected_with_supported_range` |
| 兼容版本被接受 | `compatible_version_is_accepted` |
| 支持范围边界值被接受，区间外被拒 | `boundary_versions_min_and_max_are_accepted`、`versions_outside_bounds_are_rejected` |
| handler 无法表达 workspace | `handler_cannot_express_workspace` |
| 信封的 argv 是内核的 Command AST | `argv_round_trips_command_ast` |
| stdin 是 heredoc 正文之外的独立输入流 | `stdin_round_trips_independent_input_stream` |
| lease_id 是嵌套调用继承外层租约的标识 | `lease_id_round_trips_outer_lease_marker` |
| 支持范围是可调参数，构造期校验 | `inverted_range_is_rejected_at_construction`、`single_version_range_is_legal_and_default_is_one` |
| 不建 socket、不引入 tokio、不碰网络 | `Cargo.toml` 只加 `ita-kernel` 一条依赖，零新增外部依赖（清单级核对） |

### 验证

| 命令 | 退出码 | 结果 |
|---|---|---|
| `cargo build --workspace --locked` | 0 | 全工作区构建通过 |
| `cargo nextest run --workspace --locked` | 0 | 480 项通过，0 项跳过（新增 19 项） |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 零告警 |
| `cargo fmt --all --check` | 0 | 无需格式化 |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked` | 0 | 文档构建通过 |
| `cargo test --doc --workspace --locked` | 0 | doctest 通过 |
| `bash .w1mer/scripts/check_layers.sh` | 0 | 分层门通过 |
| `bash .w1mer/scripts/check_kernel_purity.sh` | 0 | 内核纯净门通过 |
| `shellcheck .w1mer/scripts/*.sh` | 0 | shell 脚本静态检查通过 |

### 新增测试分布

新增 19 项测试：`ita-kernel` 的 `ids.rs` 1 项（`LeaseId` 接受规范 26 字符 Crockford base32
串、文本形式往返不丢信息、类型名用于诊断）；`ita-proto` 的 `version.rs` 6 项（区间外拒绝并
回显支持范围、区间内接受、边界值接受、区间外拒绝、倒置范围构造期被拒、单版本范围合法且
默认范围是它）与 `envelope.rs` 12 项（往返一致、handler 入参无 workspace、argv 是 Command
AST、stdin 独立输入流、lease_id 继承外层租约、截断被拒、多余字节被拒、未知方向字节被拒、
未知存在标志被拒、非法会话 id 被拒、非法调用者标签被拒、调用者标签校验规则）。全仓测试
总数从 461 项增至 480 项。
