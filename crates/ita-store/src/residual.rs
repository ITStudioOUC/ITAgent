//! 残留清单类型与原因。
//!
//! 残留清单是回滚结果的必要组成部分，不是日志附加项：它至少包含路径、尺寸与原因，让用户
//! 能判断每个残留文件的分量。原因有四类：超阈值（内容未存，只报告）、范围外写入（路径不在
//! 声明写集内，不碰）、后台进程遗留（快照之后新建，不在范围内）、内容缺失（声明正文已存但
//! 内容寻址存储里没有该哈希的行）。
//!
//! 纯数据结构，无文件系统访问。

/// 残留原因。
///
/// 枚举让四种原因各有名字，避免字符串的"超阈值"与"范围外"混用歧义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResidualReason {
    /// 超阈值：内容未进内容存储，回滚只报告，从不删除、从不覆盖。
    Oversized,
    /// 范围外写入：路径不在工具清单声明的写集内，回滚不碰。
    OutOfRange,
    /// 后台进程遗留：快照之后新建，不在声明写集内。
    BackgroundLeftover,
    /// 内容缺失：条目声明正文已存，但内容寻址存储里没有该哈希的行。
    MissingContent,
}

/// 一条残留清单条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResidualEntry {
    /// 工作区内声明的路径。
    pub path: String,
    /// 文件尺寸（字节）。目录没有尺寸，为 0。
    pub size: u64,
    /// 残留原因。
    pub reason: ResidualReason,
}

impl ResidualReason {
    /// 原因的中文描述，用于组装残留清单的输出。
    ///
    /// # 副作用
    /// 无。纯计算。
    pub fn describe(&self) -> &'static str {
        match self {
            Self::Oversized => "超阈值",
            Self::OutOfRange => "范围外写入",
            Self::BackgroundLeftover => "后台进程遗留",
            Self::MissingContent => "内容缺失",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 原因有名字，描述不混用。
    #[test]
    fn reason_has_name() {
        assert_eq!(ResidualReason::Oversized.describe(), "超阈值");
        assert_eq!(ResidualReason::OutOfRange.describe(), "范围外写入");
        assert_eq!(ResidualReason::BackgroundLeftover.describe(), "后台进程遗留");
        assert_eq!(ResidualReason::MissingContent.describe(), "内容缺失");
    }

    /// 残留条目带路径、尺寸与原因，三者缺一不可。
    #[test]
    fn residual_entry_carries_path_size_reason() {
        let entry = ResidualEntry {
            path: "big.log".into(),
            size: 2_000_000,
            reason: ResidualReason::Oversized,
        };
        assert_eq!(entry.path, "big.log");
        assert_eq!(entry.size, 2_000_000);
        assert_eq!(entry.reason, ResidualReason::Oversized);
    }
}
