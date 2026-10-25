# ita-tools — 工具清单、模板 ABI、注册表与安装编排

> 当前状态：空壳。`crates/ita-tools/src/lib.rs` 只有模块文档注释与 `#![deny(unsafe_code)]`、`#![deny(missing_docs)]`，没有实现代码。

## 预定职责

工具清单、模板 ABI、注册表与安装编排：解析清单、校验内容哈希与批准记录、生成注册表项、按步骤驱动安装。

## 依赖

`ita-kernel`、`ita-proto`、`ita-sandbox`（见 `STRUCTURE.md` 依赖边表）。

## 边界

- 只做声明与编排：不发起网络请求、不持有凭据，也不执行已安装的工具二进制。
- 运行期执行由执行器交给沙箱，工具二进制经 shim 回调执行器取得租约后再运行。
- 安装期的构建在沙箱内按清单声明的源进行。
- `tools` 数组会话开始即固定，只允许 append catalog 事件；单一 `{command}` schema。
