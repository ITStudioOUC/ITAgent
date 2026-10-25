# ita-store

存储层 crate（`crates/ita-store`）。唯一接触数据库的文件系统对象，只由 server 进程使用。
对外暴露**操作级**接口（"追加一条事件"这样的方法），不暴露 SQL 连接，使作用域泄漏
（漏写 `WHERE workspace_id`）在类型层面不可能发生。依赖仅 `ita-kernel`、`rusqlite`、
`blake3`、`ulid`（见 `crates/ita-store/Cargo.toml`）。

## Responsibilities

- 数据库句柄与连接纪律：单连接、`BEGIN IMMEDIATE`、`busy_timeout`、WAL 模式校验、外键常开（`crates/ita-store/src/db.rs`）。
- 迁移运行器：编号脚本内嵌、只加不改、可重复执行、单事务、脚本指纹防篡改（`crates/ita-store/src/migrate.rs`，脚本在 `crates/ita-store/migrations/0001_core.sql` 与 `0002_content.sql`）。
- 工作区注册与作用域构造（`crates/ita-store/src/scope.rs`）。
- 事件追加：会话/节点归属校验、seq 计数器分配、预览截断（`crates/ita-store/src/events.rs`、`crates/ita-store/src/seq.rs`、`crates/ita-store/src/preview.rs`）。
- 帧提交：帧正文的长度前缀编码/解码、head 比较并交换、压缩事件追加（`crates/ita-store/src/commit.rs`、`crates/ita-store/src/heads.rs`）。
- 内容寻址存储：blake3 哈希、去重、临时文件加同步加改名的原子写、按哈希读回（`crates/ita-store/src/cas.rs`、`crates/ita-store/src/blobs.rs`）。
- 快照：seq 分配、父边自动链接、kind 推导、条目形状校验与 blobs 断言（`crates/ita-store/src/snapshots.rs`、`crates/ita-store/src/entries.rs`）。
- 清单折叠（squash 与回滚对齐共用的左结合覆盖加墓碑删除）（`crates/ita-store/src/fold.rs`）。
- 保留窗口判定与 squash 合并事务（`crates/ita-store/src/retention.rs`、`crates/ita-store/src/squash.rs`）。
- 重定基线动词：自主模式拒绝、留痕三列校验（`crates/ita-store/src/retention.rs`）。
- 回滚对齐：沿链折叠读有效状态、四种情况处理、外部改动检测、残留清单组装（`crates/ita-store/src/align.rs`、`crates/ita-store/src/restore.rs`、`crates/ita-store/src/residual.rs`）。
- 回收：全局标记-清除、两观宽限期协议、行先行文件随后、孤儿文件清扫（`crates/ita-store/src/gc.rs`、`crates/ita-store/src/marks.rs`）。

## Key types & abstractions

- `Store`（`crates/ita-store/src/db.rs`）：数据库句柄，持有单连接与 CAS 根目录。
  对外只暴露操作级方法；`conn()`/`conn_mut()` 是 `pub(crate)`，业务 crate 拿不到连接。
  测试用 `conn_for_test()`/`cas_root_for_test()`（`#[doc(hidden)]`）。
- `StoreError`（同文件）：全部存储层错误。错误变体是契约的一部分，
  新增拒绝路径时先在这里加变体，不往 `Sqlite` 里塞自定义语义。
- `WorkspaceScope`（`crates/ita-store/src/scope.rs`）：绑定了工作区的查询上下文。
  构造函数 `pub(crate)`，只能由 `Store::scope` 创建，且创建时校验工作区已注册。
  业务查询永远带作用域，作用域内没有工作区 id 的裸 SQL。
- `NewEvent` / `Event` / `AppendedEvent`（`crates/ita-store/src/events.rs`）：
  待追加事件（builder 风格 `with_session`/`with_node`/`with_payload`）、已持久化事件、
  追加确认（seq 加事件 id）。
- `PreviewLimit`（`crates/ita-store/src/preview.rs`）：预览预算，默认头 4 KiB 尾 1 KiB，
  `defaults()` 是唯一定义点；截断按头加尾的总预算判断，切分回退到字符边界。
- `CommittedFrame` / `FrameBodyError` / `encode_frame` / `decode_frame`
  （`crates/ita-store/src/commit.rs`）：帧提交确认与帧正文的长度前缀文本编解码。
  编解码是纯函数，不引入序列化依赖。
- `SnapshotEntry`（`crates/ita-store/src/entries.rs`）：三种 kind 的枚举
  （`File` 带 `content_stored` 与可选 `mtime`、`Dir`、`Tombstone`），
  非法形状在类型层面不可表示。
- `CreatedSnapshot` / `SnapshotInfo`（`crates/ita-store/src/snapshots.rs`）：
  创建确认（id 加 seq）与元信息（含父边与 kind）。
- `RebaseMode` / `RebaseRequest`（`crates/ita-store/src/retention.rs`）：
  重定基线发起模式枚举与留痕三列请求。
- `AlignResult` / `ResidualEntry` / `ResidualReason`（`crates/ita-store/src/align.rs`、`crates/ita-store/src/residual.rs`）：
  对齐结果与残留清单。原因四值枚举：`Oversized`、`OutOfRange`、
  `BackgroundLeftover`、`MissingContent`。
- `GcResult`（`crates/ita-store/src/gc.rs`）：回收结果（删行数、首观行数、孤儿文件数）。
- `Migration` / `MigrateError`（`crates/ita-store/src/migrate.rs`）：迁移条目与失败原因。
- 内核类型（消费自 `ita-kernel`）：`WorkspaceId`、`SessionId`、`NodeId`、`EventId`、
  `CheckpointId`、`BlobRef`、`Frame`、`CommitPlan`。

## Internal data flow

- **打开**：`Store::open` / `open_in_memory` / `open_with_wal` 建连接，开外键，设
  5 秒忙等，把事务行为设为 `Immediate`，跑完迁移（`crates/ita-store/src/db.rs`）。
  `open_with_wal` 额外校验实际日志模式是 WAL，不是则返回 `WalUnavailable`。
- **事件追加**：`Store::append_event` 开立即事务，调 `events::append_in`：
  先 `SELECT EXISTS` 校验会话与节点归属本工作区，再 `seq::allocate` 从
  `workspace_seq` 计数器读改写分配 seq，截断预览后插入 `events` 行，提交。
  校验失败则回滚，seq 不消耗。
- **帧提交**：`Store::commit_frame` 开立即事务，调 `commit::commit_in`：
  校验会话、父节点、覆盖列表同属工作区，插入帧节点（`body_kind = 'frame'`，
  正文是 `encode_frame` 产出的长度前缀文本），`heads::compare_and_swap` 做
  head 比较并交换（同时匹配 `head_node` 与 `head_generation`，影响零行即
  `HeadConflict`），成功后追加 `compression` 事件。顺序刻意：事件追加排在
  head 交换之后，冲突不烧 seq。
- **CAS 写入**：`Store::put` 开立即事务，`cas::put_in` 算 blake3 哈希，
  先查 `blobs` 行存在则跳过写入（去重），否则原子写文件（同目录临时文件、
  `sync_data`、`rename`），再 `INSERT OR IGNORE` 插行。文件先行、行随后。
- **快照创建**：`Store::create_snapshot` 开立即事务，`snapshots::create_in`
  从表内 `MAX(seq) + 1` 分配 seq，取该工作区 seq 最大的快照作父边（没有则
  kind 为 `baseline`，否则 `incremental`），插快照行，逐条 `entries::insert_in`
  插条目（形状校验加 blobs 断言）。
- **squash**：`Store::squash_if_needed` 开立即事务，`retention::needs_squash`
  数增量快照数达到 `max_incrementals` 时 `squash::merge_in` 折叠基线 B 与最老
  两个增量 I1、I2 的清单，先把 I2 的子快照 I3 重挂到 B，再删 I1、I2 的条目
  与行，最后替换基线条目。重挂必须先于删父，`parent_id` 是无动作自引用外键。
- **重定基线**：`Store::rebaseline` 开立即事务，`retention::rebaseline_in`
  拒绝自主模式与留痕三列缺失，折叠整条链得有效状态，删所有增量，更新基线
  条目与留痕三列。基线行只更新不删除。
- **回滚对齐**：`Store::align` 开立即事务，`align::align_in` 读目标快照的
  seq，折叠所有 seq 小于等于目标 seq 的快照清单得有效状态，逐条处理：
  范围外进残留清单，目录不存在则创建，超阈值文件只报告，正文已存但 blobs
  缺行则标注内容缺失，磁盘缺失则从 CAS 写回，磁盘存在但哈希不同则按
  `force` 决定拒绝或覆盖。最后删范围内磁盘上存在但不在有效状态里的路径。
- **回收**：`Store::gc` 开立即事务，`gc::sweep_in` 用 `marks::collect` 收
  标记集（快照条目哈希、节点正文哈希、事件正文哈希、`extra_roots` 租约根
  的并集），对每个无引用 blob 执行两观协议：第一次观察写
  `unreferenced_since = now` 不删，第二次观察且超宽限期才删行。提交后
  `gc::sweep_files` 删被删行的文件并清扫孤儿文件（无 blob 行的文件，
  包括 `.tmp`）。

## Contracts

**Provides**（server 进程调用，worker 与执行器不持有 `Store`）：

- 工作区：`create_workspace`、`scope`、`workspace_count`、`schema_version`。
- 事件：`append_event`（返回 `AppendedEvent`，代表已持久化）、`events_after`。
- 帧：`commit_frame`（返回 `CommittedFrame`，冲突返回 `HeadConflict`）。
- CAS：`put`（返回 `BlobRef`）、`get`。
- 快照：`create_snapshot`、`snapshot`、`snapshot_entries`。
- 保留：`squash_if_needed`、`rebaseline`。
- 对齐：`align`（返回 `AlignResult`，含残留清单）。
- 回收：`gc`（返回 `GcResult`）。
- 迁移：`migrate::run`、`migrate::current_version`。
- 编解码：`encode_frame`、`decode_frame`（纯函数，供非存储路径使用）。

**Consumes**：

- `ita-kernel::context`：`WorkspaceId`、`SessionId`、`NodeId`、`EventId`、
  `CheckpointId`、`BlobRef`、`Frame`、`CommitPlan`。
- `rusqlite`：连接、事务、参数绑定。
- `blake3`：内容哈希。
- `ulid`：id 生成（事件 id、快照 id）。
- 调用方传入的外部参数：`max_incrementals` 保留窗口、`grace_period` 宽限期、
  `extra_roots` 租约根哈希列表、`range` 回滚范围。这些都是调用方从配置或
  租约表读出后传入的参数，存储层不解析工具清单与租约表。

## Invariants

- 单写者：一个 `Store` 对应一个数据库文件，只由 server 打开。worker 与执行器
  不持有 `Store`。`conn()`/`conn_mut()` 是 `pub(crate)`，业务 crate 拿不到连接。
- 作用域隐式：`workspace` 是连接/会话属性，业务查询永远带 `WorkspaceScope`，
  作用域只能由 `Store::scope` 创建且创建时校验工作区已注册。
- 事件即真相：`events` 表 append-only，主键 `(workspace_id, seq)`，seq 从
  `workspace_seq` 计数器分配，分配与插入在同一事务。读取只按 seq 排序，
  不用 `created_at`。
- head 比较并交换同时匹配 `(head_node, head_generation)`，影响零行即冲突。
  代次是单调的，防 ABA。帧提交在 head 交换成功后才追加事件，冲突不烧 seq。
- 迁移只加不改：脚本内嵌、版本号连续、脚本指纹（FNV-1a 64）防篡改、
  单事务执行、可重复执行（`IF NOT EXISTS`）。`DROP TABLE`、`DELETE FROM`、
  `VACUUM`、`PRAGMA` 在迁移脚本里被扫描禁止。
- 大对象外置：文件内容不进库，`blobs` 表只有哈希、尺寸、创建时间、宽限期
  起点。CAS 布局是 `blobs/<2 字符>/<完整哈希>`。写入原子（临时文件、同步、
  改名），文件先行行随后。
- 快照条目形状由类型与 CHECK 双重保证：`File` 必有哈希与尺寸，
  `content_stored = 0` 的 `File` 必带 `mtime`；`Dir` 无哈希、尺寸、内容标记；
  `Tombstone` 五列全空。`content_stored = 1` 的条目其哈希必须在 `blobs` 里，
  由写入路径断言（不是外键）。
- 快照链线性：每个工作区一条链，seq 从表内最大值加一，父边自动链接，
  kind 推导（首个 `baseline`，其余 `incremental`）。空清单合法。
- squash 与回滚对齐共用 `fold::fold` 的左结合覆盖加墓碑删除算法，折叠满足
  结合律，squash 前后最新快照的有效条目相等。
- 回滚对齐是独立动词：范围是参数，超阈值条目只报告不删除不覆盖，写回前
  核对 blobs，外部改动默认拒绝除非显式强制。
- 回收是全局标记-清除加宽限期，不是引用计数。标记集是三张表加 `extra_roots`
  的并集。两观协议：第一次观察只写不删，第二次观察且超宽限期才删。
  删除顺序是行先行文件随后，中断只留无引用文件。孤儿文件清扫是回收的一部分。
- 重定基线不可逆：自主模式一律拒绝，留痕三列（`rebased_by`、`rebase_reason`、
  `rebase_confirmed_by`）成组写入且非空，基线行只更新不删除。
- 时钟：`now_millis()` 在时钟早于 1970 时返回错误而不是零，早于 1970 的
  `created_at = 0` 与"1970 年"不可区分。存储层允许接触时钟，内核不允许。
- WAL 是吞吐假设的前提：`open_with_wal` 校验实际日志模式，不是则返回
  `WalUnavailable`。某些文件系统不支持 WAL，会静默退回 rollback journal。
