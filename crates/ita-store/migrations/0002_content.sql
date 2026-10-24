-- 内容寻址存储的元数据与快照清单。
--
-- 两条纪律落在这里：
--   * 文件内容不进库：blobs 只有哈希、尺寸与引用计数所需的元数据，正文在文件系统里。
--   * 快照是对齐的依据，不是备份：条目记录"当时这个路径是什么"，墓碑记录"当时这里没有
--     东西"，两者缺一不可。

CREATE TABLE IF NOT EXISTS blobs (
    hash        TEXT PRIMARY KEY NOT NULL,
    size        INTEGER NOT NULL,
    created_at  INTEGER NOT NULL
);

-- 快照。kind 区分基线（baseline）与增量（incremental）：到达保留上限时把最老的两个增量
-- 合并进基线，而不是丢弃最老的——这样"回到任务起点"永远可用，只是颗粒度变粗。
CREATE TABLE IF NOT EXISTS snapshots (
    id            TEXT PRIMARY KEY NOT NULL,
    workspace_id  TEXT NOT NULL REFERENCES workspaces(id),
    seq           INTEGER NOT NULL,
    parent_id     TEXT REFERENCES snapshots(id),
    kind          TEXT NOT NULL,
    -- 重定基线是不可逆操作，必须留痕：谁、为什么、是否经过人工确认。
    rebased_by    TEXT,
    rebase_reason TEXT,
    created_at    INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS snapshots_by_seq ON snapshots(workspace_id, seq);

-- 快照条目。kind 三值：
--   file       当时存在且内容已存（hash 指向 blobs）
--   dir        当时存在且是目录
--   tombstone  当时被删除
-- 超过大小阈值的文件记为 file 但 content_stored = 0，只保留 size 与 mtime：
-- 回滚时这类条目只报告、不删除也不覆盖（0017）。
CREATE TABLE IF NOT EXISTS snapshot_entries (
    snapshot_id     TEXT NOT NULL REFERENCES snapshots(id),
    path            TEXT NOT NULL,
    kind            TEXT NOT NULL,
    hash            TEXT,
    mode            INTEGER,
    size            INTEGER,
    mtime           INTEGER,
    content_stored  INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (snapshot_id, path)
);

CREATE INDEX IF NOT EXISTS entries_by_path ON snapshot_entries(path);

-- 读集：谁在什么时候读到了什么版本。
-- 键是（工作区, 路径, 会话）：同一路径上不同会话各有自己的最后读到的哈希，只记"最后一次
-- 读取"会让陈旧写判定与读者无关，那是错的。
CREATE TABLE IF NOT EXISTS read_set (
    workspace_id  TEXT NOT NULL REFERENCES workspaces(id),
    path          TEXT NOT NULL,
    session_id    TEXT NOT NULL REFERENCES sessions(id),
    last_hash     TEXT NOT NULL,
    last_read_seq INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, path, session_id)
);
