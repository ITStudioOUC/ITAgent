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
