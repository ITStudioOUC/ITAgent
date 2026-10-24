# FEATURE — 功能实现的验证结果

> 每完成一个 ROADMAP 行追加一条记录。ROADMAP 行只留一行摘要，完整数据写在这里。
> 字段固定：任务号、提交与日期、验证命令与退出码、测试数量、结论。

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
