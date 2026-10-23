---
id: CHANGES
title: Recent changes (architecture-impact deltas)
state: active
---

# Recent Changes

> Architecture-impact deltas recorded by reviewers during main batches.
> A periodic compact merges these into the stable `codebase/` docs and clears
> this log. One cache invalidation per compact — keep stable periods long.

## Deltas

> One bullet per delta, tagged with the target module doc:
> `- [gc] contract X moved`. Tags = module names from
> `codebase/INDEX.md`. Untagged → unassigned.
> `w1mer sync` shows a grouped preview; `w1mer sync --apply` writes + clears.

- 批次 01 落地了十三份架构决策记录与重写后的项目说明，系统骨架由此确定：内核纯逻辑与三进程隔离、
  单数据库单写者、纯函数解析与上下文树、语义压缩即折叠、两层回滚正交、内容寻址快照、受限命令
  文法、只用原生工具调用、单次调用租约、沙箱策略与保真度、工作流事件溯源、工具清单与模板、
  server 内 unit 监督。契约层面新增了 `resolve(head, policy)`、沙箱的 `probe` 与 `apply` 两个
  操作、工具模板的回调协议，以及快照与租约的元数据形状。
- 批次 02.1 建起 cargo 工作区：根清单新增 `[workspace.package]` 作为版本唯一来源，成员目前只有
  `ita-kernel`，其依赖表为空。契约层面新增两条 crate 级约束，内核禁止 unsafe，内核公开项必须有
  文档注释；版本口径定为 0.1.0，后续 crate 一律用 `.workspace = true` 继承，不再各写一份。
- 批次 02.2 落地内核纯净门：`crates/ita-kernel/clippy.toml` 逐条枚举内核禁止的 std 类型与
  方法（文件、网络、进程、线程、时钟、环境变量、标准输出），`lib.rs` 新增
  `deny(clippy::disallowed_methods)` 与 `deny(clippy::disallowed_types)` 使命中直接变成编译
  失败，新增 `.w1mer/scripts/check_kernel_purity.sh` 断言内核零普通/构建依赖且无 build.rs；
  版本推进到 0.1.1。契约层面新增两条可执行约束：内核不得出现在产物的依赖图里，内核源码不得
  引用被禁的 std 路径。
- 审查 R_01_1 记录了三类未闭合点，尚未有对应模块文档可以挂靠，等执行器与存储模块建成后分别
  并入：执行器的进程归属在 0001 与 0013 之间不一致；工作流节点的独占资源与单次调用租约的生命
  周期不一致；沙箱策略的来源在工具清单与节点权限范围之间没有合成规则。超阈值文件在对齐算法里
  的删除与保留也有两份相反的表述。
