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

/// 一处文件改动。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    /// 路径。
    pub path: String,
    /// 新增行数。
    pub added: u32,
    /// 删除行数。
    pub removed: u32,
}

/// 事实层。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// 被改动的文件。
    pub changed_files: Vec<FileChange>,
    /// 失败的尝试。
    pub failures: Vec<Failure>,
    /// 已否决的方案及理由。
    pub rejected: Vec<(String, String)>,
    /// 这段历史对应的检查点。
    pub checkpoint: Option<CheckpointId>,
    /// 这段历史跨过的轮数。
    pub turns: u32,
    /// 这段历史实际耗时（秒）。用于事后判断"这一轮是不是卡住了"。
    pub elapsed_secs: u64,
}

/// 事实层不完整。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactsError {
    /// 失败记录里有一条只有尝试或只有结果，渲染出来等于什么都没说。
    IncompleteFailure {
        /// 出问题的那条记录的位置。
        index: usize,
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
            Self::IncompleteFailure { index } => {
                write!(f, "第 {index} 条失败记录缺少尝试或结果，渲染出来等于没写")
            }
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
    /// 2. 每条失败记录都要有尝试与结果两半。
    /// 3. 每条已否决方案都要有理由。
    ///
    /// # 边界与前提
    /// **不要求"有改动就必须有失败"**：一个只改了一行注释、一次编译就过的批次确实可能
    /// 没有失败，把它判为不合格会让压缩在实践中频繁失败。规则改为"失败小节永远输出"，
    /// 没有失败时明确写"无失败尝试"——这比靠改动数推断可靠，也保住了"不要重复已失败方案"
    /// 这个原始意图。
    ///
    /// 只做结构校验，不判断内容质量。"失败记录写得是否准确"无法自动判定，只能靠"压缩后
    /// 是否重复已否决操作"这个离线指标来观察。
    ///
    /// # 副作用
    /// 无。
    pub fn validate(&self) -> Result<(), FactsError> {
        if self.turns == 0 {
            return Err(FactsError::EmptySpan);
        }
        for (index, failure) in self.failures.iter().enumerate() {
            if failure.attempt.trim().is_empty() || failure.outcome.trim().is_empty() {
                return Err(FactsError::IncompleteFailure { index });
            }
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
        // 不用"不可修改"这类祈使句：模型会倾向于在散文里复述约束，浪费预算。
        // 用陈述句说明来源即可。
        let mut out = String::from("事实记录：\n");

        if self.changed_files.is_empty() {
            out.push_str("- 未改动文件\n");
        } else {
            out.push_str("- 改动文件：\n");
            for change in &self.changed_files {
                out.push_str(&format!(
                    "  - {} (+{}/-{})\n",
                    change.path, change.added, change.removed
                ));
            }
        }

        // 失败小节永远输出：没有失败时明确写"无"，比省略更能阻止模型重复尝试。
        if self.failures.is_empty() {
            out.push_str("- 失败的尝试：无\n");
        } else {
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
        out.push_str(&format!("- 实际耗时：{} 秒\n", self.elapsed_secs));
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
            elapsed_secs: 30,
        }
    }

    fn change(path: &str) -> FileChange {
        FileChange { path: path.to_owned(), added: 3, removed: 1 }
    }

    fn failure(attempt: &str, outcome: &str) -> Failure {
        Failure { attempt: attempt.to_owned(), outcome: outcome.to_owned() }
    }

    /// 无改动无失败的空事实通过校验，什么都没做的批次也是合法的摘要。
    #[test]
    fn empty_facts_without_changes_pass_validation() {
        assert!(minimal().validate().is_ok());
    }

    /// 有改动但无失败也允许，一次编译就过的批次不会被判为不合格。
    #[test]
    fn changes_without_failures_are_allowed() {
        // 只改了一行注释、一次编译就过的批次确实可能没有失败；把它判为不合格会让压缩
        // 频繁失败。规则改为"失败小节永远输出"。
        let mut facts = minimal();
        facts.changed_files = vec![change("a.rs")];
        assert!(facts.validate().is_ok());
    }

    /// 失败小节永远输出，无失败时明确写"无"，模型不会猜失败被省略了。
    #[test]
    fn failure_section_always_output_with_explicit_none() {
        let mut facts = minimal();
        facts.changed_files = vec![change("a.rs")];
        let text = facts.render();
        assert!(text.contains("失败的尝试：无"), "没有失败时必须明确写出来");
    }

    /// 只有失败没有改动也允许，失败但没改文件的批次也是合法的摘要。
    #[test]
    fn failures_without_changes_are_allowed() {
        let mut facts = minimal();
        facts.failures = vec![failure("cargo build", "E0308")];
        assert!(facts.validate().is_ok());
        assert!(facts.render().contains("未改动文件"));
    }

    /// 失败记录缺尝试或结果两半之一即被拒，半空的失败永远不会被产出。
    #[test]
    fn failure_record_missing_either_half_is_rejected() {
        for (attempt, outcome) in [("cargo build", "  "), ("", "E0308")] {
            let mut facts = minimal();
            facts.failures = vec![failure(attempt, outcome)];
            let err = facts.validate().expect_err("应当被拒");
            assert!(matches!(err, FactsError::IncompleteFailure { index: 0 }));
        }
    }

    /// 已否决方案缺理由被拒，摘要不会只说"否决了 X"而不说为什么。
    #[test]
    fn rejected_option_without_reason_is_rejected() {
        let mut facts = minimal();
        facts.rejected = vec![("用文本标签调用工具".to_owned(), "  ".to_owned())];
        let err = facts.validate().expect_err("应当被拒");
        assert!(matches!(err, FactsError::RejectedWithoutReason { .. }));
    }

    /// 多条否决里只要有一条缺理由整份事实就被拒，单个缺口不会被悄悄忽略。
    #[test]
    fn single_missing_reason_among_many_rejections_is_rejected() {
        let mut facts = minimal();
        facts.rejected = vec![
            ("方案 A".to_owned(), "缓存会失效".to_owned()),
            ("方案 B".to_owned(), "".to_owned()),
        ];
        let err = facts.validate().expect_err("应当被拒");
        assert!(matches!(err, FactsError::RejectedWithoutReason { ref name } if name == "方案 B"));
    }

    /// 轮数为零优先于其它规则，调用方先看到主错误而不是次要错误。
    #[test]
    fn zero_turns_takes_priority_over_other_rules() {
        let mut facts = minimal();
        facts.turns = 0;
        facts.failures = vec![failure("", "")];
        // 优先级：轮数先判，避免调用方看到一条次要错误。
        assert_eq!(facts.validate(), Err(FactsError::EmptySpan));
    }

    /// 渲染包含五类事实——改动、失败、否决、覆盖轮数、耗时——摘要完整。
    #[test]
    fn rendering_contains_all_five_fact_categories() {
        let facts = Facts {
            changed_files: vec![change("a.rs")],
            failures: vec![failure("cargo test", "2 个用例失败")],
            rejected: vec![("方案 B".to_owned(), "会让缓存失效".to_owned())],
            checkpoint: Some(checkpoint()),
            turns: 4,
            elapsed_secs: 125,
        };
        let text = facts.render();
        assert!(text.contains("a.rs (+3/-1)"));
        assert!(text.contains("cargo test → 2 个用例失败"));
        assert!(text.contains("方案 B：会让缓存失效"));
        assert!(text.contains("覆盖轮数：4"));
        assert!(text.contains("实际耗时：125 秒"));
        assert!(text.contains(BASE));
    }

    /// 渲染用陈述句而不是祈使句，模型不会被诱导在散文里复述约束。
    #[test]
    fn rendering_uses_declarative_not_imperative() {
        // "不可修改"这类祈使句会诱导模型在散文里复述约束，浪费预算。
        let text = minimal().render();
        assert!(!text.contains("不可修改"));
        assert!(text.starts_with("事实记录："));
    }

    /// 无改动时明确写"未改动文件"，模型不会猜该字段被省略了。
    #[test]
    fn no_changes_rendered_as_explicit_no_changes() {
        assert!(minimal().render().contains("未改动文件"));
    }
}
