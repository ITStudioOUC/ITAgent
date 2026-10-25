//! 能力集快照：unit 就绪状态的投影。
//!
//! 能力集是 server 级能力概念，由 unit 状态推导，不是 per-unit 状态，独立于 `ita-workflow`
//! 的节点类型。它是落库的 `units` 表三值域的投影：`ready` 状态的 unit 进就绪列表，
//! `degraded` 与 `failed` 状态的 unit 进降级列表。失败的 unit 计入降级而不是丢弃，
//! 客户端才能展示"模型不可用"而不是笼统错误。
//!
//! 能力集读 `units` 表建，不读内存状态：server 重启后能力集仍可从表恢复。版本号是
//! 单调递增计数器，首次推送是完整能力集（版本 0）。

use super::UnitId;
use ita_store::UnitState;

/// 能力集快照。
///
/// 两个列表不相交：一个 unit 要么在就绪列表，要么在降级列表，不同时在两者。两个列表都
/// 按标签排序，输出稳定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilitySet {
    /// 版本号：单调递增计数器，首次推送为版本 0。
    pub version: u64,
    /// 就绪 unit：`ready` 状态的 unit，按标签排序。
    pub ready: Vec<UnitId>,
    /// 降级 unit：`degraded` 或 `failed` 状态的 unit，按标签排序。
    pub degraded: Vec<UnitId>,
}

/// 由 unit 状态记录建能力集：把 unit 状态投影成能力集的纯函数。
///
/// # 步骤
/// 1. 逐条记录投影：状态是 `ready` 的 unit 进就绪列表，`degraded` 与 `failed` 的进降级列表。
/// 2. 两个列表按标签排序去重，保证输出稳定。
///
/// # 边界与前提
/// - 记录里的 unit 标识是 server 写入的校验过的标签，解析必然成功。
/// - `failed` 计入降级，不是丢弃。
/// - 版本号由调用方传入，本函数不管理版本号。
///
/// # 副作用
/// 无。
pub fn build_capability_set(records: &[(String, UnitState)], version: u64) -> CapabilitySet {
    let mut ready = Vec::new();
    let mut degraded = Vec::new();

    // 按状态投影：ready 进就绪列表，degraded 与 failed 进降级列表。
    for (id, state) in records {
        // units 表里的 unit 标识是 server 写入的校验过的标签，解析必然成功。
        let unit = UnitId::new(id.as_str()).expect("units 表里的 unit 标识必是校验过的标签");
        match state {
            UnitState::Ready => ready.push(unit),
            UnitState::Degraded | UnitState::Failed => degraded.push(unit),
        }
    }

    // 按标签排序去重：能力集的输出不随输入顺序变化。
    ready.sort();
    ready.dedup();
    degraded.sort();
    degraded.dedup();

    CapabilitySet { version, ready, degraded }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(label: &str) -> UnitId {
        UnitId::new(label).expect("测试标签应当合法")
    }

    /// 能力集是 unit 就绪状态的投影：ready 进就绪列表，degraded 与 failed 进降级列表。
    #[test]
    fn capability_set_projects_unit_states() {
        let records = vec![
            ("store".to_string(), UnitState::Ready),
            ("model".to_string(), UnitState::Degraded),
            ("telemetry".to_string(), UnitState::Failed),
        ];
        let set = build_capability_set(&records, 0);
        assert_eq!(set.ready, vec![id("store")], "ready unit 应当进就绪列表");
        assert_eq!(
            set.degraded,
            vec![id("model"), id("telemetry")],
            "degraded 与 failed unit 应当进降级列表"
        );
    }

    /// 就绪与降级区分：两个列表不相交，一个 unit 不同时在两者。
    #[test]
    fn ready_and_degraded_lists_are_disjoint() {
        let records = vec![
            ("store".to_string(), UnitState::Ready),
            ("model".to_string(), UnitState::Degraded),
        ];
        let set = build_capability_set(&records, 0);
        assert!(set.ready.contains(&id("store")), "store 应当在就绪列表");
        assert!(set.degraded.contains(&id("model")), "model 应当在降级列表");
        assert!(!set.degraded.contains(&id("store")), "store 不应当在降级列表");
        assert!(!set.ready.contains(&id("model")), "model 不应当在就绪列表");
    }

    /// 失败 unit 计入降级，不是丢弃。
    #[test]
    fn failed_unit_counts_as_degraded() {
        let records = vec![("model".to_string(), UnitState::Failed)];
        let set = build_capability_set(&records, 0);
        assert_eq!(set.degraded, vec![id("model")], "failed unit 应当计入降级");
        assert!(set.ready.is_empty(), "failed unit 不进就绪列表");
    }

    /// 七 unit 最小拓扑的能力集：必需 unit 全部就绪，model 与 telemetry 计入降级。
    #[test]
    fn seven_unit_minimal_topology_capability_set() {
        let records = vec![
            ("store".to_string(), UnitState::Ready),
            ("router".to_string(), UnitState::Ready),
            ("ipc".to_string(), UnitState::Ready),
            ("lease".to_string(), UnitState::Ready),
            ("tools".to_string(), UnitState::Ready),
            ("model".to_string(), UnitState::Degraded),
            ("telemetry".to_string(), UnitState::Failed),
        ];
        let set = build_capability_set(&records, 0);
        assert_eq!(
            set.ready,
            vec![id("ipc"), id("lease"), id("router"), id("store"), id("tools")],
            "必需 unit 全部在就绪列表，按标签排序"
        );
        assert_eq!(set.degraded, vec![id("model"), id("telemetry")], "model 与 telemetry 计入降级");
    }

    /// 空记录得空能力集：两个列表都空，版本号原样保留。
    #[test]
    fn empty_records_yield_empty_capability_set() {
        let set = build_capability_set(&[], 3);
        assert!(set.ready.is_empty(), "空记录应当得到空就绪列表");
        assert!(set.degraded.is_empty(), "空记录应当得到空降级列表");
        assert_eq!(set.version, 3, "版本号应当原样保留");
    }

    /// 输出按标签排序，不随输入顺序变化。
    #[test]
    fn capability_set_output_is_sorted_by_label() {
        let records = vec![
            ("store".to_string(), UnitState::Ready),
            ("ipc".to_string(), UnitState::Ready),
            ("lease".to_string(), UnitState::Degraded),
            ("model".to_string(), UnitState::Degraded),
        ];
        let set = build_capability_set(&records, 0);
        assert_eq!(set.ready, vec![id("ipc"), id("store")], "就绪列表按标签排序");
        assert_eq!(set.degraded, vec![id("lease"), id("model")], "降级列表按标签排序");
    }
}
