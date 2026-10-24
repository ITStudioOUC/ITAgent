-- 核心表：工作区、会话、节点。
--
-- 三条约束写在这里而不是代码里，因为它们是数据的性质而不是某一处调用的检查：
--   * 业务表默认带 workspace_id，跨工作区的行永远不会被同一条查询命中。内容寻址存储的两
--     张表是明确的例外：blobs 按内容哈希全局去重，跨工作区共享同一份；快照条目不带工作区
--     列，按快照 id join 回 snapshots 才能确定归属。
--   * 节点不可变：没有 UPDATE 路径，只有 INSERT。
--   * head 的比较并交换靠 (session_id, head_node, head_generation) 三者一起匹配。

CREATE TABLE IF NOT EXISTS workspaces (
    id          TEXT PRIMARY KEY NOT NULL,
    root_path   TEXT NOT NULL,
    created_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
    id                TEXT PRIMARY KEY NOT NULL,
    workspace_id      TEXT NOT NULL REFERENCES workspaces(id),
    -- 分叉产生的会话记录父会话与分叉点，用于追溯来源。
    parent_session    TEXT REFERENCES sessions(id),
    fork_node         TEXT,
    head_node         TEXT NOT NULL,
    head_generation   INTEGER NOT NULL DEFAULT 0,
    created_at        INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS sessions_by_workspace ON sessions(workspace_id);

-- 节点表。body_kind 区分内容种类，正文本身可能很大，因此只存句柄与预览。
CREATE TABLE IF NOT EXISTS nodes (
    id            TEXT PRIMARY KEY NOT NULL,
    workspace_id  TEXT NOT NULL REFERENCES workspaces(id),
    session_id    TEXT NOT NULL REFERENCES sessions(id),
    parent_id     TEXT REFERENCES nodes(id),
    body_kind     TEXT NOT NULL,
    -- 小正文内联；大正文走内容寻址存储，这里只留句柄。
    inline_body   TEXT,
    body_hash     TEXT,
    body_len      INTEGER,
    created_at    INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS nodes_by_session ON nodes(session_id);
CREATE INDEX IF NOT EXISTS nodes_by_parent ON nodes(parent_id);

-- 事件日志：append-only，按工作区单调递增的 seq 排序。
-- seq 是断线续传的游标，也是"事件不可变"的排序依据；不用全局 rowid 是因为它跨工作区
-- 交错，无法表达"某个工作区内的第 N 条"。
CREATE TABLE IF NOT EXISTS events (
    workspace_id  TEXT NOT NULL REFERENCES workspaces(id),
    seq           INTEGER NOT NULL,
    id            TEXT NOT NULL,
    session_id    TEXT,
    node_id       TEXT,
    kind          TEXT NOT NULL,
    actor         TEXT NOT NULL,
    payload_hash  TEXT,
    payload_len   INTEGER,
    preview_head  TEXT,
    preview_tail  TEXT,
    created_at    INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, seq)
);

CREATE INDEX IF NOT EXISTS events_by_session ON events(session_id, seq);

-- 每个工作区的 seq 计数器。分配 seq 必须与插入事件在同一个事务里，
-- 因此这里只存"下一个可用值"，由存储层在写事务内读改写。
CREATE TABLE IF NOT EXISTS workspace_seq (
    workspace_id  TEXT PRIMARY KEY NOT NULL REFERENCES workspaces(id),
    next_seq      INTEGER NOT NULL DEFAULT 1
);
