# ita-server — 唯一数据库写者

> 当前状态：unit 模块已落地（`07.1` 至 `07.4`），`crates/ita-server/src/units/` 下有拓扑、状态机、探针、监督者、启动计划、启动门、能力集与 watch 通道八个模块。路由、事件日志、模型代理与本地 socket 尚未实现。

## 已落地职责（unit 模块）

unit 是固定命名的子系统（`store`、`router`、`ipc`、`lease`、`tools`、`model`、`telemetry`），拓扑在启动时静态声明，不动态建行。`UnitId` 是校验过的标签名义类型（小写字母数字加连字符、非空、首尾不连字符、不连续连字符），与 systemd unit 命名同型。

- `topology.rs`：`DepKind` 三变体（Requires/Wants/After）在图结构里方向相同，种类只记录不消费；`find_cycle` 用 DFS 三色标记在三种边的并集图上找环，撞灰节点提取闭合环路径；`startup_order` 先查环，无环则做 Kahn 加最小堆，产出词典序最小的拓扑序。`lease ↔ tools` 的双向记号落成单方向边（lease 依赖 tools，tools 先启动）。
- `state.rs`：`UnitState` 六变体（未启动/启动中/就绪/失败/停止中/已停止）与 `UnitEvent` 六变体，纯转移函数 `next(state, event) -> Option<UnitState>` 非法转移返回 `None`。失败 unit 的不可重启在状态机层面即有保障。
- `probe.rs`：`ProbeResult` 三值（就绪/未就绪/失败）加可注入 trait `ReadinessProbe`，探针是「功能可用」判定不是「进程活着」判定。
- `supervisor.rs`：`Supervisor` 同步，按拓扑序启动（一个 unit 全部依赖 `Ready` 才启动）、按逆拓扑序停止，失败不级联重启是双层保障。
- `plan.rs`：`StartupPlan` 是 `required`/`optional` 两个 `BTreeSet<UnitId>`，与拓扑交叉校验。
- `activation.rs`：`activate(supervisor, plan) -> ActivationResult` 是纯函数，必需 unit 全部 `Ready` 才就绪、非必需失败只标记降级不阻塞。
- `capability.rs`：`CapabilitySet` 是 unit 就绪状态的投影，`build_capability_set` 纯函数把 `read_all_unit_states` 读出的状态投影成能力集。
- `watch.rs`：`CapabilityWatcher` 持 `Store` 句柄，进程内 watch 通道向订阅者推带版本号的完整能力集。

## 预定职责（尚未实现）

唯一数据库写者：路由、事件日志与模型代理。本地 socket 与连接级工作区绑定（`08.2`）、权威与遥测分流（`08.3`）、group commit（`08.4`）、批量读（`08.5`）。

## 依赖

`ita-kernel`、`ita-store`、`ita-proto`、`ita-model`、`ita-workflow`、`ita-tools`（见 `STRUCTURE.md` 依赖边表）。

## 能力归属

HTTP/TLS 栈只允许在 `ita-server` 使用。`nix`/`rustix` 也在 `ita-server` 用于 unit 监督。SQLite 驱动只允许在 `ita-store`。

## 边界

- 位于内核之外，可以持有 IO、时钟或网络句柄；但这些副作用必须收在本层，向上只暴露普通数据与显式错误。
- 数据库只有 server 能打开；worker 无连接、无 DDL、无 `workspace` 参数。
- 全部模型流量经 server 代理，凭据不进沙箱。
