//! unit 状态机：六个状态加一个纯转移函数。
//!
//! 状态机是纯逻辑，无 IO。"就绪不等于运行"由 `Starting`（进程活着但未就绪）与 `Ready`
//! （探针通过、功能可用）的区分承载。非法转移返回 `None` 而不是原地不动，失败 unit 的
//! 不可重启在状态机层面就有保障，不依赖监督者记得不重启。

/// unit 状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitState {
    /// 未启动。
    NotStarted,
    /// 启动中：进程活着但未就绪。
    Starting,
    /// 就绪：探针通过，功能可用。
    Ready,
    /// 失败：探针失败，不再重启。
    Failed,
    /// 停止中。
    Stopping,
    /// 已停止。
    Stopped,
}

/// 驱动状态转移的事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitEvent {
    /// 启动。
    Start,
    /// 探针判定就绪。
    ProbeReady,
    /// 探针判定失败。
    ProbeFailed,
    /// 探针判定未就绪。
    ProbeNotReady,
    /// 停止。
    Stop,
    /// 已停止。
    Stopped,
}

/// 纯转移函数。
///
/// 合法转移边是：未启动加 `Start` 到启动中；启动中加 `ProbeReady` 到就绪；启动中加
/// `ProbeFailed` 到失败；启动中加 `ProbeNotReady` 到启动中（原地重试）；就绪加 `Stop`
/// 到停止中；停止中加 `Stopped` 到已停止。其余组合都返回 `None`。
///
/// # 边界与前提
/// - 非法转移返回 `None`，不原地不动。
///
/// # 副作用
/// 无。
pub fn next(state: UnitState, event: UnitEvent) -> Option<UnitState> {
    match (state, event) {
        (UnitState::NotStarted, UnitEvent::Start) => Some(UnitState::Starting),
        (UnitState::Starting, UnitEvent::ProbeReady) => Some(UnitState::Ready),
        (UnitState::Starting, UnitEvent::ProbeFailed) => Some(UnitState::Failed),
        (UnitState::Starting, UnitEvent::ProbeNotReady) => Some(UnitState::Starting),
        (UnitState::Ready, UnitEvent::Stop) => Some(UnitState::Stopping),
        (UnitState::Stopping, UnitEvent::Stopped) => Some(UnitState::Stopped),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 未启动加启动到启动中。
    #[test]
    fn next_starts_from_not_started() {
        assert_eq!(next(UnitState::NotStarted, UnitEvent::Start), Some(UnitState::Starting));
    }

    /// 启动中加探针就绪到就绪。
    #[test]
    fn next_ready_from_probe_ready() {
        assert_eq!(next(UnitState::Starting, UnitEvent::ProbeReady), Some(UnitState::Ready));
    }

    /// 启动中加探针失败到失败。
    #[test]
    fn next_failed_from_probe_failed() {
        assert_eq!(next(UnitState::Starting, UnitEvent::ProbeFailed), Some(UnitState::Failed));
    }

    /// 启动中加探针未就绪原地重试。
    #[test]
    fn next_stays_starting_from_probe_not_ready() {
        assert_eq!(next(UnitState::Starting, UnitEvent::ProbeNotReady), Some(UnitState::Starting));
    }

    /// 就绪加停止到停止中。
    #[test]
    fn next_stopping_from_stop() {
        assert_eq!(next(UnitState::Ready, UnitEvent::Stop), Some(UnitState::Stopping));
    }

    /// 停止中加已停止到已停止。
    #[test]
    fn next_stopped_from_stopped() {
        assert_eq!(next(UnitState::Stopping, UnitEvent::Stopped), Some(UnitState::Stopped));
    }

    /// 非法转移被拒：就绪加启动、已停止加启动都返回 None。
    #[test]
    fn next_rejects_illegal_transition_from_ready() {
        assert_eq!(next(UnitState::Ready, UnitEvent::Start), None);
        assert_eq!(next(UnitState::Stopped, UnitEvent::Start), None);
    }

    /// 失败 unit 状态机层面不可重启。
    #[test]
    fn next_rejects_restart_of_failed_unit() {
        assert_eq!(next(UnitState::Failed, UnitEvent::Start), None);
    }
}
