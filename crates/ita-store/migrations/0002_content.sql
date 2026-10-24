-- 内容寻址存储的元数据与快照清单。
--
-- 两条纪律落在这里：
--   * 文件内容不进库：blobs 只有哈希、尺寸、创建时间与宽限期起点，正文在文件系统里；引用
--     关系同样不进本表，回收走全局标记-清除加宽限期，不用引用计数。
--   * 快照是对齐的依据，不是备份：条目记录"当时这个路径是什么"，墓碑记录"当时这里没有
--     东西"，两者缺一不可。
--   * 条目的字段要求按 kind 分档写成约束：文件必有哈希与尺寸，尺寸非负；正文未进内容存储
--     的超阈值文件还必须带修改时间，因为回滚对它们只报告不写回，残留清单靠尺寸与修改时间
--     才能说清"这个文件在快照之后动过没有"。
--
-- 约束写成建表语句里的 CHECK，而不是留在注释或调用点：SQLite 的 ALTER TABLE 只能加列、
-- 不能追加约束，而迁移又只允许加表加列，所以"某种取值下才成立"的规则必须在建表这一次
-- 定形，错过之后只能重建表，那已经不在允许范围内。

CREATE TABLE IF NOT EXISTS blobs (
    hash        TEXT PRIMARY KEY NOT NULL,
    size        INTEGER NOT NULL,
    created_at  INTEGER NOT NULL,
    -- 宽限期起点：由回收扫描观察到的"变为无引用"的时刻，为空表示还没被观察过。创建时间
    -- 不能顶替它——宽限期服务的是"刚被引用、清单尚未提交"这个窗口，拿创建时间当起点会把
    -- 刚写入的对象提前判死。
    unreferenced_since INTEGER,
    CONSTRAINT unreferenced_since_not_before_creation CHECK (
        unreferenced_since IS NULL OR unreferenced_since >= created_at
    )
);

-- 快照。kind 区分基线（baseline）与增量（incremental）：到达保留上限时把最老的两个增量
-- 合并进基线，而不是丢弃最老的——这样"回到任务起点"永远可用，只是颗粒度变粗。
CREATE TABLE IF NOT EXISTS snapshots (
    id            TEXT PRIMARY KEY NOT NULL,
    workspace_id  TEXT NOT NULL REFERENCES workspaces(id),
    seq           INTEGER NOT NULL,
    -- 自引用父边刻意不加级联也不置空：合并必须在同一事务里先把子快照重挂到基线、再删父
    -- 快照。留成无动作外键，忘记重挂会让整笔事务失败；若改成删除时置空，谱系会静默断掉，
    -- 而空父边与"顶层快照"不可区分。
    parent_id     TEXT REFERENCES snapshots(id),
    kind          TEXT NOT NULL CHECK (kind IN ('baseline', 'incremental')),
    -- 重定基线不可逆，留痕三列同进同出且只对基线成立：谁发起、为什么、由谁人工确认。
    -- 为 NULL 才算没有人工确认，写入方不得用空串代替——空串会绕过只判 NULL 的 CHECK，
    -- 被读成"有人确认过"。因此三列非空时还要求非空串。
    rebased_by           TEXT,
    rebase_reason        TEXT,
    rebase_confirmed_by  TEXT,
    created_at    INTEGER NOT NULL,
    CONSTRAINT rebase_trail_is_complete CHECK (
        (rebased_by IS NULL) = (rebase_reason IS NULL)
        AND (rebased_by IS NULL) = (rebase_confirmed_by IS NULL)
        AND (rebased_by IS NULL OR rebased_by <> '')
        AND (rebase_reason IS NULL OR rebase_reason <> '')
        AND (rebase_confirmed_by IS NULL OR rebase_confirmed_by <> '')
    ),
    CONSTRAINT rebase_trail_only_on_baseline CHECK (rebased_by IS NULL OR kind = 'baseline')
);

CREATE UNIQUE INDEX IF NOT EXISTS snapshots_by_seq ON snapshots(workspace_id, seq);
-- 合并要按父查子，窗口查询也沿着这条边，没有索引就只能全表扫。
CREATE INDEX IF NOT EXISTS snapshots_by_parent ON snapshots(parent_id);

-- 快照条目。kind 三值：
--   file       当时存在，hash 与 size 必填；content_stored 表示正文是否已进内容存储
--   dir        当时存在且是目录，没有哈希、尺寸与内容标记
--   tombstone  当时被删除，五列全空——那里什么都没有
-- 超过大小阈值的文件仍是 file，但 content_stored = 0，只保留 hash、size 与 mtime：它们的
-- 内容不在快照覆盖范围内，回滚时只报告、不删除也不覆盖。
--
-- 所以 content_stored = 0 的 file 条目必须有 mtime，这条约束由下面的形态 CHECK 强制。理由
-- 是这类条目在回滚时不会被写回，残留清单是用户判断"这个文件算不算问题"的唯一依据，而清单里
-- 只有尺寸与修改时间两项事实，缺了修改时间就分不清它是快照前就有的还是之后被别的东西改过。
-- 正文已进内容存储的条目（content_stored = 1）不要求 mtime：它的正文本身就是快照的一部分，
-- 回滚不看修改时间，把它一并要求等于把约束扩大到它管不到的地方。
--
-- content_stored 不给默认值：默认值会让"写墓碑时忘了带标记"落成一行自洽的坏数据。现在
-- file 必须显式给出 0 或 1，dir 与墓碑则必须留空——留空表示这条标记不适用，而 0 表示
-- "当时存在但内容未存"，那是超阈值文件的语义，两者不能混用。
--
-- mode 与 mtime 对 dir 不判空：目录确实有权限位与修改时间，回滚要恢复目录权限时它们有用，
-- 而 CHECK 一旦定形就改不了（ALTER TABLE 不能给既有表换约束），现在判空等于永久丢掉这两
-- 个事实。size 则不同——它在本表里表示文件正文长度，目录没有这个量，写进来就是坏数据。
-- 墓碑连 mode 与 mtime 一起判空，因为它表示"当时这里没有东西"，任何字段有值都自相矛盾。
CREATE TABLE IF NOT EXISTS snapshot_entries (
    snapshot_id     TEXT NOT NULL REFERENCES snapshots(id),
    path            TEXT NOT NULL,
    kind            TEXT NOT NULL CHECK (kind IN ('file', 'dir', 'tombstone')),
    hash            TEXT,
    mode            INTEGER,
    size            INTEGER,
    mtime           INTEGER,
    content_stored  INTEGER,
    PRIMARY KEY (snapshot_id, path),
    -- 每个分支都显式写 IS NULL / IS NOT NULL：CHECK 表达式求值为 NULL 时算通过，只用
    -- `content_stored IN (0, 1)` 会让漏写的标记（NULL）静默过关。负尺寸同样要拦：它是
    -- 残留清单要标注的尺寸，负值会让清单与对比失去意义。
    --
    -- file 支的最后一项把"未存内容就必带修改时间"写成 `content_stored = 1 OR mtime IS
    -- NOT NULL`，而不是一律要求 mtime：正文已进内容存储的条目回滚时按正文写回，修改时间
    -- 不参与判定，一律要求会把约束扩到它管不到的地方；只有超阈值那一档要靠修改时间判断
    -- 磁盘上的文件在快照之后有没有被动过。这一项不会让整支退化成 NULL——它前面的
    -- `content_stored IS NOT NULL` 在标记为空时已把整支判为假，而假与 NULL 相与仍为假。
    CONSTRAINT entry_shape_matches_kind CHECK (
        (kind = 'file' AND hash IS NOT NULL AND size IS NOT NULL AND size >= 0
            AND content_stored IS NOT NULL AND content_stored IN (0, 1)
            AND (content_stored = 1 OR mtime IS NOT NULL))
        OR (kind = 'dir' AND hash IS NULL AND size IS NULL AND content_stored IS NULL)
        OR (kind = 'tombstone' AND hash IS NULL AND size IS NULL AND mode IS NULL
            AND mtime IS NULL AND content_stored IS NULL)
    )
);

CREATE INDEX IF NOT EXISTS entries_by_path ON snapshot_entries(path);

-- 读集：谁在什么时候读到了什么版本。
-- 键是（工作区, 路径, 会话）：同一路径上不同会话各有自己的最后读到的哈希，只记"最后一次
-- 读取"会让陈旧写判定与读者无关，那是错的。没有记录表示未知，不写一行空哈希来表达。
CREATE TABLE IF NOT EXISTS read_set (
    workspace_id  TEXT NOT NULL REFERENCES workspaces(id),
    path          TEXT NOT NULL,
    session_id    TEXT NOT NULL REFERENCES sessions(id),
    last_hash     TEXT NOT NULL,
    last_read_seq INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, path, session_id)
);

-- 会话结束时按会话回收读集，没有索引就要全表扫。
CREATE INDEX IF NOT EXISTS read_set_by_session ON read_set(session_id);
