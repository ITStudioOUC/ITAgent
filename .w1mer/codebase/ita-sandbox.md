# ita-sandbox — 沙箱策略与平台实现

> 当前状态：空壳。`crates/ita-sandbox/src/lib.rs` 只有模块文档注释与 `#![deny(unsafe_code)]`、`#![deny(missing_docs)]`，没有实现代码。

## 预定职责

沙箱策略与平台实现：`Sandbox` trait 与能力探针（`10.4`）、路径真实解析与穿越防护（`10.5`）、Linux 能力探针（`10.6`）、Landlock 文件规则（`10.7`）、Seccomp 过滤器（`10.8`）、端口级网络与域名意图（`10.9`）、环境清洗与资源上限（`10.10`）。

## 依赖

`ita-kernel`（见 `STRUCTURE.md` 依赖边表）。

## 能力归属

landlock/seccomp 只允许在 `ita-sandbox` 使用。`libc` 只允许在 `ita-sandbox` 与 `ita-exec` 使用。极少数位置可用 `unsafe`，一律逐处 `#[allow]` 并写 `# Safety` 说明成立条件。

## 边界

- 执行器的权限检查只是快速失败与好错误；安全边界在沙箱（Policy ∩ Fidelity），无降级模式。
- 生效策略 = 工具清单范围 ∩ 节点权限范围，节点只能收窄。
