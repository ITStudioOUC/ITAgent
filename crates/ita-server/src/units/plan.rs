//! 启动计划：声明哪些 unit 是必需、哪些是非必需。
//!
//! 计划是静态声明，在启动前建好，不动态建行。必需 unit 构成激活门——激活门等待它们全部
//! 就绪才返回就绪；非必需 unit 后台异步加载，失败只标记降级不阻塞激活。
//!
//! 计划与拓扑交叉校验：计划里的每个 unit 必须在拓扑里，必需集非空，必需集与非必需集不相交。
//! 校验失败返回显式错误，不静默继续。

use std::collections::BTreeSet;
use std::fmt;

use super::UnitId;
use super::topology::Topology;

/// 启动计划。
///
/// 两个集合：`required`（必需 unit，激活门等待它们全部就绪）与 `optional`（非必需 unit，
/// 后台异步加载，失败只标记降级）。计划是静态声明，在启动前建好，不动态建行。
#[derive(Debug, Clone, Default)]
pub struct StartupPlan {
    /// 必需 unit：激活门等待它们全部就绪。
    pub required: BTreeSet<UnitId>,
    /// 非必需 unit：后台异步加载，失败只标记降级。
    pub optional: BTreeSet<UnitId>,
}

/// 启动计划校验失败的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupPlanError {
    /// 计划里的 unit 不在拓扑里。
    UnknownUnit {
        /// 那个 unit。
        unit: UnitId,
    },
    /// 必需集为空：CLI 永远激活不了。
    EmptyRequired,
    /// 一个 unit 同时在必需集与非必需集。
    Overlapping {
        /// 那个 unit。
        unit: UnitId,
    },
}

impl fmt::Display for StartupPlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownUnit { unit } => {
                write!(f, "启动计划里的 unit `{unit}` 不在拓扑里")
            }
            Self::EmptyRequired => write!(f, "启动计划的必需集为空，CLI 永远激活不了"),
            Self::Overlapping { unit } => {
                write!(f, "unit `{unit}` 同时在必需集与非必需集")
            }
        }
    }
}

impl std::error::Error for StartupPlanError {}

impl StartupPlan {
    /// 构造一个空计划。
    pub fn new() -> Self {
        Self::default()
    }

    /// 把一个 unit 加入必需集。
    pub fn add_required(&mut self, unit: UnitId) {
        self.required.insert(unit);
    }

    /// 把一个 unit 加入非必需集。
    pub fn add_optional(&mut self, unit: UnitId) {
        self.optional.insert(unit);
    }

    /// 与拓扑交叉校验。
    ///
    /// # 步骤
    /// 1. 计划里的每个 unit（必需集与非必需集的并集）必须在拓扑里。
    /// 2. 必需集非空。
    /// 3. 必需集与非必需集不相交。
    ///
    /// # 边界与前提
    /// 任一条件不满足返回显式错误，不静默继续。计划含幽灵 unit 会在门里静默通过（拓扑里没有
    /// 这个 unit，门查不到它的状态），交叉校验让"计划里的 unit 必须在拓扑里"成为不变量。
    ///
    /// # 副作用
    /// 无。
    pub fn validate(&self, topology: &Topology) -> Result<(), StartupPlanError> {
        // 拓扑在启动时静态校验无环，startup_order 必成功。用它取拓扑里的全部 unit。
        let order = topology.startup_order().expect("拓扑构造时已校验无环");
        let units: BTreeSet<&UnitId> = order.iter().collect();
        // 计划里的每个 unit 必须在拓扑里。
        for unit in self.required.iter().chain(self.optional.iter()) {
            if !units.contains(unit) {
                return Err(StartupPlanError::UnknownUnit { unit: unit.clone() });
            }
        }
        // 必需集非空。
        if self.required.is_empty() {
            return Err(StartupPlanError::EmptyRequired);
        }
        // 必需集与非必需集不相交。
        for unit in self.required.iter() {
            if self.optional.contains(unit) {
                return Err(StartupPlanError::Overlapping { unit: unit.clone() });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(label: &str) -> UnitId {
        UnitId::new(label).expect("测试标签应当合法")
    }

    /// 合法计划通过校验：计划里的 unit 都在拓扑里，必需集非空，两集不相交。
    #[test]
    fn valid_plan_passes_validation() {
        let mut t = Topology::new();
        t.add_requires(id("router"), id("store"));
        t.add_unit(id("model"));
        let mut plan = StartupPlan::new();
        plan.add_required(id("store"));
        plan.add_required(id("router"));
        plan.add_optional(id("model"));
        plan.validate(&t).expect("应当通过校验");
    }

    /// 计划含拓扑里没有的 unit 被拒：交叉校验不静默继续。
    #[test]
    fn plan_with_ghost_unit_is_rejected() {
        let mut t = Topology::new();
        t.add_unit(id("store"));
        let mut plan = StartupPlan::new();
        plan.add_required(id("store"));
        plan.add_required(id("ghost"));
        let err = plan.validate(&t).expect_err("应当被拒");
        assert!(matches!(err, StartupPlanError::UnknownUnit { .. }));
    }

    /// 必需集为空被拒：CLI 永远激活不了。
    #[test]
    fn empty_required_set_is_rejected() {
        let mut t = Topology::new();
        t.add_unit(id("store"));
        let mut plan = StartupPlan::new();
        plan.add_optional(id("store"));
        let err = plan.validate(&t).expect_err("应当被拒");
        assert!(matches!(err, StartupPlanError::EmptyRequired));
    }

    /// 必需集与非必需集相交被拒：一个 unit 不能既是必需又是非必需。
    #[test]
    fn overlapping_sets_are_rejected() {
        let mut t = Topology::new();
        t.add_unit(id("store"));
        let mut plan = StartupPlan::new();
        plan.add_required(id("store"));
        plan.add_optional(id("store"));
        let err = plan.validate(&t).expect_err("应当被拒");
        assert!(matches!(err, StartupPlanError::Overlapping { .. }));
    }

    /// 空计划（必需集与非必需集都空）被拒：必需集为空。
    #[test]
    fn empty_plan_is_rejected() {
        let t = Topology::new();
        let plan = StartupPlan::new();
        let err = plan.validate(&t).expect_err("应当被拒");
        assert!(matches!(err, StartupPlanError::EmptyRequired));
    }

    /// 七 unit 最小拓扑的计划通过校验：必需集与非必需集不相交，全部在拓扑里。
    #[test]
    fn seven_unit_minimal_plan_passes_validation() {
        let mut t = Topology::new();
        t.add_requires(id("router"), id("store"));
        t.add_requires(id("ipc"), id("router"));
        t.add_requires(id("lease"), id("tools"));
        t.add_unit(id("model"));
        t.add_unit(id("telemetry"));
        let mut plan = StartupPlan::new();
        for unit in ["store", "router", "ipc", "lease", "tools"] {
            plan.add_required(id(unit));
        }
        for unit in ["model", "telemetry"] {
            plan.add_optional(id(unit));
        }
        plan.validate(&t).expect("应当通过校验");
    }
}
