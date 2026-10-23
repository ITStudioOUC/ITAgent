//! 事实层：由机器填写的那一半摘要。
//!
//! 让模型总结自己，它会系统性地美化：把"试了 X 但报错 E"写成"探索了 X"。下一轮它就会
//! 再试一遍 X。事实层的存在就是为了抑制这一点——改动文件、失败的测试、报错摘要由机器填，
//! 模型没有机会改写。
//!
//! 事实层也是压缩唯一的"硬校验"入口：缺失败记录就不允许出帧。宁可这次压缩失败重试，也
//! 不要产出一个把失败抹掉的摘要。

use super::super::ids::CheckpointId;

/// 一条失败记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// 尝试了什么（命令或方案）。
    pub attempt: String,
    /// 结果如何（报错摘要、失败的测试名）。
    pub outcome: String,
}

/// 事实层。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// 被改动的文件，`(路径, 增行, 删行)`。
    pub changed_files: Vec<(String, u32, u32)>,
    /// 失败的尝试。
    pub failures: Vec<Failure>,
    /// 已否决的方案及理由。
    pub rejected: Vec<(String, String)>,
    /// 这段历史对应的检查点。
    pub checkpoint: Option<CheckpointId>,
    /// 这段历史跨过的轮数。
    pub turns: u32,
}

/// 事实层不完整。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactsError {
    /// 有改动但没有任何失败记录。
    ///
    /// 真实的开发过程几乎总有失败；一条都没有，通常意味着收集环节漏了，或者模型把失败
    /// 写成了"探索"。两种情况都该拒绝出帧。
    NoFailuresWithChanges {
        /// 改动的文件数。
        changed: usize,
    },
    /// 已否决的方案只写了名字没写理由。
    RejectedWithoutReason {
        /// 那条方案的名称。
        name: String,
    },
    /// 轮数为零：这段历史什么都没覆盖。
    EmptySpan,
}

impl std::fmt::Display for FactsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoFailuresWithChanges { changed } => write!(
                f,
                "改动了 {changed} 个文件却没有任何失败记录；失败记录不能为空，否则下一轮会重复已失败的方案"
            ),
            Self::RejectedWithoutReason { name } => {
                write!(f, "已否决的方案 `{name}` 没有写理由，无法判断是否该重新考虑")
            }
            Self::EmptySpan => write!(f, "这段历史没有覆盖任何轮次"),
        }
    }
}

impl std::error::Error for FactsError {}

impl Facts {
    /// 校验事实层完整性。
    ///
    /// # 步骤
    /// 1. 轮数不能为零。
    /// 2. 有文件改动时必须有失败记录。
    /// 3. 每条已否决方案都要有理由。
    ///
    /// # 边界与前提
    /// 只做结构校验，不判断内容质量。"失败记录写得是否准确"无法自动判定，只能靠"压缩后
    /// 是否重复已否决操作"这个离线指标来观察。
    ///
    /// # 副作用
    /// 无。
    pub fn validate(&self) -> Result<(), FactsError> {
        if self.turns == 0 {
            return Err(FactsError::EmptySpan);
        }
        if !self.changed_files.is_empty() && self.failures.is_empty() {
            return Err(FactsError::NoFailuresWithChanges { changed: self.changed_files.len() });
        }
        for (name, reason) in &self.rejected {
            if reason.trim().is_empty() {
                return Err(FactsError::RejectedWithoutReason { name: name.clone() });
            }
        }
        Ok(())
    }

    /// 渲染成给模型看的事实段。
    ///
    /// # 边界与前提
    /// 调用前必须已通过 [`validate`](Self::validate)。渲染不做二次校验，避免同一份规则
    /// 写两遍。
    ///
    /// # 副作用
    /// 无。
    pub fn render(&self) -> String {
        let mut out = String::from("事实（由系统填写，不可修改）：\n");

        if self.changed_files.is_empty() {
            out.push_str("- 未改动文件\n");
        } else {
            out.push_str("- 改动文件：\n");
            for (path, added, removed) in &self.changed_files {
                out.push_str(&format!("  - {path} (+{added}/-{removed})\n"));
            }
        }

        if !self.failures.is_empty() {
            out.push_str("- 失败的尝试（不要重复）：\n");
            for failure in &self.failures {
                out.push_str(&format!("  - {} → {}\n", failure.attempt, failure.outcome));
            }
        }

        if !self.rejected.is_empty() {
            out.push_str("- 已否决的方案：\n");
            for (name, reason) in &self.rejected {
                out.push_str(&format!("  - {name}：{reason}\n"));
            }
        }

        if let Some(checkpoint) = &self.checkpoint {
            out.push_str(&format!("- 检查点：{checkpoint}\n"));
        }
        out.push_str(&format!("- 覆盖轮数：{}\n", self.turns));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    fn checkpoint() -> CheckpointId {
        CheckpointId::parse(BASE).expect("应当合法")
    }

    fn minimal() -> Facts {
        Facts {
            changed_files: vec![],
            failures: vec![],
            rejected: vec![],
            checkpoint: None,
            turns: 1,
        }
    }

    #[test]
    fn 无改动的空事实通过校验() {
        assert!(minimal().validate().is_ok());
    }

    #[test]
    fn 有改动但无失败记录被拒() {
        let mut facts = minimal();
        facts.changed_files = vec![("a.rs".to_owned(), 3, 1)];
        let err = facts.validate().expect_err("应当被拒");
        assert!(matches!(err, FactsError::NoFailuresWithChanges { changed: 1 }));
        assert!(err.to_string().contains("重复已失败的方案"));
    }

    #[test]
    fn 有改动且有失败记录通过() {
        let mut facts = minimal();
        facts.changed_files = vec![("a.rs".to_owned(), 3, 1)];
        facts.failures = vec![Failure {
            attempt: "cargo build".to_owned(),
            outcome: "E0308 类型不匹配".to_owned(),
        }];
        assert!(facts.validate().is_ok());
    }

    #[test]
    fn 已否决方案缺理由被拒() {
        let mut facts = minimal();
        facts.rejected = vec![("用文本标签调用工具".to_owned(), "  ".to_owned())];
        let err = facts.validate().expect_err("应当被拒");
        assert!(matches!(err, FactsError::RejectedWithoutReason { .. }));
    }

    #[test]
    fn 轮数为零被拒() {
        let mut facts = minimal();
        facts.turns = 0;
        assert_eq!(facts.validate(), Err(FactsError::EmptySpan));
    }

    #[test]
    fn 渲染包含三类事实与检查点() {
        let facts = Facts {
            changed_files: vec![("a.rs".to_owned(), 3, 1)],
            failures: vec![Failure {
                attempt: "cargo test".to_owned(),
                outcome: "2 个用例失败".to_owned(),
            }],
            rejected: vec![("方案 B".to_owned(), "会让缓存失效".to_owned())],
            checkpoint: Some(checkpoint()),
            turns: 4,
        };
        let text = facts.render();
        assert!(text.contains("a.rs (+3/-1)"));
        assert!(text.contains("cargo test → 2 个用例失败"));
        assert!(text.contains("方案 B：会让缓存失效"));
        assert!(text.contains("覆盖轮数：4"));
        assert!(text.contains(BASE));
        assert!(text.contains("不可修改"));
    }

    #[test]
    fn 无改动时明确写未改动() {
        let text = minimal().render();
        assert!(text.contains("未改动文件"));
        assert!(!text.contains("失败的尝试"), "没有失败时不输出空小节");
    }

    #[test]
    fn 事实段声明不可修改() {
        // 这句话是给模型看的约束：它不该试图改写事实段。
        assert!(minimal().render().contains("不可修改"));
    }
}
