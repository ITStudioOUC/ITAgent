//! 就绪探针：三值结果加可注入 trait。
//!
//! 探针是"功能可用"的判定，不是"进程活着"的判定。就绪的定义是数据库迁移完成、IPC 套接字
//! 在监听、路由表载入完毕，这些都是功能可用性检查。本模块定义探针抽象，真实的每 unit 探针
//! 实现（检查迁移、套接字、路由表）留到真实 unit 存在时再做，测试用假探针。

use super::UnitId;

/// 探针结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeResult {
    /// 就绪：功能可用。
    Ready,
    /// 未就绪：仍在启动。
    NotReady,
    /// 失败：功能不可用且不会恢复。
    Failed,
}

/// 就绪探针。
pub trait ReadinessProbe {
    /// 判定 unit 是否就绪。
    ///
    /// 探针是"功能可用"的判定，不是"进程活着"的判定。
    ///
    /// # 副作用
    /// 无。
    fn probe(&self, unit: &UnitId) -> ProbeResult;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 固定返回某个结果的假探针。
    struct FixedProbe {
        result: ProbeResult,
    }

    impl ReadinessProbe for FixedProbe {
        fn probe(&self, _unit: &UnitId) -> ProbeResult {
            self.result
        }
    }

    /// 假探针按三值返回。
    #[test]
    fn fixed_probe_returns_three_values() {
        let unit = UnitId::new("store").expect("合法标签");
        assert_eq!(FixedProbe { result: ProbeResult::Ready }.probe(&unit), ProbeResult::Ready);
        assert_eq!(
            FixedProbe { result: ProbeResult::NotReady }.probe(&unit),
            ProbeResult::NotReady
        );
        assert_eq!(FixedProbe { result: ProbeResult::Failed }.probe(&unit), ProbeResult::Failed);
    }
}
