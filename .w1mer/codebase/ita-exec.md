# ita-exec — 工作区执行器

> 当前状态：空壳。`crates/ita-exec/src/lib.rs` 只有模块文档注释与 `#![deny(unsafe_code)]`、`#![deny(missing_docs)]`，没有实现代码。

## 预定职责

工作区执行器：租约、权限预检与进程组回收。属于内核之外的一层，做 IO、时钟或子进程，不追求可复现的纯函数语义，但必须把副作用限制在自己的边界内，不向上层泄漏。

## 依赖

`ita-kernel`、`ita-proto`、`ita-sandbox`、`ita-tools`（见 `STRUCTURE.md` 依赖边表）。

## 能力归属

`libc` 只允许在 `ita-sandbox` 与 `ita-exec` 使用（系统调用与平台 FFI）。极少数位置可用 `unsafe`，一律逐处 `#[allow]` 并写 `# Safety` 说明成立条件；其余位置由 `deny(unsafe_code)` 拦住。

## 边界

- 权限预检只是快速失败与好错误；安全边界在沙箱（Policy ∩ Fidelity），无降级模式。
- 进程组与 subreaper 回收范围、逐层回收与超时强杀、取消传播到整组，都由本 crate 承接。
- 回收后快照与实际写入差集（`09.9`）在本 crate 内完成。
