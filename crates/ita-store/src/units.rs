//! unit 状态的读写。
//!
//! unit 是 server 内固定命名的子系统，状态是激活门判定后的三值之一：就绪、降级、失败。
//! 状态落库是为了 server 重启后能恢复 unit 状态，也是 07.4 能力集快照的输入——能力集由
//! unit 状态推导，哪些就绪、哪些降级。
//!
//! 只写不读事件：这里只提供写入与读取，不提供更新与删除以外的操作。写入是权威写，必须在
//! 事务提交后才返回。unit 标识是校验过的标签（小写字母数字加连字符），不是 ULID。

use rusqlite::{Connection, Transaction};

use crate::db::{StoreError, now_millis};

/// unit 的持久化状态。
///
/// 三值域是六状态状态机的投影：内存里的六状态（未启动、启动中、就绪、失败、停止中、已停止）
/// 中只有三种稳定态会跨重启存活。就绪的 unit 记为 [`UnitState::Ready`]，非必需 unit 失败或仍
/// 在启动中记为 [`UnitState::Degraded`]，必需 unit 失败记为 [`UnitState::Failed`]。未启动、
/// 停止中、已停止是瞬态，不落库。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitState {
    /// 就绪：探针通过，功能可用。
    Ready,
    /// 降级：非必需 unit 失败或仍在启动中，功能未可用但不阻塞激活。
    Degraded,
    /// 失败：必需 unit 失败，阻塞激活。
    Failed,
}

impl UnitState {
    /// 取字符串形式，与表里的 `state` 列取值域一致。
    pub fn to_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Degraded => "degraded",
            Self::Failed => "failed",
        }
    }

    /// 从字符串形式解析。
    ///
    /// # 边界与前提
    /// 取值域外的字符串返回 [`StoreError::InvalidUnitState`]。表里的 CHECK 约束已保证落库的
    /// 值必在取值域内，这里是防御性解析，让"库里出现非法状态"显式报错而不是静默吞掉。
    fn from_str(text: &str) -> Result<Self, StoreError> {
        match text {
            "ready" => Ok(Self::Ready),
            "degraded" => Ok(Self::Degraded),
            "failed" => Ok(Self::Failed),
            other => Err(StoreError::InvalidUnitState(other.to_owned())),
        }
    }
}

/// 在已开启的事务内写入一个 unit 的状态。
///
/// # 步骤
/// 1. 取当前时间（epoch 毫秒）。
/// 2. 插入或更新该 unit 的状态行，`updated_at` 记为当前时间。
///
/// # 边界与前提
/// 接收已开启的事务，不自行开事务。写入是 upsert：unit 不存在则建行，已存在则更新状态。
/// 同一 unit 的多次写入只保留最后一次，`updated_at` 反映最后写入时刻。
///
/// # 副作用
/// 写 `units` 表。
pub(crate) fn set_unit_state(
    tx: &Transaction,
    id: &str,
    state: UnitState,
) -> Result<(), StoreError> {
    let now = now_millis()?;
    tx.execute(
        "INSERT INTO units (id, state, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET state = excluded.state, updated_at = excluded.updated_at",
        rusqlite::params![id, state.to_str(), now],
    )?;
    Ok(())
}

/// 读取一个 unit 的状态。
///
/// # 边界与前提
/// unit 不存在时返回 `None`，不报错。状态字符串非法时返回 [`StoreError::InvalidUnitState`]。
///
/// # 副作用
/// 只读。
pub(crate) fn read_unit_state(
    conn: &Connection,
    id: &str,
) -> Result<Option<UnitState>, StoreError> {
    let result =
        conn.query_row("SELECT state FROM units WHERE id = ?1", rusqlite::params![id], |row| {
            row.get::<_, String>(0)
        });
    match result {
        Ok(state_str) => Ok(Some(UnitState::from_str(&state_str)?)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(StoreError::Sqlite(e)),
    }
}

/// 读取全部 unit 的状态，按 unit 标识排序。
///
/// # 边界与前提
/// 空库返回空列表。状态字符串非法时返回 [`StoreError::InvalidUnitState`]。
///
/// # 副作用
/// 只读。
pub(crate) fn read_all_unit_states(
    conn: &Connection,
) -> Result<Vec<(String, UnitState)>, StoreError> {
    let mut stmt = conn.prepare("SELECT id, state FROM units ORDER BY id")?;
    let rows = stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let state_str: String = row.get(1)?;
        Ok((id, state_str))
    })?;
    let mut result = Vec::new();
    for row in rows {
        let (id, state_str) = row?;
        result.push((id, UnitState::from_str(&state_str)?));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Store;

    /// 写入 unit 状态后读回，状态与写入一致。
    #[test]
    fn set_and_read_unit_state_round_trips() {
        let mut store = Store::open_in_memory().expect("应当可以打开");
        let tx = store.conn_mut().transaction().expect("应当可以开事务");
        set_unit_state(&tx, "store", UnitState::Degraded).expect("应当成功");
        tx.commit().expect("应当可以提交");
        let state = read_unit_state(store.conn(), "store").expect("应当可读");
        assert_eq!(state, Some(UnitState::Degraded), "读回的状态应当是 degraded");
    }

    /// 读不存在的 unit 返回 None，不报错。
    #[test]
    fn read_unknown_unit_returns_none() {
        let store = Store::open_in_memory().expect("应当可以打开");
        let state = read_unit_state(store.conn(), "ghost").expect("应当可读");
        assert_eq!(state, None, "不存在的 unit 应当返回 None");
    }

    /// 覆盖写入：同一 unit 的第二次写入取代第一次。
    #[test]
    fn set_unit_state_overwrites_previous() {
        let mut store = Store::open_in_memory().expect("应当可以打开");
        let tx = store.conn_mut().transaction().expect("应当可以开事务");
        set_unit_state(&tx, "store", UnitState::Ready).expect("应当成功");
        tx.commit().expect("应当可以提交");
        let tx = store.conn_mut().transaction().expect("应当可以开事务");
        set_unit_state(&tx, "store", UnitState::Failed).expect("应当成功");
        tx.commit().expect("应当可以提交");
        let state = read_unit_state(store.conn(), "store").expect("应当可读");
        assert_eq!(state, Some(UnitState::Failed), "读回的状态应当是最后一次写入");
    }

    /// 读全部 unit 状态按标识排序。
    #[test]
    fn read_all_unit_states_sorted_by_id() {
        let mut store = Store::open_in_memory().expect("应当可以打开");
        let tx = store.conn_mut().transaction().expect("应当可以开事务");
        set_unit_state(&tx, "router", UnitState::Ready).expect("应当成功");
        set_unit_state(&tx, "store", UnitState::Ready).expect("应当成功");
        set_unit_state(&tx, "model", UnitState::Degraded).expect("应当成功");
        tx.commit().expect("应当可以提交");
        let all = read_all_unit_states(store.conn()).expect("应当可读");
        let ids: Vec<&str> = all.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, vec!["model", "router", "store"], "应当按标识排序");
        assert_eq!(all[0].1, UnitState::Degraded, "model 应当是 degraded");
        assert_eq!(all[1].1, UnitState::Ready, "router 应当是 ready");
        assert_eq!(all[2].1, UnitState::Ready, "store 应当是 ready");
    }

    /// 空库读全部 unit 状态返回空列表。
    #[test]
    fn read_all_unit_states_on_empty_store() {
        let store = Store::open_in_memory().expect("应当可以打开");
        let all = read_all_unit_states(store.conn()).expect("应当可读");
        assert!(all.is_empty(), "空库应当返回空列表");
    }

    /// 三值域的字符串形式与表里的取值域一致。
    #[test]
    fn unit_state_string_forms_match_table_domain() {
        assert_eq!(UnitState::Ready.to_str(), "ready");
        assert_eq!(UnitState::Degraded.to_str(), "degraded");
        assert_eq!(UnitState::Failed.to_str(), "failed");
    }

    /// 非法状态字符串被拒，防御性解析不静默吞掉。
    #[test]
    fn unit_state_from_str_rejects_unknown() {
        let err = UnitState::from_str("running").expect_err("应当被拒");
        assert!(matches!(err, StoreError::InvalidUnitState(_)));
    }
}
