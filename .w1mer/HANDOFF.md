# 长程推进状态

> 这个文件是给"接手者"的入口：一个 compact 之后的会话、或者隔天回来的自己。它只记
> 当前进度、下一个批次怎么开、以及跨批次必须记住的判断。历史查 `.w1mer/`，不读正文。

## 当前位置

- 版本 `0.1.17`，最新提交在 `main` 且 CI 绿。
- 已完成史诗：01 决策固化、02 仓库骨架、03 命令文法、04 上下文模型。
- 下一步：史诗 05 语义压缩（`05.1` 起）。拆分依据见 `.w1mer/detail/04_plan-05.md`。

## 每个批次的标准动作

1. `w1mer batch-start <子任务>`，`w1mer role-join <子任务> implementer`。
2. 实现 + 三项验证：`cargo nextest run --workspace --locked`、
   `cargo clippy --workspace --all-targets --locked -- -D warnings`、
   `cargo fmt --all --check`，外加 `bash .w1mer/scripts/check_kernel_purity.sh`。
   每条都要看**退出码**，不要用管道过滤输出（曾经因此把 clippy 失败吞掉）。
3. `bash .w1mer/scripts/bump_version.sh batch`（一个批次一次，脚本会同步 Cargo.lock）。
4. 提交，`w1mer batch-end <子任务>`，`w1mer set task <子任务> --state done --effect "..."`。
5. 派 reviewer 子代理复审该批次提交；有问题当场修，修完把 review 置 `ok` 并追加处置记录。
6. 补齐本批缺失的角色（`role-join ... explorer`），否则下一批会被完整性门拦住。
7. 每 5 个批次推一次 `origin/main`，推送后确认 CI 绿。

## 跨批次必须记住的判断

- **内核纯净**：`ita-kernel` 零依赖（含 dev），禁止 IO/时钟/环境变量/线程。要读什么就
  作为参数传进来。这条由脚本与 clippy 双门强制，破门会直接失败。
- **不要用 `git add -A` 之外的捷径**：pre-commit 只格式化已暂存文件，未暂存文件的语法
  错误不会阻塞提交——但也不会被格式化。
- **文档与代码同批提交**：ADR 的"编辑性更正"可以直接改，决策变化必须新写一份并在状态段
  互相指明。
- **审查是有效的**：04 这一史诗里 reviewer 抓出了四个会导致返工的真缺陷（head 令牌没绑
  会话、固定层缺正文、字节数当预算用、帧重叠漏判）。不要跳过这一步。
- **写测试名时避免空格**：中文测试名里夹空格会变成非法标识符。

## 已知的、有意为之的取舍

- 命令文法版本一不支持管道、通配符、变量展开；heredoc 只允许一个且不与 `<` 同现。
- 未加引号的分隔符只在正文不含 `$`、反引号、反斜杠时接受（真实 shell 会展开）。
- 快照超过 1 MiB 的文件只记录元数据，回滚时只报告、不删除不覆盖。
- 压缩阈值 85%，预留必须显式配置，否则构造预算时直接失败。
