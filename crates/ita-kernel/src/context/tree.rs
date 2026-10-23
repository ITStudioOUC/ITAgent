//! 上下文树：节点集合、会话 head 与比较并交换。
//!
//! 这里实现的是"会话看到哪一段历史"的唯一可变状态。节点本身不可变，所以整个树只是
//! 一个节点池加一组 head 指针；回退是把 head 指回祖先，分叉是新建一个会话指向某个祖先，
//! 两者都不复制节点。
//!
//! head 的更新一律走比较并交换：只有"我以为的当前值"仍是当前值时才允许写入。否则两个
//! 并发推进会让后写者静默覆盖前者的分支，而这种丢失不会报错。

use std::collections::BTreeMap;

use super::ids::{NodeId, SessionId};
use super::node::Node;

/// head 令牌：一个会话当前指向哪个节点，以及它被推进过多少次。
///
/// 只带 `NodeId` 不够：回退到祖先再前进会让节点 id 重新变成"当前值"，此时一个过期的
/// 写入会通过比较。代次是单调的，因此能区分"同一个节点，但不是同一次推进"。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadToken {
    /// 当前指向的节点。
    pub node: NodeId,
    /// 该会话的推进代次，从零开始，每次成功推进加一。
    pub generation: u64,
}

/// 一次写入失败的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeError {
    /// 期望的 head 与当前 head 不一致：有人先推进了。
    HeadConflict {
        /// 调用方以为的令牌。
        expected: HeadToken,
        /// 实际令牌。
        actual: HeadToken,
    },
    /// 引用了不存在的节点。
    UnknownNode(NodeId),
    /// 引用了不存在的会话。
    UnknownSession(SessionId),
    /// 新节点的父节点与会话当前 head 不连续。
    ///
    /// 追加只允许接在当前 head 之后；要接到别处必须先回退 head，这样"分支"永远是显式
    /// 动作而不是意外。
    NotDescendantOfHead {
        /// 父节点。
        parent: NodeId,
        /// 会话当前 head。
        head: NodeId,
    },
    /// 节点 id 已存在。节点不可变，因此重复 id 一律拒绝而不是覆盖。
    DuplicateNode(NodeId),
}

impl std::fmt::Display for TreeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HeadConflict { expected, actual } => write!(
                f,
                "head 已被推进：期望 {} 第 {} 代，实际 {} 第 {} 代",
                expected.node, expected.generation, actual.node, actual.generation
            ),
            Self::UnknownNode(id) => write!(f, "未知节点 {id}"),
            Self::UnknownSession(id) => write!(f, "未知会话 {id}"),
            Self::NotDescendantOfHead { parent, head } => {
                write!(f, "父节点 {parent} 不是当前 head {head}，追加前需要先移动 head")
            }
            Self::DuplicateNode(id) => write!(f, "节点 {id} 已存在"),
        }
    }
}

impl std::error::Error for TreeError {}

/// 上下文树。
///
/// 只做内存里的结构维护；持久化与事务边界属于存储层。这样"分叉是否合法"这类判断可以
/// 在不碰数据库的情况下穷举测试。
#[derive(Debug, Default)]
pub struct Tree {
    nodes: BTreeMap<NodeId, Node>,
    heads: BTreeMap<SessionId, HeadToken>,
}

impl Tree {
    /// 建一棵空树。
    pub fn new() -> Self {
        Self::default()
    }

    /// 取一个节点。
    pub fn node(&self, id: &NodeId) -> Option<&Node> {
        self.nodes.get(id)
    }

    /// 取一个会话的 head 令牌。
    pub fn head(&self, session: &SessionId) -> Option<HeadToken> {
        self.heads.get(session).cloned()
    }

    /// 节点总数。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// 从某个节点起向根回溯，按从近到远返回祖先链（不含起点）。
    ///
    /// # 边界与前提
    /// 跨会话回溯：父边可以指向另一个会话的节点（分叉共享祖先），因此这里不按会话过滤。
    /// 遇到未知父节点即停止——树可能只加载了部分节点，静默截断比报错更危险，所以由
    /// 调用方通过返回长度与节点数自行判断完整性。
    ///
    /// # 副作用
    /// 无。
    pub fn ancestors(&self, from: &NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut cursor = self.nodes.get(from).and_then(|n| n.parent.clone());
        while let Some(id) = cursor {
            let Some(node) = self.nodes.get(&id) else {
                break;
            };
            out.push(id);
            cursor = node.parent.clone();
        }
        out
    }

    /// 建立一个会话并写入它的第一个节点。
    ///
    /// # 边界与前提
    /// 根节点必须没有父节点，且会话与节点 id 都不能已存在。会话的第一次推进与后续追加
    /// 分开，是因为根节点没有父可校验，把它塞进 `append` 会让那个校验多一个例外分支。
    ///
    /// # 副作用
    /// 修改节点池与会话表。
    pub fn init(&mut self, session: SessionId, root: Node) -> Result<HeadToken, TreeError> {
        if self.heads.contains_key(&session) {
            return Err(TreeError::UnknownSession(session));
        }
        if self.nodes.contains_key(&root.id) {
            return Err(TreeError::DuplicateNode(root.id.clone()));
        }
        if root.parent.is_some() {
            return Err(TreeError::NotDescendantOfHead {
                parent: root.parent.clone().unwrap_or_else(|| root.id.clone()),
                head: root.id.clone(),
            });
        }
        let node_id = root.id.clone();
        let token = HeadToken { node: node_id.clone(), generation: 0 };
        self.nodes.insert(node_id, root);
        self.heads.insert(session, token.clone());
        Ok(token)
    }

    /// 新建一个会话，head 指向给定节点。
    ///
    /// # 边界与前提
    /// 该节点必须已存在。分叉就是"新会话指向某个祖先"，因此这里不需要额外参数。
    ///
    /// # 副作用
    /// 修改会话表。
    pub fn fork(&mut self, session: SessionId, at: NodeId) -> Result<HeadToken, TreeError> {
        if !self.nodes.contains_key(&at) {
            return Err(TreeError::UnknownNode(at));
        }
        let token = HeadToken { node: at, generation: 0 };
        self.heads.insert(session, token.clone());
        Ok(token)
    }

    /// 追加一个新节点，并把 head 推进到它。
    ///
    /// # 步骤
    /// 1. 校验会话存在、期望令牌与当前一致。
    /// 2. 校验节点 id 未被占用。
    /// 3. 校验新节点的父就是当前 head。
    /// 4. 写入节点并推进 head，代次加一。
    ///
    /// # 边界与前提
    /// 追加只能接在当前 head 之后。要接到祖先上，必须先回退 head——这让"分支"成为显式
    /// 动作，避免两个并发调用各自从同一祖先长出分支而不自知。
    ///
    /// # 副作用
    /// 修改节点池与会话表。
    pub fn append(
        &mut self,
        session: &SessionId,
        expected: HeadToken,
        node: Node,
    ) -> Result<HeadToken, TreeError> {
        let actual =
            self.heads.get(session).cloned().ok_or(TreeError::UnknownSession(session.clone()))?;
        if actual != expected {
            return Err(TreeError::HeadConflict { expected, actual });
        }
        if self.nodes.contains_key(&node.id) {
            return Err(TreeError::DuplicateNode(node.id));
        }
        if node.parent.as_ref() != Some(&actual.node) {
            return Err(TreeError::NotDescendantOfHead {
                parent: node.parent.clone().unwrap_or_else(|| node.id.clone()),
                head: actual.node,
            });
        }

        let node_id = node.id.clone();
        let token = HeadToken { node: node_id.clone(), generation: actual.generation + 1 };
        self.nodes.insert(node_id, node);
        self.heads.insert(session.clone(), token.clone());
        Ok(token)
    }

    /// 把 head 移到一个祖先节点。
    ///
    /// # 边界与前提
    /// 目标必须是当前 head 的祖先，否则会允许"跳到别人的分支上"——那不是回退，是合并，
    /// 而合并需要显式动作与明确的作者。代次继续加一，因此回退之后旧令牌依然失效。
    ///
    /// # 副作用
    /// 修改会话表。
    pub fn rewind(
        &mut self,
        session: &SessionId,
        expected: HeadToken,
        to: NodeId,
    ) -> Result<HeadToken, TreeError> {
        let actual =
            self.heads.get(session).cloned().ok_or(TreeError::UnknownSession(session.clone()))?;
        if actual != expected {
            return Err(TreeError::HeadConflict { expected, actual });
        }
        if !self.nodes.contains_key(&to) {
            return Err(TreeError::UnknownNode(to));
        }
        if to != actual.node && !self.ancestors(&actual.node).contains(&to) {
            return Err(TreeError::NotDescendantOfHead { parent: to, head: actual.node });
        }
        let token = HeadToken { node: to, generation: actual.generation + 1 };
        self.heads.insert(session.clone(), token.clone());
        Ok(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::node::{NodeBody, TurnRole};

    const BASE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    fn id(n: u8) -> NodeId {
        let mut text = BASE.to_owned();
        text.replace_range(25..26, &char::from(b'0' + n % 10).to_string());
        NodeId::parse(text).expect("应当合法")
    }

    fn session(n: u8) -> SessionId {
        let mut text = BASE.to_owned();
        text.replace_range(24..26, &format!("{n:02}"));
        SessionId::parse(text).expect("应当合法")
    }

    fn turn(node_id: NodeId, parent: Option<NodeId>, owner: SessionId) -> Node {
        Node {
            id: node_id,
            parent,
            session: owner,
            body: NodeBody::Turn { role: TurnRole::Assistant, text: "内容".to_owned() },
        }
    }

    /// 建一条三节点的线性历史。
    fn linear() -> (Tree, SessionId, HeadToken) {
        let s = session(1);
        let mut tree = Tree::new();
        let root = id(1);
        let token =
            tree.init(s.clone(), turn(root.clone(), None, s.clone())).expect("根节点应当可以建");
        let token = tree
            .append(&s, token.clone(), turn(id(2), Some(root), s.clone()))
            .expect("追加应当成功");
        let token = tree
            .append(&s, token.clone(), turn(id(3), Some(id(2)), s.clone()))
            .expect("追加应当成功");
        (tree, s, token)
    }

    #[test]
    fn 追加推进_head_并增加代次() {
        let (tree, s, token) = linear();
        assert_eq!(token.node, id(3));
        assert_eq!(token.generation, 2);
        assert_eq!(tree.head(&s), Some(token.clone()));
        assert_eq!(tree.len(), 3);
    }

    #[test]
    fn 过期令牌的追加被拒() {
        let (mut tree, s, token) = linear();
        let stale = HeadToken { node: id(1), generation: 0 };
        let err =
            tree.append(&s, stale, turn(id(4), Some(id(1)), s.clone())).expect_err("应当被拒");
        assert!(matches!(err, TreeError::HeadConflict { .. }));
        // 拒绝之后 head 不变。
        assert_eq!(tree.head(&s), Some(token.clone()));
    }

    #[test]
    fn 追加的父必须就是当前_head() {
        let (mut tree, s, token) = linear();
        let err = tree
            .append(&s, token.clone(), turn(id(4), Some(id(1)), s.clone()))
            .expect_err("应当被拒");
        assert!(matches!(err, TreeError::NotDescendantOfHead { .. }));
    }

    #[test]
    fn 重复的节点_id_被拒而不是覆盖() {
        let (mut tree, s, token) = linear();
        let err = tree
            .append(&s, token.clone(), turn(id(2), Some(id(3)), s.clone()))
            .expect_err("应当被拒");
        assert!(matches!(err, TreeError::DuplicateNode(_)));
    }

    #[test]
    fn 祖先链跨会话不截断() {
        let (mut tree, parent_session, token) = linear();
        // 子会话从父会话的中间节点分叉。
        let child = session(2);
        let child_head = tree.fork(child.clone(), id(2)).expect("应当可以分叉");
        assert_eq!(child_head.node, id(2));
        assert_eq!(child_head.generation, 0);

        // 子会话继续推进，它的节点 parent 指向父会话的节点。
        let token2 = tree
            .append(&child, child_head.clone(), turn(id(4), Some(id(2)), child.clone()))
            .expect("追加应当成功");
        assert_eq!(tree.ancestors(&id(4)), vec![id(2), id(1)]);

        // 父会话的 head 不受影响。
        assert_eq!(tree.head(&parent_session), Some(token.clone()));
        assert_eq!(token2.node, id(4));
    }

    #[test]
    fn 分叉不复制祖先节点() {
        let (mut tree, _, _) = linear();
        let before = tree.len();
        tree.fork(session(2), id(2)).expect("应当可以分叉");
        tree.fork(session(3), id(3)).expect("应当可以分叉");
        assert_eq!(tree.len(), before, "分叉不应新增节点");
    }

    #[test]
    fn 回退到祖先再前进时旧令牌失效() {
        let (mut tree, s, token) = linear();
        let rewound = tree.rewind(&s, token.clone(), id(2)).expect("回退应当成功");
        assert_eq!(rewound.node, id(2));
        assert_eq!(rewound.generation, token.generation + 1);

        // 回退之后，指向同一节点的旧令牌仍然过期：代次不同。
        let stale = HeadToken { node: id(3), generation: token.generation };
        assert!(tree.rewind(&s, stale, id(1)).is_err());
    }

    #[test]
    fn 回退之后可以从祖先长出另一条分支() {
        let (mut tree, s, token) = linear();
        let rewound = tree.rewind(&s, token.clone(), id(2)).expect("回退应当成功");
        let branched = tree
            .append(&s, rewound, turn(id(4), Some(id(2)), s.clone()))
            .expect("应当可以长出分支");
        assert_eq!(branched.node, id(4));
        // 原来的 id(3) 仍在池里，可以按 id 取回。
        assert!(tree.node(&id(3)).is_some());
    }

    #[test]
    fn 回退只能到祖先() {
        let (mut tree, s, token) = linear();
        let other = session(9);
        tree.fork(other, id(1)).expect("应当可以分叉");
        let err = tree.rewind(&s, token.clone(), id(9)).expect_err("应当被拒");
        assert!(matches!(err, TreeError::UnknownNode(_)));
    }

    #[test]
    fn 未知会话被拒() {
        let (mut tree, _, _) = linear();
        let ghost = session(7);
        let err = tree
            .append(
                &ghost,
                HeadToken { node: id(1), generation: 0 },
                turn(id(8), Some(id(1)), ghost.clone()),
            )
            .expect_err("应当被拒");
        assert!(matches!(err, TreeError::UnknownSession(_)));
    }

    #[test]
    fn 根节点必须没有父() {
        let mut tree = Tree::new();
        let s = session(1);
        let err = tree.init(s.clone(), turn(id(1), Some(id(2)), s)).expect_err("应当被拒");
        assert!(matches!(err, TreeError::NotDescendantOfHead { .. }));
    }

    #[test]
    fn 同一会话不能重复建立() {
        let mut tree = Tree::new();
        let s = session(1);
        tree.init(s.clone(), turn(id(1), None, s.clone())).expect("首次应当成功");
        let err = tree.init(s.clone(), turn(id(2), None, s)).expect_err("应当被拒");
        assert!(matches!(err, TreeError::UnknownSession(_)));
    }

    #[test]
    fn 在未知节点上分叉被拒() {
        let mut tree = Tree::new();
        assert!(matches!(tree.fork(session(1), id(5)), Err(TreeError::UnknownNode(_))));
    }

    #[test]
    fn 回退到当前_head_是允许的空操作() {
        let (mut tree, s, token) = linear();
        let same = tree.rewind(&s, token.clone(), id(3)).expect("应当成功");
        assert_eq!(same.node, id(3));
        assert_eq!(same.generation, token.generation + 1);
    }
}
