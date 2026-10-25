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
