//! 监督者：按拓扑序启动、按逆拓扑序停止。
//!
//! 监督者是同步的，消费拓扑与启动顺序。一个 unit 只有在全部依赖都就绪后才启动。失败
//! 的单位永不重启，其依赖者永不启动（保持未启动）。停止按逆拓扑序执行，依赖者先于被
//! 依赖者停止。"未就绪时后续依赖不启动"由"只启动全部依赖都就绪的 unit"这条规则承载。

use std::collections::HashMap;

use super::UnitId;
use super::probe::{ProbeResult, ReadinessProbe};
use super::state::{UnitEvent, UnitState, next};
use super::topology::Topology;

/// 监督者。
pub struct Supervisor {
    /// 拓扑。
    topology: Topology,
    /// 状态表。
    states: HashMap<UnitId, UnitState>,
    /// 探针。
    probe: Box<dyn ReadinessProbe>,
}

/// 一步监督的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    /// 有进展：启动或重新探针了某个 unit。
    Progressed(UnitId),
    /// 无进展：没有可启动或可重新探针的 unit。
    Blocked,
}

/// 停止结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownResult {
    /// 被停止的 unit 列表。
    pub stopped: Vec<UnitId>,
}

impl Supervisor {
    /// 构造一个监督者。
    ///
    /// # 边界与前提
    /// - 拓扑必须无环，有环直接 panic（拓扑在启动时静态校验无环）。
    ///
    /// # 副作用
    /// 无。
    pub fn new(topology: Topology, probe: Box<dyn ReadinessProbe>) -> Self {
        let order = topology.startup_order().expect("拓扑构造时已校验无环");
        let states = order.iter().map(|u| (u.clone(), UnitState::NotStarted)).collect();
        Self { topology, states, probe }
    }

    /// 驱动一次监督者循环。
    ///
    /// # 步骤
    /// 1. 按启动顺序遍历。
    /// 2. 找到第一个未启动且全部依赖都就绪的 unit，启动它并探针。
    /// 3. 否则找到第一个启动中的 unit，重新探针。
    ///
    /// # 边界与前提
    /// - 无进展时返回 `Blocked`。
    ///
    /// # 副作用
    /// - 修改状态表。
    pub fn step(&mut self) -> StepOutcome {
        let order = self.topology.startup_order().expect("拓扑构造时已校验无环");
        for unit in &order {
            match self.state(unit) {
                Some(UnitState::NotStarted) if self.all_dependencies_ready(unit) => {
                    self.apply(unit, UnitEvent::Start);
                    self.probe_unit(unit);
                    return StepOutcome::Progressed(unit.clone());
                }
                Some(UnitState::Starting) => {
                    self.probe_unit(unit);
                    return StepOutcome::Progressed(unit.clone());
                }
                _ => {}
            }
        }
        StepOutcome::Blocked
    }

    /// 反复调 step 直到 Blocked。
    ///
    /// # 副作用
    /// - 修改状态表。
    pub fn run_until_blocked(&mut self) -> Vec<UnitId> {
        let mut started = Vec::new();
        while let StepOutcome::Progressed(unit) = self.step() {
            started.push(unit);
        }
        started
    }

    /// 按逆拓扑序停止。
    ///
    /// # 步骤
    /// 1. 按启动顺序的逆序遍历。
    /// 2. 对每个就绪的 unit 执行 就绪 → 停止中 → 已停止。
    ///
    /// # 边界与前提
    /// - 失败、未启动、已停止的 unit 跳过（无可停止）。
    ///
    /// # 副作用
    /// - 修改状态表。
    pub fn shutdown(&mut self) -> ShutdownResult {
        let order = self.topology.startup_order().expect("拓扑构造时已校验无环");
        let mut stopped = Vec::new();
        for unit in order.iter().rev() {
            if self.state(unit) == Some(UnitState::Ready) {
                self.apply(unit, UnitEvent::Stop);
                self.apply(unit, UnitEvent::Stopped);
                stopped.push(unit.clone());
            }
        }
        ShutdownResult { stopped }
    }

    /// 查询 unit 状态。
    pub fn state(&self, unit: &UnitId) -> Option<UnitState> {
        self.states.get(unit).copied()
    }

    /// 查询 unit 是否就绪。
    pub fn is_ready(&self, unit: &UnitId) -> bool {
        self.state(unit) == Some(UnitState::Ready)
    }

    /// 返回全部就绪的 unit，按标签排序。
    pub fn ready_units(&self) -> Vec<UnitId> {
        let mut units: Vec<UnitId> = self
            .states
            .iter()
            .filter(|(_, state)| **state == UnitState::Ready)
            .map(|(unit, _)| unit.clone())
            .collect();
        units.sort();
        units
    }

    fn apply(&mut self, unit: &UnitId, event: UnitEvent) {
        if let Some(new_state) = next(self.state(unit).expect("unit 必在状态表里"), event) {
            self.states.insert(unit.clone(), new_state);
        }
    }

    fn probe_unit(&mut self, unit: &UnitId) {
        let event = match self.probe.probe(unit) {
            ProbeResult::Ready => UnitEvent::ProbeReady,
            ProbeResult::NotReady => UnitEvent::ProbeNotReady,
            ProbeResult::Failed => UnitEvent::ProbeFailed,
        };
        self.apply(unit, event);
    }

    fn all_dependencies_ready(&self, unit: &UnitId) -> bool {
        self.topology
            .dependencies_of(unit)
            .iter()
            .all(|dep| self.state(dep) == Some(UnitState::Ready))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

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

    /// 依赖未就绪时后续依赖不启动。
    #[test]
    fn dependent_stays_not_started_while_dependency_starting() {
        let mut t = Topology::new();
        t.add_requires(id("router"), id("store"));
        t.add_requires(id("ipc"), id("router"));
        let mut scripts = HashMap::new();
        scripts.insert(id("store"), vec![ProbeResult::NotReady, ProbeResult::Ready]);
        scripts.insert(id("router"), vec![ProbeResult::Ready]);
        scripts.insert(id("ipc"), vec![ProbeResult::Ready]);
        let mut s = Supervisor::new(t, Box::new(ScriptedProbe::new(scripts)));
        // 第一步：store 启动，探针未就绪，store 保持启动中，router 不启动。
        assert_eq!(s.step(), StepOutcome::Progressed(id("store")));
        assert_eq!(s.state(&id("store")), Some(UnitState::Starting));
        assert_eq!(s.state(&id("router")), Some(UnitState::NotStarted));
        // 第二步：store 重新探针，就绪，router 启动。
        assert_eq!(s.step(), StepOutcome::Progressed(id("store")));
        assert_eq!(s.state(&id("store")), Some(UnitState::Ready));
        assert_eq!(s.step(), StepOutcome::Progressed(id("router")));
        assert_eq!(s.state(&id("router")), Some(UnitState::Ready));
        assert_eq!(s.step(), StepOutcome::Progressed(id("ipc")));
        assert_eq!(s.state(&id("ipc")), Some(UnitState::Ready));
    }

    /// 启动按拓扑序推进。
    #[test]
    fn startup_advances_in_topological_order() {
        let mut t = Topology::new();
        t.add_requires(id("router"), id("store"));
        t.add_requires(id("ipc"), id("router"));
        let mut scripts = HashMap::new();
        scripts.insert(id("store"), vec![ProbeResult::Ready]);
        scripts.insert(id("router"), vec![ProbeResult::Ready]);
        scripts.insert(id("ipc"), vec![ProbeResult::Ready]);
        let mut s = Supervisor::new(t, Box::new(ScriptedProbe::new(scripts)));
        let started = s.run_until_blocked();
        assert_eq!(started, vec![id("store"), id("router"), id("ipc")]);
        assert_eq!(s.state(&id("store")), Some(UnitState::Ready));
        assert_eq!(s.state(&id("router")), Some(UnitState::Ready));
        assert_eq!(s.state(&id("ipc")), Some(UnitState::Ready));
    }

    /// 停止按逆序执行。
    #[test]
    fn shutdown_runs_in_reverse_topological_order() {
        let mut t = Topology::new();
        t.add_requires(id("router"), id("store"));
        t.add_requires(id("ipc"), id("router"));
        let mut scripts = HashMap::new();
        scripts.insert(id("store"), vec![ProbeResult::Ready]);
        scripts.insert(id("router"), vec![ProbeResult::Ready]);
        scripts.insert(id("ipc"), vec![ProbeResult::Ready]);
        let mut s = Supervisor::new(t, Box::new(ScriptedProbe::new(scripts)));
        s.run_until_blocked();
        let result = s.shutdown();
        assert_eq!(result.stopped, vec![id("ipc"), id("router"), id("store")]);
        assert_eq!(s.state(&id("store")), Some(UnitState::Stopped));
        assert_eq!(s.state(&id("router")), Some(UnitState::Stopped));
        assert_eq!(s.state(&id("ipc")), Some(UnitState::Stopped));
    }

    /// 失败不级联重启。
    #[test]
    fn failed_unit_is_not_restarted() {
        let mut t = Topology::new();
        t.add_requires(id("router"), id("store"));
        let mut scripts = HashMap::new();
        scripts.insert(id("store"), vec![ProbeResult::Failed]);
        scripts.insert(id("router"), vec![ProbeResult::Ready]);
        let mut s = Supervisor::new(t, Box::new(ScriptedProbe::new(scripts)));
        s.run_until_blocked();
        assert_eq!(s.state(&id("store")), Some(UnitState::Failed));
        assert_eq!(s.state(&id("router")), Some(UnitState::NotStarted));
        // 再调一次 run_until_blocked，store 仍是 Failed（未重启）。
        s.run_until_blocked();
        assert_eq!(s.state(&id("store")), Some(UnitState::Failed));
    }

    /// 失败不阻塞独立 unit。
    #[test]
    fn failure_does_not_block_independent_unit() {
        let mut t = Topology::new();
        t.add_requires(id("router"), id("store"));
        t.add_unit(id("model"));
        let mut scripts = HashMap::new();
        scripts.insert(id("store"), vec![ProbeResult::Failed]);
        scripts.insert(id("router"), vec![ProbeResult::Ready]);
        scripts.insert(id("model"), vec![ProbeResult::Ready]);
        let mut s = Supervisor::new(t, Box::new(ScriptedProbe::new(scripts)));
        s.run_until_blocked();
        assert_eq!(s.state(&id("store")), Some(UnitState::Failed));
        assert_eq!(s.state(&id("router")), Some(UnitState::NotStarted));
        assert_eq!(s.state(&id("model")), Some(UnitState::Ready));
    }

    /// 探针区分就绪与运行。
    #[test]
    fn probe_not_ready_keeps_unit_starting() {
        let mut t = Topology::new();
        t.add_requires(id("router"), id("store"));
        let mut scripts = HashMap::new();
        scripts.insert(id("store"), vec![ProbeResult::NotReady]);
        scripts.insert(id("router"), vec![ProbeResult::Ready]);
        let mut s = Supervisor::new(t, Box::new(ScriptedProbe::new(scripts)));
        assert_eq!(s.step(), StepOutcome::Progressed(id("store")));
        assert_eq!(s.state(&id("store")), Some(UnitState::Starting));
        assert_eq!(s.state(&id("router")), Some(UnitState::NotStarted));
    }

    /// 孤立 unit 启动与停止。
    #[test]
    fn isolated_unit_starts_and_stops() {
        let mut t = Topology::new();
        t.add_unit(id("store"));
        let mut scripts = HashMap::new();
        scripts.insert(id("store"), vec![ProbeResult::Ready]);
        let mut s = Supervisor::new(t, Box::new(ScriptedProbe::new(scripts)));
        s.run_until_blocked();
        assert_eq!(s.state(&id("store")), Some(UnitState::Ready));
        let result = s.shutdown();
        assert_eq!(result.stopped, vec![id("store")]);
        assert_eq!(s.state(&id("store")), Some(UnitState::Stopped));
    }

    /// 七 unit 最小拓扑启动与停止。
    #[test]
    fn seven_unit_minimal_topology_starts_and_stops() {
        let mut t = Topology::new();
        t.add_requires(id("router"), id("store"));
        t.add_requires(id("ipc"), id("router"));
        // 双向记号落单方向边：lease 依赖 tools，tools 先启动。
        t.add_requires(id("lease"), id("tools"));
        t.add_unit(id("model"));
        t.add_unit(id("telemetry"));
        let mut scripts = HashMap::new();
        for unit in ["store", "router", "ipc", "lease", "tools", "model", "telemetry"] {
            scripts.insert(id(unit), vec![ProbeResult::Ready]);
        }
        let mut s = Supervisor::new(t, Box::new(ScriptedProbe::new(scripts)));
        s.run_until_blocked();
        for unit in ["store", "router", "ipc", "lease", "tools", "model", "telemetry"] {
            assert_eq!(s.state(&id(unit)), Some(UnitState::Ready), "{unit} 应当就绪");
        }
        let result = s.shutdown();
        assert_eq!(result.stopped.len(), 7);
        for unit in ["store", "router", "ipc", "lease", "tools", "model", "telemetry"] {
            assert_eq!(s.state(&id(unit)), Some(UnitState::Stopped), "{unit} 应当已停止");
        }
    }
}
