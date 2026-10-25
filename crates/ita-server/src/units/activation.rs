//! 启动门：必需 unit 全部就绪才返回就绪，非必需失败只标记降级。
//!
//! 门是纯函数，不改变状态，只判定。就绪不等于运行由 `Ready` 与 `Starting` 的区分承载：
//! 门只认 `Ready`，不认 `Starting`。一个必需 unit 进程活着（`Starting`）但探针未通过，
//! 门返回未就绪。非必需 unit 的 `Starting` 计入降级（还在启动中，功能未可用），不是未就绪。
//!
//! 门不写库、不转移状态。降级标记是门之后的独立步骤（server 拿到 `Ready` 结果后写库），
//! 与判定分离，让"判定"与"持久化"各归各位。

use super::UnitId;
use super::plan::StartupPlan;
use super::state::UnitState;
use super::supervisor::Supervisor;

/// 激活结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationResult {
    /// 激活成功：全部必需 unit 就绪，带就绪的必需 unit 与降级的非必需 unit 列表。
    Ready {
        /// 就绪的必需 unit 列表，按标签排序。
        ready: Vec<UnitId>,
        /// 降级的非必需 unit 列表，按标签排序。
        degraded: Vec<UnitId>,
    },
    /// 激活未就绪：有必需 unit 未就绪，带未就绪的必需 unit 列表。
    NotReady {
        /// 未就绪的必需 unit 列表，按标签排序。
        not_ready: Vec<UnitId>,
    },
}

/// 启动门：必需 unit 全部就绪才返回就绪，非必需失败只标记降级。
///
/// # 步骤
/// 1. 对每个必需 unit，状态是 `Ready` 才计入就绪列表，否则计入未就绪列表。
/// 2. 对每个非必需 unit，状态是 `Failed`/`NotStarted`/`Starting` 才计入降级列表，`Ready` 不计。
/// 3. 未就绪列表为空则返回 `Ready`（带降级列表），否则返回 `NotReady`（带未就绪列表）。
///
/// # 边界与前提
/// 门只认 `Ready`，不认 `Starting`。一个必需 unit 进程活着（`Starting`）但探针未通过，
/// 门返回未就绪。非必需 unit 的 `Starting` 计入降级，不是未就绪。
///
/// # 副作用
/// 无。门不改变状态，只判定。
pub fn activate(supervisor: &Supervisor, plan: &StartupPlan) -> ActivationResult {
    let mut ready = Vec::new();
    let mut not_ready = Vec::new();
    let mut degraded = Vec::new();

    for unit in &plan.required {
        match supervisor.state(unit) {
            Some(UnitState::Ready) => ready.push(unit.clone()),
            // `Starting`（探针未通过）与 `Failed`（不再重启）计入未就绪；
            // `NotStarted` 是被依赖阻塞，不是独立未就绪，不计入未就绪列表。
            Some(UnitState::Starting) | Some(UnitState::Failed) => not_ready.push(unit.clone()),
            _ => {}
        }
    }

    for unit in &plan.optional {
        match supervisor.state(unit) {
            Some(UnitState::Failed) | Some(UnitState::NotStarted) | Some(UnitState::Starting) => {
                degraded.push(unit.clone());
            }
            _ => {}
        }
    }

    if not_ready.is_empty() {
        ActivationResult::Ready { ready, degraded }
    } else {
        ActivationResult::NotReady { not_ready }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::super::probe::{ProbeResult, ReadinessProbe};
    use super::super::topology::Topology;

    fn id(label: &str) -> UnitId {
        UnitId::new(label).expect("测试标签应当合法")
    }

    /// 按脚本返回结果的假探针：每个 unit 的结果序列，用完保持最后一个。
    struct ScriptedProbe {
        scripts: RefCell<HashMap<UnitId, Vec<ProbeResult>>>,
        calls: RefCell<HashMap<UnitId, usize>>,
    }

    impl ScriptedProbe {
        fn new(scripts: HashMap<UnitId, Vec<ProbeResult>>) -> Self {
            Self { scripts: RefCell::new(scripts), calls: RefCell::new(HashMap::new()) }
        }
    }

    impl ReadinessProbe for ScriptedProbe {
        fn probe(&self, unit: &UnitId) -> ProbeResult {
            let call = {
                let mut calls = self.calls.borrow_mut();
                let call = calls.entry(unit.clone()).or_insert(0);
                *call += 1;
                *call
            };
            let scripts = self.scripts.borrow();
            let results = scripts.get(unit).expect("测试脚本必须覆盖该 unit");
            let index = call - 1;
            if index < results.len() {
                results[index]
            } else {
                *results.last().expect("脚本非空")
            }
        }
    }

    /// 必需未就绪时激活返回未就绪：store 保持 Starting，router 与 ipc 因依赖未就绪保持 NotStarted。
    #[test]
    fn required_not_ready_returns_not_ready() {
        let mut t = Topology::new();
        t.add_requires(id("router"), id("store"));
        t.add_requires(id("ipc"), id("router"));
        let mut scripts = HashMap::new();
        scripts.insert(id("store"), vec![ProbeResult::NotReady]);
        scripts.insert(id("router"), vec![ProbeResult::Ready]);
        scripts.insert(id("ipc"), vec![ProbeResult::Ready]);
        let mut s = Supervisor::new(t, Box::new(ScriptedProbe::new(scripts)));
        // store 探针 NotReady 保持 Starting，router 与 ipc 因依赖未就绪保持 NotStarted。
        s.step();
        let mut plan = StartupPlan::new();
        plan.add_required(id("store"));
        plan.add_required(id("router"));
        plan.add_required(id("ipc"));
        let result = activate(&s, &plan);
        assert_eq!(
            result,
            ActivationResult::NotReady { not_ready: vec![id("store")] },
            "store 未就绪应当返回 NotReady，router 与 ipc 是 NotStarted 不计入未就绪列表"
        );
    }

    /// 全部必需就绪时激活成功：就绪列表是全部必需 unit，降级列表为空。
    #[test]
    fn all_required_ready_returns_ready() {
        let mut t = Topology::new();
        t.add_requires(id("router"), id("store"));
        t.add_requires(id("ipc"), id("router"));
        let mut scripts = HashMap::new();
        scripts.insert(id("store"), vec![ProbeResult::Ready]);
        scripts.insert(id("router"), vec![ProbeResult::Ready]);
        scripts.insert(id("ipc"), vec![ProbeResult::Ready]);
        let mut s = Supervisor::new(t, Box::new(ScriptedProbe::new(scripts)));
        s.run_until_blocked();
        let mut plan = StartupPlan::new();
        plan.add_required(id("store"));
        plan.add_required(id("router"));
        plan.add_required(id("ipc"));
        let result = activate(&s, &plan);
        assert_eq!(
            result,
            ActivationResult::Ready {
                ready: vec![id("ipc"), id("router"), id("store")],
                degraded: vec![]
            },
            "全部必需就绪应当返回 Ready，就绪列表按标签排序"
        );
    }

    /// 非必需失败只标记降级，不阻塞激活：model 失败不阻塞 store 激活。
    #[test]
    fn optional_failed_only_degrades() {
        let mut t = Topology::new();
        t.add_requires(id("router"), id("store"));
        t.add_unit(id("model"));
        let mut scripts = HashMap::new();
        scripts.insert(id("store"), vec![ProbeResult::Ready]);
        scripts.insert(id("router"), vec![ProbeResult::Ready]);
        scripts.insert(id("model"), vec![ProbeResult::Failed]);
        let mut s = Supervisor::new(t, Box::new(ScriptedProbe::new(scripts)));
        s.run_until_blocked();
        let mut plan = StartupPlan::new();
        plan.add_required(id("store"));
        plan.add_required(id("router"));
        plan.add_optional(id("model"));
        let result = activate(&s, &plan);
        assert_eq!(
            result,
            ActivationResult::Ready {
                ready: vec![id("router"), id("store")],
                degraded: vec![id("model")]
            },
            "model 失败只标记降级，不阻塞激活"
        );
    }

    /// 非必需未就绪计入降级：telemetry 保持 Starting，计入降级不是未就绪。
    #[test]
    fn optional_not_ready_counts_as_degraded() {
        let mut t = Topology::new();
        t.add_unit(id("store"));
        t.add_unit(id("telemetry"));
        let mut scripts = HashMap::new();
        scripts.insert(id("store"), vec![ProbeResult::Ready]);
        scripts.insert(id("telemetry"), vec![ProbeResult::NotReady]);
        let mut s = Supervisor::new(t, Box::new(ScriptedProbe::new(scripts)));
        // store 探针 Ready 变 Ready，telemetry 探针 NotReady 保持 Starting。
        s.step();
        s.step();
        let mut plan = StartupPlan::new();
        plan.add_required(id("store"));
        plan.add_optional(id("telemetry"));
        let result = activate(&s, &plan);
        assert_eq!(
            result,
            ActivationResult::Ready { ready: vec![id("store")], degraded: vec![id("telemetry")] },
            "非必需 unit 的 Starting 计入降级，不是未就绪"
        );
    }

    /// 就绪不等于运行：store 保持 Starting（进程活着但未就绪），门返回未就绪。
    #[test]
    fn ready_is_not_running() {
        let mut t = Topology::new();
        t.add_unit(id("store"));
        let mut scripts = HashMap::new();
        scripts.insert(id("store"), vec![ProbeResult::NotReady]);
        let mut s = Supervisor::new(t, Box::new(ScriptedProbe::new(scripts)));
        // store 探针 NotReady 保持 Starting（进程活着但未就绪）。
        s.step();
        let mut plan = StartupPlan::new();
        plan.add_required(id("store"));
        let result = activate(&s, &plan);
        assert_eq!(
            result,
            ActivationResult::NotReady { not_ready: vec![id("store")] },
            "Starting 的必需 unit 计入未就绪，不是就绪"
        );
    }

    /// 七 unit 最小拓扑激活与降级：必需 unit 全部就绪，model 与 telemetry 失败都计入降级。
    ///
    /// telemetry 用 `Failed` 而非 `NotReady`：`NotReady` 保持 `Starting`，而 `step()` 按启动顺序
    /// 先碰到 `Starting` 的 telemetry 就重探针并返回，`tools` 与 `lease` 永远启动不了，
    /// `run_until_blocked` 也永不终止。`Failed` 是终态，不重探针，`tools` 与 `lease` 能推进到
    /// 就绪。非必需 unit 的 `Starting` 计入降级由 `optional_not_ready_counts_as_degraded` 单独覆盖。
    #[test]
    fn seven_unit_minimal_topology_activates_with_degradation() {
        let mut t = Topology::new();
        t.add_requires(id("router"), id("store"));
        t.add_requires(id("ipc"), id("router"));
        t.add_requires(id("lease"), id("tools"));
        t.add_unit(id("model"));
        t.add_unit(id("telemetry"));
        let mut scripts = HashMap::new();
        for unit in ["store", "router", "ipc", "lease", "tools"] {
            scripts.insert(id(unit), vec![ProbeResult::Ready]);
        }
        scripts.insert(id("model"), vec![ProbeResult::Failed]);
        scripts.insert(id("telemetry"), vec![ProbeResult::Failed]);
        let mut s = Supervisor::new(t, Box::new(ScriptedProbe::new(scripts)));
        s.run_until_blocked();
        let mut plan = StartupPlan::new();
        for unit in ["store", "router", "ipc", "lease", "tools"] {
            plan.add_required(id(unit));
        }
        plan.add_optional(id("model"));
        plan.add_optional(id("telemetry"));
        let result = activate(&s, &plan);
        assert_eq!(
            result,
            ActivationResult::Ready {
                ready: vec![id("ipc"), id("lease"), id("router"), id("store"), id("tools")],
                degraded: vec![id("model"), id("telemetry")]
            },
            "必需 unit 全部就绪，model 与 telemetry 计入降级"
        );
    }
}
