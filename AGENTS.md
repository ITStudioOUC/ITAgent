# ITAgent — Agent Constraints

本文件是开发约束的唯一入口。所有 Agent（主 Agent 与 SubAgent）启动后先读本文件，
再读 `.w1mer/codebase/` 的稳定层（`STACK → STRUCTURE → CONVENTIONS`），然后按角色
进入 `.w1mer/ROADMAP.md` 的当前任务。若根目录存在 `.w1mer/HANDOFF.md`，先按它续跑。

## 0. 项目定位

ITAgent 是一个长程自主开发 Agent。仓库产出的二进制叫 `ita`。设计原则是**克制的
内核与自由的用户态分离**：内核只做纯逻辑（上下文树、命令文法、租约、权限谓词），
一切 IO、进程、沙箱、模型调用都在内核之外。这个分离不是风格偏好，是架构约束，
违反即返工。

## 1. 开发框架

项目使用 W1MER skill 推进开发，规划目录 `.w1mer/` **随仓库提交**——它是跨批次的
事实来源，批次边界依赖 git 对象，不进版本控制会丢失协议。

```
w1mer init / new / set / list / defer / re-rank / build / sync
w1mer batch-start <task> / batch-end <task> / ensure / role-join / show / status
```

类型注册表见 `.w1mer/schema.yaml`，内容文件是真相，INDEX 是 `w1mer build` 的产物，
一律不手改 INDEX，不手改 ROADMAP 表格行（走 CLI）。

批次生命周期：主 Agent 放批前 `batch-start`，批次提交后 `batch-end`；SubAgent 启动
即 `ensure` + `role-join`（幂等）。Reviewer 与 Explorer 只读已提交代码，用
`w1mer show <task>` 取 git 端点内容，永不读实现者的工作树。

## 2. 验证纪律

日常验证三件套，缺一不算完成：

```
cargo nextest run -p <crate>     # 单测
cargo build --workspace          # 全量构建
cargo clippy --workspace --all-targets   # 零新增告警（与改动前对比）
```

用 nextest 而非 `cargo test`：每测试独立进程，能把"靠共享全局状态才通过"的测试
暴露出来。若某测试在 nextest 下失败而 `cargo test` 通过，是该修的隐藏状态依赖，
不是绕过 nextest。按名过滤用 `-E 'test~正则'`。

`ita-kernel` 有额外的一道门：**它不得依赖 `tokio`、`rusqlite`、`reqwest` 或任何
带 IO/时钟的 crate**。这条门由 CI 里的依赖检查执行（`cargo tree -p ita-kernel`
断言无 IO 依赖），因为它是本项目的核心主张，靠自觉守不住。

性能相关改动一律按第 10 节纪律，只做本地零成本测量，不跑付费评测。

## 3. 提交规范（硬约束）

```
<type>: <一句中文，≤60 字>
```

- `type ∈ fix|feat|perf|refactor|test|docs|chore|style|bench|build|ci`
- 禁止 scope 前缀（`kernel/lexer:` 这种）、禁止 `｜` 分隔、禁止内部编号（ROADMAP
  编号、ADR 编号）、禁止提交哈希、禁止测试计数
- 主题写"改了什么、为什么"，不写实现步骤；细节写进 `.w1mer/` 或 `docs/adr/`
- 不用 `revert` 以外的动词开头，`revert:` 后接被撤销主题

好例：

```
feat: heredoc 分隔符缺失时拒绝写入并回显候选分隔符
fix: 嵌套调用不再申请新租约，改为继承外层
docs: 补租约与写集的静态检查规则
```

坏例：

```
feat(kernel/lexer): 05.1 heredoc 状态机 + 失败矩阵 8 例 | 12 tests pass
```

由 `.git/hooks/commit-msg` 强制，脚本副本在 `.w1mer/tools/commit-msg`；新克隆执行
`.w1mer/scripts/install_hooks.sh` 安装（同时装 `pre-commit`）。钩子拒绝时按提示改，
不改钩子。

## 4. 版本管理（硬约束）

版本格式 `0.<N>.<P>`，记于根 `Cargo.toml` 的 `[workspace.package] version`。

- `N` 是**史诗计数器**：ROADMAP 的一个大任务（顶层行，下称史诗）推进一位。
- `P` 是**批次计数器**：每完成并在 `w1mer batch-end` 收尾一个批次（不论主批次还是子批次）
  推进一位；新史诗开始时 `N+1` 且 `P` 归零。
- 基线 `0.1.0` 锚在仓库骨架落成时（ROADMAP 02）。纯规划提交（01 与 01.1）计入基线，不单独推进。
- 推进动作由 Implementer / Fixer 在**`batch-end` 之前的最后一次提交**里执行：
  `.w1mer/scripts/bump_version.sh <batch|epic>`，脚本改 Cargo.toml 并校验格式。
  参数 `batch` 表示同史诗内推进一位，`epic` 表示新史诗（`N+1` 且 `P` 归零）。
- 一个批次只推进一次。批次内的多个提交共用同一版本号，格式化与规划类自动提交不推进。
- 版本号不是内部编号："禁内部编号"约束的是 ROADMAP/ADR 任务号，不约束版本号；
  commit 里出现版本号用阿拉伯数字，不写成汉字。

## 5. 注释规范

全部新增与修改的注释遵循 `.w1mer/COMMENT_GUIDELINES.md`，要点：写语义不写历史、
禁止在代码里引用 ROADMAP 编号、函数接口注释按"步骤 / 边界与前提 / 副作用 / 注意事项"
结构书写、实现内部注释分步骤写且步骤间空行分隔但不写序号。`unsafe` 块必须有
`# Safety` 小节说明成立条件。规范变更先改文件，再改代码。

## 6. 文档规范

`.w1mer/` 下全部文档与 `docs/adr/` 遵循 `.w1mer/DOC_GUIDELINES.md`：先结论后
数据、完整中文句、缩写首现释义、禁电报体与私有词汇。设计决策写 ADR
（`docs/adr/NNNN-*.md`），五段固定格式：**状态 / 决策 / 理由 / 后果（含代价）/
被否决的替代方案**。最后两段不可省：代价让后来人知道哪些问题是预期内的，
被否决清单让后续批次不再重开已经收敛的争论。已采纳的 ADR 不再修改正文，决策变化时
新写一份并互相指明。

## 7. 架构不变量（违反即返工）

| 不变量 | 含义 |
|---|---|
| 内核纯 | `ita-kernel` 无 IO、无 async、无时钟、无随机；`resolve()` 是纯函数 |
| 单写者 | 数据库只有 server 能打开；worker 无连接、无 DDL、无 `workspace` 参数 |
| 作用域隐式 | `workspace` 是连接/会话属性，绝不作请求参数出现 |
| 两类消息 | 权威写（事件、head、租约）ack 必须代表已持久化；遥测可丢；队列必须有界 |
| 事件即真相 | 事件表 append-only 且不可变；head/租约/catalog 可变；两类不共用缓存 |
| 迁移只加 | 只允许加表加列，不改不删；ID 用 ULID `VARCHAR(26)`，时间用 epoch 毫秒 |
| 大对象外置 | 文件内容与工具正文进 CAS，库里只存 `hash + 长度 + 预览` |
| 回滚即对齐 | 回滚 = 把 manifest 范围内有内容记录的文件与墓碑对齐到快照，范围外不碰且输出残留清单；超过大小阈值的文件只报告、不删除不覆盖 |
| 租约单次 | 一次工具调用一张租约；多路径排序后原子获取；嵌套调用继承外层；写入前校验 hash；独占资源只是启动前的门，不产生长租约 |
| 快照时机 | 只在**整次调用**的全部后代进程被 reap 之后打快照，绝不与仍在写文件的进程并发 |
| 工具静态 | `tools` 数组会话开始即固定，只允许 append catalog 事件；单一 `{command}` schema |
| 原生调用 | 只用 provider 原生 tool calling，不做正文标签或正文 JSON 解析兜底 |
| 文法受限 | argv + `\|` + 重定向 + heredoc `<<`；单命令；禁通配符；正文全量收完再交付 |
| 沙箱唯一 | 执行器的权限检查只是快速失败与好错误；安全边界在沙箱（Policy ∩ Fidelity）；无降级模式 |
| 策略取交集 | 生效策略 = 工具清单范围 ∩ 节点权限范围，节点只能收窄；策略拒绝与租约拒绝分开 |
| 无出网 | worker 与沙箱默认无网络；全部模型流量经 server 代理，凭据不进沙箱 |
| 声明可审 | 节点与工具的 write_set 是预测；实际写入取调用前后快照清单的差集，偏差落库可查 |
| 参数可调 | ADR 与文档里的阈值都是默认值；读取统一走配置结构，不得以常量散落在调用点 |

## 8. 长期自主开发模式

项目处于长程自主开发阶段：Agent 拥有 commit 权限，自主决定实现顺序与修复进度，
无需逐步等待用户确认。三并调度、批次生命周期、角色能力、中断恢复、分层 codebase
文档与 map-reduce 映射由 w1mer skill 的 `references/` 定义，SubAgent 启动时加载。

- **任务粒度**：ROADMAP 顶层行是史诗，派发前拆成子任务（一个验收行为、≤8 文件、
  ≤~400 行、单次可验证）。实现者一批只做一个子任务，超限即请求拆分，不硬做。
- **主 Agent 上下文**：只保留状态卡（当前/下一子任务、未决风险 ≤3 条），历史查
  `.w1mer/` 不读正文。
- **CI 推送节奏**：每 5 个批次推送一次 origin/main，`batch-end` 后递增
  `.w1mer/ci_push_state.json`，到 5 推送并清零。CI 不绿优先修，不继续推。
- **交接**：上下文将满时，把续跑状态与下一批完整启动指令写入
  `.w1mer/HANDOFF.md`；交接落地后删除该文件。
- **格式化前移**：`.git/hooks/pre-commit` 在提交前跑 `cargo fmt --all`，并把结果
  并入本次提交，不产生额外的 `style` 提交。因此提交前工作树里的 Rust 文件必须是
  已格式化的意图，未暂存文件若被格式化会被告知且不纳入本次提交。
- 因此不再需要「提交后确认 HEAD 是否前进」的心智负担：一个子任务一个提交，HEAD
  就是自己的提交。

## 9. 长时命令与心跳

跑预计超过十分钟的命令（全量基准、大构建、长回测）前先判断有无更短的替代路径；
能按名字过滤到秒级完成就不跑全量。

- **SubAgent**：长命令放后台，轮询用前台阻塞 `sleep`，会话存活到任务收口。子会话
  提前终止会把命令孤儿化。
- **主 Agent**：长命令与心跳都可放后台，期间继续推进其他工作。
- **心跳只针对耗时命令，不针对后台 SubAgent**：SubAgent 完成会自动通知，设心跳
  轮询是浪费。

节奏：长命令后台启动并把输出重定向到日志；同时起 `sleep 600` 心跳；唤醒后查进程
是否存活、日志尾部是否推进；仍在运行则重开一个心跳；连续多个周期零推进或宿主
压力超限时终止，并按第 11 节归档根因。

## 10. 成本纪律（硬约束）

**不跑付费评测**。任务成功率、压缩保真度这类主观指标不进自动化评测：真实会话本身
就是数据集，离线挖掘即可。

- 本地零成本测量：lexer 吞吐、`resolve()` 耗时、SQLite 单写吞吐、IPC 往返、
  快照耗时随工作区大小变化、冷启动到 ready 的分解、工具调用路径分解。
- 供应商回传的 usage（输入/输出/cache 读写 token）必须落事件表，这是免费的，
  是成本账本的唯一来源。
- 任何要花钱的批量调用（模型对比、大规模回测）必须先取得用户明确同意。
- 基准结果写 `.w1mer/performance/`，判据与命令写清，便于复测。

## 11. 错误排查流程

1. 先查 `.w1mer/bug_reason/INDEX.md`，确认该错误是否已有根因分析。
2. 有记录则按文档中的方向处理，不重复汇报。
3. 无记录则汇报用户，由其决定是否深入。
4. 新根因确认后 `w1mer new bug --title "..."` 建档并 `w1mer build`。

## 12. 沟通与语言

全程中文。自动启用 ponytail 模式：最简实现，不引入不必要的依赖与抽象。

- 对话简洁直接，但用完整句子，不写电报体。
- 所有落盘产出（ROADMAP 行、review/detail/perf/results/bug 文档、commit、注释）
  用完整中文句，遵守第 5、6 节。
- 落盘前自查：开头能否用一句人话概括？不能就重写。
