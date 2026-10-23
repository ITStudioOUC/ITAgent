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
- 批次 02.3 新增 `.github/workflows/ci.yml`，按格式、clippy 加内核纯净门、构建加测试切成三个
  并行 job，clippy 与构建两步带 `--locked`，测试用 nextest 并暂时允许空测试集通过。契约层面 CI
  成为唯一强制 `-D warnings` 与锁文件校验的地方，本地的 `cargo clippy` 与 pre-commit 仍比 CI 松，
  这个差距只靠详情文档的风险表提醒，没有本地门承接。
- 批次 02.4 把工作区从单成员扩到五个成员，新增 ita-store、ita-proto、ita-sandbox、ita-exec
  四个零依赖空壳，职责按目标布局切分为存储、协议、沙箱与执行器，成员之间没有依赖边。契约层面
  新增一条待改约束：沙箱与执行器需要系统调用级实现，crate 级 unsafe 禁令应为 `deny` 而非
  `forbid`，否则 landlock 与 seccomp 无法就地实现；另外版本推进脚本必须同步全部成员的锁文件
  条目，工作区成员不再只有一个。审查时该批次尚无提交，上述内容以工作树为准。
- 批次 02.5 在工作树里把成员从五个扩到九个，新增 ita-tools、ita-model、ita-workflow、ita-server
  四个零依赖空壳，版本推出 0.1.5；本条记录时该批次尚无提交，以工作树为准。契约层面这次没有
  新增调用契约，只新增四条 crate 级约束：四者均 `deny(unsafe_code)` 并 `deny(missing_docs)`。
  依赖方向尚缺一条关键边：验收要求 server 依赖 store、proto、model、workflow，当前四者依赖表
  仍为空，目标布局里的上层到基础层的单向依赖还没建立。
- 批次 02.6 落地 `ita` 单二进制的角色分发与 ita-worker 空壳，成员增至十一个，版本推进到 0.1.6。
  `crates/ita-cli/src/role.rs` 新增 `Role` 枚举与 `parse`、`as_str`，角色只从参数解析、缺省为 master，
  不读环境变量；`crates/ita-cli/src/main.rs` 承担 `--version` 与分发，未知角色退出 2，三个已知角色
  打印"尚未实现"后退出 0。ita-worker 仅有 `pub fn run()` 占位，注释写明不得读环境变量或开数据库。
  契约层面新增两条尚未被门守住的约定：缺省角色是 master，未实现的角色退出 0；另外 CI 删除了
  `--no-tests=pass`，空测试集重新视为失败，`STACK.md` 的命令行同步去掉该参数。
- 审查 R_01_1 记录了三类未闭合点，尚未有对应模块文档可以挂靠，等执行器与存储模块建成后分别
  并入：执行器的进程归属在 0001 与 0013 之间不一致；工作流节点的独占资源与单次调用租约的生命
  周期不一致；沙箱策略的来源在工具清单与节点权限范围之间没有合成规则。超阈值文件在对齐算法里
  的删除与保留也有两份相反的表述。
