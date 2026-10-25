//! 迁移运行器。
//!
//! 迁移以编号脚本的形式内嵌在二进制里，启动时按序执行、记录已执行的版本。三条纪律：
//!
//! - **只加不改**：迁移只允许建表与加列。事件表是不可变真相，删列或改名会让历史无法
//!   replay，也会让 `--locked` 之外的漂移难以发现。
//! - **可重复执行**：同一个迁移跑两次不能有副作用，因此每条迁移都自带 `IF NOT EXISTS`。
//! - **单事务**：一次启动里的全部待执行迁移在一个事务内完成，失败则整体回滚，不留半迁移
//!   的库。
//!
//! 版本号记在 `schema_version` 表里，而不是依赖 SQLite 的 `user_version`——后者是单值，
//! 无法表达"哪些迁移已经执行过"。

use rusqlite::Connection;

/// 一条迁移。
pub struct Migration {
    /// 版本号，从 1 开始，必须连续。
    pub version: u32,
    /// 人类可读的名字，出现在日志里。
    pub name: &'static str,
    /// SQL 语句，可多条，用分号分隔。
    pub sql: &'static str,
}

/// 迁移失败的原因。
#[derive(Debug)]
pub enum MigrateError {
    /// 数据库操作失败。
    Sqlite(rusqlite::Error),
    /// 已执行的迁移脚本内容与当前二进制里的不一致。
    ///
    /// 这说明有人改了已发布的迁移文件。同一个版本号对应两套表结构，新库与老库会分叉。
    ScriptChanged {
        /// 出问题的版本。
        version: u32,
    },
    /// 数据库的版本高于本二进制支持的版本。
    ///
    /// 旧二进制读写新表结构不会立刻报错，只会在很久之后以奇怪的方式失败。
    DatabaseNewer {
        /// 库里的版本。
        database: u32,
        /// 本二进制支持的最高版本。
        supported: u32,
    },
    /// 已执行的版本号出现空洞，说明有人手工改过库。
    VersionGap {
        /// 期望的版本。
        expected: u32,
        /// 实际找到的版本。
        found: u32,
    },
}

impl std::fmt::Display for MigrateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(e) => write!(f, "数据库错误：{e}"),
            Self::ScriptChanged { version } => write!(
                f,
                "迁移 {version} 的脚本内容与已执行的不一致；已发布的迁移不允许修改，请新增一条"
            ),
            Self::DatabaseNewer { database, supported } => {
                write!(f, "数据库版本 {database} 高于本程序支持的 {supported}；请升级程序")
            }
            Self::VersionGap { expected, found } => {
                write!(f, "迁移版本不连续：期望 {expected}，实际 {found}")
            }
        }
    }
}

impl std::error::Error for MigrateError {}

impl From<rusqlite::Error> for MigrateError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}

/// 全部迁移，按版本升序。
///
/// 新增迁移只能追加到末尾，且版本号必须紧接上一个。
pub const MIGRATIONS: &[Migration] = &[
    Migration { version: 1, name: "core", sql: include_str!("../migrations/0001_core.sql") },
    Migration { version: 2, name: "content", sql: include_str!("../migrations/0002_content.sql") },
];

/// 执行全部待执行的迁移。
///
/// # 步骤
/// 1. 建版本表（如果不存在）。
/// 2. 读出已执行的版本号，校验连续。
/// 3. 在一个事务里执行所有更高版本的迁移，并记录版本。
///
/// # 边界与前提
/// 必须在一个独占连接上调用（启动阶段）。并发迁移会互相阻塞，而 SQLite 的单写者语义
/// 会让第二个事务等待到超时。
///
/// # 副作用
/// 建表、改库结构、写版本表。
///
/// # 注意事项
/// 校验版本连续是为了尽早发现"有人手工删了版本行"或"跑了旧版本二进制"。这两种情况下
/// 继续迁移会让表结构与代码假设不一致，而失败只会在很久之后以奇怪的方式出现。
pub fn run(conn: &mut Connection) -> Result<u32, MigrateError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
             version INTEGER PRIMARY KEY,
             name    TEXT NOT NULL,
             applied_at INTEGER NOT NULL,
             script_hash TEXT
         );",
    )?;

    let applied: Vec<(u32, Option<String>)> = {
        let mut stmt =
            conn.prepare("SELECT version, script_hash FROM schema_version ORDER BY version")?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, u32>(0)?, row.get::<_, Option<String>>(1)?)))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    for (index, (version, _)) in applied.iter().enumerate() {
        let expected = u32::try_from(index).unwrap_or(u32::MAX) + 1;
        if *version != expected {
            return Err(MigrateError::VersionGap { expected, found: *version });
        }
    }

    // 已执行的脚本内容必须与当前二进制里的一致。改了已发布的迁移而不新增版本，会让新库
    // 与老库的表结构分叉，而两边都不会报错。
    for (version, recorded) in &applied {
        let Some(migration) = MIGRATIONS.iter().find(|m| m.version == *version) else {
            continue;
        };
        // 不用 let-chains：rust-version 锚在 1.85，而该语法 1.88 才稳定。
        if let Some(recorded) = recorded {
            if recorded != &script_hash(migration.sql) {
                return Err(MigrateError::ScriptChanged { version: *version });
            }
        }
    }

    let supported = MIGRATIONS.iter().map(|m| m.version).max().unwrap_or(0);
    let database = applied.last().map(|(v, _)| *v).unwrap_or(0);
    if database > supported {
        return Err(MigrateError::DatabaseNewer { database, supported });
    }

    let current = applied.len() as u32;
    let pending: Vec<&Migration> = MIGRATIONS.iter().filter(|m| m.version > current).collect();
    if pending.is_empty() {
        return Ok(current);
    }

    // 立即事务：一开始就拿写锁。延迟事务会在提交时才升级锁，在 WAL 下可能拿到
    // SQLITE_BUSY_SNAPSHOT——它不是"重试这条语句"能解决的，必须整笔重来。
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    for migration in &pending {
        tx.execute_batch(migration.sql)?;
        tx.execute(
            "INSERT INTO schema_version (version, name, applied_at, script_hash)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                migration.version,
                migration.name,
                // 时间在这里是允许的：存储层不在内核里，它本来就接触时钟。
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0),
                script_hash(migration.sql)
            ],
        )?;
    }
    tx.commit()?;

    Ok(MIGRATIONS.iter().map(|m| m.version).max().unwrap_or(0))
}

/// FNV-1a 64 的偏移基数。
const FNV1A64_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 的质数。
const FNV1A64_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 按 FNV-1a 64 计算字节串的哈希。
///
/// 逐字节异或后做回绕乘法。必须用 `wrapping_mul`：普通乘法在 debug 下会溢出
/// panic、在 release 下回绕，两种构建模式的行为不一致。
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = FNV1A64_OFFSET_BASIS;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV1A64_PRIME);
    }
    hash
}

/// 迁移脚本的内容指纹。
///
/// 算法固定为 FNV-1a 64，不经过标准库的哈希实现：标准库的默认哈希器不保证跨
/// Rust 版本稳定，而指纹不一致会让既有数据库以 `ScriptChanged` 拒绝启动。前缀用于
/// 未来换算法时识别旧值；指纹只用于发现意外改动，不承担防篡改。
fn script_hash(sql: &str) -> String {
    format!("fnv1a64:{:016x}", fnv1a64(sql.as_bytes()))
}

/// 当前已执行的最高版本。
///
/// # 副作用
/// 只读。
pub fn current_version(conn: &Connection) -> Result<u32, rusqlite::Error> {
    conn.query_row("SELECT COALESCE(MAX(version), 0) FROM schema_version", [], |row| row.get(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory() -> Connection {
        let conn = Connection::open_in_memory().expect("内存库应当可以打开");
        conn.pragma_update(None, "foreign_keys", "ON").expect("应当可以开启外键");
        conn
    }

    #[test]
    fn 首次迁移建表并记录版本() {
        let mut conn = memory();
        let version = run(&mut conn).expect("迁移应当成功");
        assert_eq!(version, 2);
        assert_eq!(current_version(&conn).expect("应当可读"), 2);
    }

    #[test]
    fn 重复执行不重复建表也不报错() {
        let mut conn = memory();
        run(&mut conn).expect("首次应当成功");
        let version = run(&mut conn).expect("重复执行应当成功");
        assert_eq!(version, 2);

        let count: u32 = conn
            .query_row("SELECT COUNT(*) FROM schema_version", [], |row| row.get(0))
            .expect("应当可读");
        assert_eq!(count, 2, "版本行不应重复");
    }

    #[test]
    fn 版本出现空洞时拒绝继续() {
        let mut conn = memory();
        run(&mut conn).expect("首次应当成功");
        // 手工制造空洞：删掉版本 1 再插入 3。
        conn.execute("DELETE FROM schema_version", []).expect("应当可删");
        conn.execute(
            "INSERT INTO schema_version (version, name, applied_at) VALUES (3, 'x', 0)",
            [],
        )
        .expect("应当可插");

        let err = run(&mut conn).expect_err("应当被拒");
        assert!(matches!(err, MigrateError::VersionGap { expected: 1, found: 3 }));
    }

    #[test]
    fn 已执行脚本内容变化时拒绝启动() {
        let mut conn = memory();
        run(&mut conn).expect("首次应当成功");
        // 模拟"有人改了已发布的迁移脚本"：把记录的哈希改掉。
        conn.execute("UPDATE schema_version SET script_hash = 'deadbeef'", []).expect("应当可改");
        let err = run(&mut conn).expect_err("应当被拒");
        assert!(matches!(err, MigrateError::ScriptChanged { version: 1 }));
    }

    #[test]
    fn 数据库版本高于程序支持时拒绝启动() {
        let mut conn = memory();
        run(&mut conn).expect("首次应当成功");
        conn.execute(
            "INSERT INTO schema_version (version, name, applied_at, script_hash)
             VALUES (3, 'future', 0, NULL)",
            [],
        )
        .expect("应当可插");
        let err = run(&mut conn).expect_err("应当被拒");
        assert!(matches!(err, MigrateError::DatabaseNewer { database: 3, supported: 2 }));
    }

    #[test]
    fn 脚本里不允许破坏性语句() {
        // 纪律靠扫描守住：只加不改不是靠自觉。
        for migration in MIGRATIONS {
            let upper = migration.sql.to_uppercase();
            for forbidden in ["DROP TABLE", "DELETE FROM", "VACUUM", "PRAGMA"] {
                assert!(!upper.contains(forbidden), "迁移 {} 含 {forbidden}", migration.version);
            }
        }
    }

    #[test]
    fn 脚本可重复执行() {
        let mut conn = memory();
        run(&mut conn).expect("首次应当成功");
        // 表已存在时再跑一遍迁移 SQL 不应失败（脚本自带 IF NOT EXISTS）。
        conn.execute_batch(MIGRATIONS[0].sql).expect("重复执行 SQL 应当成功");
        conn.execute_batch(MIGRATIONS[1].sql).expect("重复执行 SQL 应当成功");
    }

    /// 指纹匹配 FNV-1a 64 的公开向量：空串与单字节钉住常量，foobar 覆盖多轮进位。
    #[test]
    fn script_hash_matches_known_vectors() {
        assert_eq!(script_hash(""), "fnv1a64:cbf29ce484222325");
        assert_eq!(script_hash("a"), "fnv1a64:af63dc4c8601ec8c");
        assert_eq!(script_hash("foobar"), "fnv1a64:85944171f73967e8");
    }

    /// 同一脚本多次计算结果相同：指纹里不能混入随机量或时间。
    #[test]
    fn script_hash_is_stable_for_same_input() {
        let sql = "CREATE TABLE t (id TEXT PRIMARY KEY);";
        assert_eq!(script_hash(sql), script_hash(sql));
    }

    /// 内容变化必然换指纹：这是发现已发布迁移被改动的唯一判据。
    #[test]
    fn script_hash_changes_with_content() {
        assert_ne!(script_hash("SELECT 1"), script_hash("SELECT 2"));
    }

    /// 多字节脚本按 UTF-8 字节计算而不是按字符，这条钉住迭代单位。
    #[test]
    fn script_hash_uses_utf8_bytes() {
        assert_eq!(script_hash("迁移"), "fnv1a64:b07037ac985004f6");
    }
}
