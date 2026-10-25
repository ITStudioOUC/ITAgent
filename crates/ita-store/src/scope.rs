//! 工作区作用域。
//!
//! 单库多工作区意味着每个查询都必须带工作区过滤。靠"记得写 `WHERE`"守不住，因此业务代码
//! 只能拿到 [`WorkspaceScope`]，而它内部持有工作区 id，构造业务查询时自动带上。
//!
//! 这条约束目前只做到一半：`WorkspaceScope` 的构造函数不是 `pub`，只有 [`Store`] 能创建
//! 它，且构造时校验工作区存在。但"每个业务查询都必须传作用域"要等事件与快照模块落地才
//! 成立——本批次还没有带作用域的操作方法。在那之前，这条注释描述的是目标而不是现状。
//!
//! [`Store`]: crate::Store

use ita_kernel::context::WorkspaceId;

/// 一个绑定了工作区的查询上下文。
///
/// 不能直接构造：由 [`Store::scope`](crate::Store::scope) 创建，确保它一定对应一个真实存在
/// 的工作区。
#[derive(Debug, Clone)]
pub struct WorkspaceScope {
    workspace: WorkspaceId,
}

impl WorkspaceScope {
    /// 由存储层内部创建。
    ///
    /// # 边界与前提
    /// 调用方必须已经确认该工作区存在。这个构造函数不对外，正是因为"确认存在"是存储层的
    /// 职责，不能让业务代码绕过。
    ///
    /// # 副作用
    /// 无。
    pub(crate) fn new(workspace: WorkspaceId) -> Self {
        Self { workspace }
    }

    /// 取工作区 id。
    ///
    /// # 副作用
    /// 无。
    pub fn workspace(&self) -> &WorkspaceId {
        &self.workspace
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 作用域携带工作区 id，业务查询永远绑定到单个工作区。
    #[test]
    fn scope_carries_workspace() {
        let id = WorkspaceId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAV").expect("应当合法");
        let scope = WorkspaceScope::new(id.clone());
        assert_eq!(scope.workspace(), &id);
    }
}
