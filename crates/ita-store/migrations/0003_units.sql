-- unit 状态表：激活门与能力集快照的持久化依据。
--
-- 三列承载三件事实：
--   * unit 标识：unit 是 server 内固定命名的子系统，不是动态建行实体，所以 id 是
--     校验过的标签（小写字母数字加连字符），不是 ULID。
--   * 状态：激活门判定后的三值之一，不是枚举。取值域写成 CHECK 约束，在表结构里定形，
--     因为迁移只加不改，错过建表这一次之后只能重建表。
--   * 状态最后更新的时间：epoch 毫秒，记录状态最后一次被写入的时刻。
--
-- 只加不改：CREATE TABLE IF NOT EXISTS，可重复执行。
--
-- 状态取值域是 ready / degraded / failed，不是六状态状态机。六状态（未启动、启动中、
-- 就绪、失败、停止中、已停止）是内存里的状态机，其中只有三种稳定态会跨重启存活：
-- 就绪的 unit 记为 ready，非必需 unit 失败或仍在启动中记为 degraded，必需 unit 失败记为
-- failed。未启动、停止中、已停止是瞬态，不落库。这个三值域是 07.4 能力集快照的输入：
-- 能力集由 unit 状态推导，哪些就绪、哪些降级。
--
-- 不带工作区列：unit 是 server 全局子系统（store、router、ipc、lease、tools、model、
-- telemetry），不是每工作区一个，跨工作区共享同一份状态。blobs 表按内容哈希全局去重
-- 因而无工作区列，units 表同理。
CREATE TABLE IF NOT EXISTS units (
    id          TEXT PRIMARY KEY NOT NULL,
    state       TEXT NOT NULL CHECK (state IN ('ready', 'degraded', 'failed')),
    updated_at  INTEGER NOT NULL
);
