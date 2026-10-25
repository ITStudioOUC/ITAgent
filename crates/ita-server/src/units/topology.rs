//! 拓扑结构与无环校验。
//!
//! 图是三种依赖边的并集，任何环都致命：`find_cycle` 用 DFS 三色标记找环并返回环路径
//! （闭合，首等于尾），`startup_order` 先查环，无环则做 Kahn 拓扑排序加最小堆，产出词典序
//! 最小的拓扑序。
//!
//! 边是三元组 `(target, dependent, kind)`，含义是 `dependent` 依赖 `target`，`target` 先启动。
//! 三种种类的边方向相同，种类只记录不消费。

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap};
use std::fmt;

use super::{DepKind, UnitId};

/// 拓扑成环的错误。
///
/// `cycle` 字段是环路径，闭合（首等于尾），按依赖方向表达：每个元素依赖下一个。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleError {
    /// 环路径，闭合（首等于尾），按依赖方向表达。
    pub cycle: Vec<UnitId>,
}

impl fmt::Display for CycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unit 依赖环：")?;
        for (i, unit) in self.cycle.iter().enumerate() {
            if i > 0 {
                write!(f, " → ")?;
            }
            write!(f, "{unit}")?;
        }
        Ok(())
    }
}

impl std::error::Error for CycleError {}

/// unit 依赖拓扑。
///
/// 一个 unit 集合加一个有向边集合。边是三元组 `(target, dependent, kind)`，含义是
/// `dependent` 依赖 `target`，`target` 先启动。三种种类的边方向相同，种类只记录。
#[derive(Debug, Clone, Default)]
pub struct Topology {
    /// unit 集合。
    units: BTreeSet<UnitId>,
    /// 边集合，按 `(target, dependent, kind)` 自然去重。
    edges: BTreeSet<(UnitId, UnitId, DepKind)>,
}

impl Topology {
    /// 构造一个空拓扑。
    pub fn new() -> Self {
        Self::default()
    }

    /// 加入一个 unit。已存在则无操作。
    pub fn add_unit(&mut self, unit: UnitId) {
        self.units.insert(unit);
    }

    /// `dependent` 硬依赖 `target`：`target` 失败则 `dependent` 连带失败。
    pub fn add_requires(&mut self, dependent: UnitId, target: UnitId) {
        self.add_edge(dependent, target, DepKind::Requires);
    }

    /// `dependent` 软依赖 `target`：`target` 失败则 `dependent` 只是降级。
    pub fn add_wants(&mut self, dependent: UnitId, target: UnitId) {
        self.add_edge(dependent, target, DepKind::Wants);
    }

    /// `dependent` 排在 `target` 之后：`target` 先启动，不隐含依赖。
    pub fn add_after(&mut self, dependent: UnitId, target: UnitId) {
        self.add_edge(dependent, target, DepKind::After);
    }

    /// 在三种边的并集上找环。
    ///
    /// # 步骤
    /// 1. 建并集邻接表。
    /// 2. DFS 三色标记，从最小标签起按标签序推进。
    /// 3. 撞灰节点即从栈顶回溯到灰节点，再补回起点，形成闭合环。
    ///
    /// # 边界与前提
    /// - 空拓扑或无边拓扑返回 `None`。
    /// - 自依赖是环，环路径为 `[A, A]`。
    ///
    /// # 副作用
    /// 无。
    ///
    /// # 注意事项
    /// 环路径按依赖方向表达：每个元素依赖下一个。输出归一为从环中最小标签起，使同一个环
    /// 无论由哪个灰节点触发检测都给出同样的表示。
    pub fn find_cycle(&self) -> Option<Vec<UnitId>> {
        let adj = self.union_adjacency();
        // 三色标记：全部节点初始为白，DFS 推进时转灰，出栈转黑。
        let mut color: HashMap<UnitId, Color> =
            self.units.iter().map(|u| (u.clone(), Color::White)).collect();
        let mut path: Vec<UnitId> = Vec::new();
        for start in self.units.iter() {
            if color.get(start).copied() == Some(Color::White) {
                if let Some(cycle) = dfs(start, &adj, &mut color, &mut path) {
                    return Some(cycle);
                }
            }
        }
        None
    }

    /// 稳定启动顺序。
    ///
    /// # 步骤
    /// 1. 先查环，有环返回带环路径的 `CycleError`。
    /// 2. 否则做 Kahn 拓扑排序加最小堆：入度为零的 unit 全进堆，每次取最小标签出堆、追加到
    ///    顺序、松弛其后继。
    ///
    /// # 边界与前提
    /// - 空拓扑返回空顺序。
    /// - 孤立 unit（无边）合法。
    ///
    /// # 副作用
    /// 无。
    ///
    /// # 注意事项
    /// 输出是词典序最小的拓扑序，同输入必同输出，状态机与启动门按这个顺序推进不必再排序。
    pub fn startup_order(&self) -> Result<Vec<UnitId>, CycleError> {
        if let Some(cycle) = self.find_cycle() {
            return Err(CycleError { cycle });
        }
        let adj = self.union_adjacency();
        // 入度按并集邻接表数：同一对 unit 的多种类边只算一次，与松弛次数一致。
        let mut in_degree: HashMap<UnitId, usize> =
            self.units.iter().map(|u| (u.clone(), 0)).collect();
        for dependents in adj.values() {
            for dependent in dependents {
                *in_degree.get_mut(dependent).expect("边的端点必在 unit 集合里") += 1;
            }
        }
        let mut heap: BinaryHeap<Reverse<UnitId>> = in_degree
            .iter()
            .filter(|(_, deg)| **deg == 0)
            .map(|(unit, _)| Reverse(unit.clone()))
            .collect();
        let mut order: Vec<UnitId> = Vec::new();
        while let Some(Reverse(unit)) = heap.pop() {
            order.push(unit.clone());
            if let Some(dependents) = adj.get(&unit) {
                for dependent in dependents {
                    let deg = in_degree.get_mut(dependent).expect("边的端点必在 unit 集合里");
                    *deg -= 1;
                    if *deg == 0 {
                        heap.push(Reverse(dependent.clone()));
                    }
                }
            }
        }
        Ok(order)
    }

    fn add_edge(&mut self, dependent: UnitId, target: UnitId, kind: DepKind) {
        self.units.insert(dependent.clone());
        self.units.insert(target.clone());
        self.edges.insert((target, dependent, kind));
    }

    /// 建并集邻接表：`target -> dependents`。
    fn union_adjacency(&self) -> BTreeMap<UnitId, BTreeSet<UnitId>> {
        let mut adj: BTreeMap<UnitId, BTreeSet<UnitId>> = BTreeMap::new();
        for unit in &self.units {
            adj.entry(unit.clone()).or_default();
        }
        for (target, dependent, _) in &self.edges {
            adj.entry(target.clone()).or_default().insert(dependent.clone());
        }
        adj
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Color {
    White,
    Gray,
    Black,
}

fn dfs(
    node: &UnitId,
    adj: &BTreeMap<UnitId, BTreeSet<UnitId>>,
    color: &mut HashMap<UnitId, Color>,
    path: &mut Vec<UnitId>,
) -> Option<Vec<UnitId>> {
    color.insert(node.clone(), Color::Gray);
    path.push(node.clone());

    if let Some(dependents) = adj.get(node) {
        for dependent in dependents {
            match color.get(dependent).copied() {
                Some(Color::Gray) => {
                    // 撞灰节点：环是栈上从灰节点到当前节点，再回到灰节点。
                    let start = path.iter().position(|u| u == dependent).expect("灰节点必在栈上");
                    let mut cycle = path[start..].to_vec();
                    cycle.push(dependent.clone());
                    // DFS 沿边方向走，反转后是依赖方向。
                    cycle.reverse();
                    // 归一为从最小标签起，使同一个环总有同样的表示。
                    let smallest = cycle.iter().min().expect("环非空");
                    let turn = cycle.iter().position(|u| u == smallest).expect("最小标签必在环里");
                    cycle.rotate_left(turn);
                    return Some(cycle);
                }
                Some(Color::Black) => {}
                _ => {
                    if let Some(cycle) = dfs(dependent, adj, color, path) {
                        return Some(cycle);
                    }
                }
            }
        }
    }

    path.pop();
    color.insert(node.clone(), Color::Black);
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(label: &str) -> UnitId {
        UnitId::new(label).expect("测试标签应当合法")
    }

    /// 有环声明返回环路径：三节点环被检出，路径闭合（首等于尾）。
    #[test]
    fn cycle_returns_closed_path() {
        let mut t = Topology::new();
        t.add_requires(id("a"), id("b"));
        t.add_requires(id("b"), id("c"));
        t.add_requires(id("c"), id("a"));
        let err = t.startup_order().expect_err("应当有环");
        assert_eq!(err.cycle, vec![id("a"), id("b"), id("c"), id("a")]);
        assert_eq!(
            err.cycle.first().expect("环非空"),
            err.cycle.last().expect("环非空"),
            "环路径必须闭合"
        );
    }

    /// 合法拓扑得到稳定启动顺序：同输入重复十次输出相同。
    #[test]
    fn legal_topology_gives_stable_order() {
        let mut t = Topology::new();
        t.add_requires(id("router"), id("store"));
        t.add_requires(id("ipc"), id("router"));
        let expected = vec![id("store"), id("router"), id("ipc")];
        for _ in 0..10 {
            assert_eq!(t.startup_order().expect("应当无环"), expected, "重复调用输出必须相同");
        }
    }

    /// 自依赖是环：环路径是 `[A, A]`。
    #[test]
    fn self_dependency_is_a_cycle() {
        let mut t = Topology::new();
        t.add_requires(id("a"), id("a"));
        let err = t.startup_order().expect_err("应当有环");
        assert_eq!(err.cycle, vec![id("a"), id("a")]);
    }

    /// 孤立 unit 合法：单个 unit 无边，返回自身顺序。
    #[test]
    fn isolated_unit_is_legal() {
        let mut t = Topology::new();
        t.add_unit(id("store"));
        assert_eq!(t.startup_order().expect("应当合法"), vec![id("store")]);
    }

    /// 空拓扑合法：空顺序。
    #[test]
    fn empty_topology_is_legal() {
        let t = Topology::new();
        assert_eq!(t.startup_order().expect("应当合法"), Vec::<UnitId>::new());
    }

    /// 三种边都参与无环判定：非 Requires 环也被检出。
    #[test]
    fn all_edge_kinds_participate_in_cycle_check() {
        let mut t = Topology::new();
        t.add_wants(id("a"), id("b"));
        t.add_after(id("b"), id("a"));
        let err = t.startup_order().expect_err("应当有环");
        assert_eq!(err.cycle, vec![id("a"), id("b"), id("a")]);
    }

    /// 稳定顺序是词典序最小的拓扑序：菱形里 B 在 C 前。
    #[test]
    fn stable_order_is_lexicographically_smallest() {
        let mut t = Topology::new();
        t.add_requires(id("b"), id("a"));
        t.add_requires(id("c"), id("a"));
        t.add_requires(id("d"), id("b"));
        t.add_requires(id("d"), id("c"));
        assert_eq!(t.startup_order().expect("应当无环"), vec![id("a"), id("b"), id("c"), id("d")]);
    }

    /// 重复边幂等：加两次同边不改顺序。
    #[test]
    fn duplicate_edge_is_idempotent() {
        let mut t = Topology::new();
        t.add_requires(id("a"), id("b"));
        t.add_requires(id("a"), id("b"));
        assert_eq!(t.startup_order().expect("应当无环"), vec![id("b"), id("a")]);
    }

    /// 同一对 unit 多种类边合法：不冲突。
    #[test]
    fn multiple_kinds_between_same_pair_are_legal() {
        let mut t = Topology::new();
        t.add_requires(id("a"), id("b"));
        t.add_after(id("a"), id("b"));
        assert_eq!(t.startup_order().expect("应当无环"), vec![id("b"), id("a")]);
    }

    /// 七 unit 最小拓扑无环：双向记号落单方向边，顺序合法。
    #[test]
    fn seven_unit_minimal_topology_is_acyclic() {
        let mut t = Topology::new();
        t.add_requires(id("router"), id("store"));
        t.add_requires(id("ipc"), id("router"));
        // 双向记号落单方向边：lease 依赖 tools，tools 先启动。
        t.add_requires(id("lease"), id("tools"));
        t.add_unit(id("model"));
        t.add_unit(id("telemetry"));
        let order = t.startup_order().expect("应当无环");
        // store 在 router 前，router 在 ipc 前，tools 在 lease 前。
        let pos = |u: &str| order.iter().position(|x| x.as_str() == u).expect("unit 必在顺序里");
        assert!(pos("store") < pos("router"));
        assert!(pos("router") < pos("ipc"));
        assert!(pos("tools") < pos("lease"));
        // 完整顺序是词典序最小的拓扑序。
        assert_eq!(
            order,
            vec![
                id("model"),
                id("store"),
                id("router"),
                id("ipc"),
                id("telemetry"),
                id("tools"),
                id("lease")
            ]
        );
    }
}
