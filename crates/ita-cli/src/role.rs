//! 角色解析。
//!
//! 一个二进制承担多种角色，避免多份可执行文件带来的路径与版本不一致问题。角色只从
//! 参数解析，不读环境变量——环境变量在不同工作区之间共享，会让角色变得不可预测。

/// 进程角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// 全局唯一的 server：持有数据库与模型凭据。
    Master,
    /// 每工作区一个 worker。
    Worker,
    /// 工具宿主，用于安装期构建与受限试运行。
    ToolHost,
}

impl Role {
    /// 从参数解析角色，缺省为 master。
    ///
    /// # 边界与前提
    /// 未知角色返回 `None`，由调用方打印用法并退出。
    ///
    /// # 副作用
    /// 无。
    pub fn parse(arg: Option<&str>) -> Option<Self> {
        match arg {
            None | Some("master") => Some(Self::Master),
            Some("worker") => Some(Self::Worker),
            Some("tool-host") => Some(Self::ToolHost),
            Some(_) => None,
        }
    }

    /// 角色的命令名，用于错误信息。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Master => "master",
            Self::Worker => "worker",
            Self::ToolHost => "tool-host",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Role;

    #[test]
    fn 缺省角色是_master() {
        assert_eq!(Role::parse(None), Some(Role::Master));
    }

    #[test]
    fn 三个已知角色都能解析() {
        assert_eq!(Role::parse(Some("master")), Some(Role::Master));
        assert_eq!(Role::parse(Some("worker")), Some(Role::Worker));
        assert_eq!(Role::parse(Some("tool-host")), Some(Role::ToolHost));
    }

    #[test]
    fn 未知角色返回_none() {
        assert_eq!(Role::parse(Some("serve")), None);
    }
}
