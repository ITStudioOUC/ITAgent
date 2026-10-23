# 架构决策记录

本目录按编号存放 ITAgent 的架构决策。每份记录只讲一件事：决定了什么、为什么这样
决定、否决了哪些替代方案。记录一旦采纳就不再修改；决策变化时新写一份，并在新旧两份
的状态里互相指明。

## 索引

| 编号 | 决策 | 状态 |
|---|---|---|
| [0001](0001-pure-kernel-and-process-isolation.md) | 内核纯逻辑与进程隔离 | 已采纳 |
| [0002](0002-single-database-and-single-writer-server.md) | 单数据库与 server 单写者 | 已采纳 |
| [0003](0003-context-tree-and-pure-resolve.md) | 上下文树与纯函数 resolve | 已采纳 |
| [0004](0004-semantic-compression-as-fold.md) | 语义压缩即折叠 | 已采纳 |
| [0005](0005-two-orthogonal-rollback-layers.md) | 两层回滚正交 | 已采纳 |
| [0006](0006-content-addressed-snapshots.md) | 快照用内容寻址存储 | 已采纳 |
| [0007](0007-restricted-bash-grammar.md) | 受限 bash 文法与 heredoc | 已采纳 |
| [0008](0008-native-tool-calling-only.md) | 只用原生工具调用 | 已采纳 |
| [0009](0009-single-call-leases.md) | 单次调用租约与冲突检测分层 | 已采纳 |
| [0010](0010-sandbox-policy-and-fidelity.md) | 沙箱按策略与保真度建模 | 已采纳 |
| [0011](0011-workflow-events-and-subagent-inheritance.md) | 工作流事件溯源与子 agent 双继承 | 已采纳 |
| [0012](0012-tool-packaging-and-trust.md) | 工具打包与信任链 | 已采纳 |
| [0013](0013-server-units.md) | server 内的 unit 监督 | 已采纳 |

## 阅读顺序

新加入的开发者或 agent 建议按 0001 → 0002 → 0003 读，这三份决定了整个系统的骨架。
其余按需查阅。每份记录第四节的"被否决的替代方案"不是历史遗迹，其中列出的方案在
某些条件下仍然成立，文里写明了触发条件。

## 与其他文档的分工

- 本目录记录**为什么**这样设计，以及当时的取舍。
- `.w1mer/ROADMAP.md` 记录**还要做什么**。
- `.w1mer/codebase/` 记录**现在是什么样**。
- 表结构、协议字段这类可执行细节属于实现规格，写在代码旁边或对应的规划文档里，
  不在本目录重复。
