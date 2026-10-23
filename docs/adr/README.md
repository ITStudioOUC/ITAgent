# 架构决策记录

本目录按编号存放 ITAgent 的架构决策。每份记录只讲一件事：决定了什么、为什么这样决定、
否决了哪些替代方案。

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
| [0014](0014-executor-process-and-invocation-boundary.md) | 执行器是独立进程，一次调用有明确边界 | 已采纳 |
| [0015](0015-exclusive-resources-are-start-gates.md) | 独占资源是启动前的门，不是长租约 | 已采纳 |
| [0016](0016-policy-composition-and-refusal-classes.md) | 沙箱策略取交集，两类拒绝分开 | 已采纳 |
| [0017](0017-oversized-files-outside-snapshot-coverage.md) | 超阈值文件不在快照覆盖内 | 已采纳 |
| [0018](0018-kernel-interface-shape-and-tunables.md) | 内核接口形状与可调参数 | 已采纳 |

## 阅读顺序

新加入的开发者或 agent 建议按 0001 → 0002 → 0003 读，这三份决定了整个系统的骨架。
其余按需查阅。

0014 至 0018 是第一轮一致性审查（见 `.w1mer/review/R_01_1.md`）之后补的澄清与定型，
凡是与早期文档冲突之处，以编号更大的一份为准；早期文档的状态段记录了修订说明。

每份记录第五节的"被否决的替代方案"不是历史遗迹，其中列出的方案在某些条件下仍然成立，
文里写明了触发条件。

## 修订规则

- **决策变化必须新写一份**，编号递增，新旧两份在状态段互相指明。已采纳的正文不因决策
  变化而改写。
- **编辑性更正可以直接改正文**：事实错误、缺失的定义、交叉引用写错、术语首现未释义、
  措辞与另一份文档的硬纪律撞名。改完在状态段加一行修订说明。
- 判断标准：改动会不会让某个已经照着旧文实现的组件变得不兼容。会，就是决策变化；不会，
  就是编辑性更正。

## 与其他文档的分工

- 本目录记录**为什么**这样设计，以及当时的取舍。
- `.w1mer/ROADMAP.md` 记录**还要做什么**。
- `.w1mer/codebase/` 记录**现在是什么样**。
- 表结构、协议字段这类可执行细节属于实现规格，写在代码旁边或对应的规划文档里，
  不在本目录重复。
