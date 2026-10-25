# Codebase — Module Registry

> One module doc per module, in this directory. Overview docs: `STACK.md`,
> `STRUCTURE.md`, `CONVENTIONS.md` (project-wide, cheap to refresh).
> Read order: `STACK → STRUCTURE → CONVENTIONS`, then the doc of the module
> you touch. `w1mer list` shows this table; `w1mer sync` resolves `[module]`
> delta tags against it.

| module | doc | last_sync | note |
|--------|-----|-----------|------|
| `ita-kernel` | `ita-kernel.md` | eed0132 | 纯逻辑内核：命令文法、上下文树、纯 `resolve()`、语义压缩计划、读集。零依赖 |
| `ita-store` | `ita-store.md` | eed0132 | SQLite 存储层：事件追加、帧提交、CAS、快照链、squash、回滚对齐、GC。仅依赖 ita-kernel |
| `ita-cli` | `ita-cli.md` | eed0132 | `ita` 二进制，角色分发（master\|worker\|tool-host），唯一组合根 |
| `ita-server` | `ita-server.md` | eed0132 | 空壳。唯一数据库写者：unit 监督、路由、事件日志、模型代理 |
| `ita-worker` | `ita-worker.md` | eed0132 | 空壳。无状态工作区 worker，会话循环与协议客户端 |
| `ita-proto` | `ita-proto.md` | eed0132 | 空壳。协议信封与本地帧编解码、版本协商 |
| `ita-model` | `ita-model.md` | eed0132 | 空壳。供应商报文归一化、流式聚合、用量记账 |
| `ita-sandbox` | `ita-sandbox.md` | eed0132 | 空壳。沙箱策略与平台实现（Landlock、Seccomp） |
| `ita-exec` | `ita-exec.md` | eed0132 | 空壳。工作区执行器：租约、权限预检、进程组回收 |
| `ita-tools` | `ita-tools.md` | eed0132 | 空壳。工具清单、模板 ABI、注册表、安装编排 |
| `ita-workflow` | `ita-workflow.md` | eed0132 | 空壳。工作流定义、静态检查、节点状态推导 |
