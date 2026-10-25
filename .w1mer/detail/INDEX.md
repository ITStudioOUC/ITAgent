# Detail Index (detail/)

> Dynamic layer: recent changes, field-level details, concerns — anything too
> volatile for the stable codebase layer. Each detail gets its own file; this
> INDEX is a build artifact (`w1mer build`).

## Details

| ID | Title | State | Doc |
|----|-------|-------|-----|
<!-- w1mer:rows -->
| 01 | EPIC 02 仓库骨架的拆分依据 | active | [01_plan-02.md](01_plan-02.md) |
| 02 | EPIC 03 命令文法的拆分依据 | active | [02_plan-03.md](02_plan-03.md) |
| 03 | EPIC 04 上下文模型的拆分依据 | active | [03_plan-04.md](03_plan-04.md) |
| 04 | EPIC 05 语义压缩的拆分依据 | active | [04_plan-05.md](04_plan-05.md) |
| 05 | EPIC 06 存储层的拆分依据 | active | [05_plan-06.md](05_plan-06.md) |
| 06 | EPIC 07 unit 监督者的拆分依据 | active | [06_plan-07.md](06_plan-07.md) |
| 07 | EPIC 08 server 写路径与 IPC 的拆分依据 | active | [07_plan-08.md](07_plan-08.md) |
| 08 | EPIC 09 执行器的拆分依据 | active | [08_plan-09.md](08_plan-09.md) |
| 09 | EPIC 10 Linux 沙箱的拆分依据 | active | [09_plan-10.md](09_plan-10.md) |
| 10 | EPIC 11 工具系统的拆分依据 | active | [10_plan-11.md](10_plan-11.md) |
| 11 | EPIC 12 模型层的拆分依据 | active | [11_plan-12.md](11_plan-12.md) |
| 12 | EPIC 13 工具检索的拆分依据 | active | [12_plan-13.md](12_plan-13.md) |
| 13 | EPIC 14 工作流 DSL 的拆分依据 | active | [13_plan-14.md](13_plan-14.md) |
| 14 | EPIC 15 subagent 的拆分依据 | active | [14_plan-15.md](14_plan-15.md) |
| 15 | EPIC 16 节点调度的拆分依据 | active | [15_plan-16.md](15_plan-16.md) |
| 16 | EPIC 17 埋点与基准的拆分依据 | active | [16_plan-17.md](16_plan-17.md) |
| 17 | EPIC 18 工作区导入导出的拆分依据 | active | [17_plan-18.md](17_plan-18.md) |
| 18 | EPIC 02.2 探针结论 | active | [18_plan-02-2.md](18_plan-02-2.md) |
| 19 | EPIC 02.3 CI 方案核对 | active | [19_plan-02-3.md](19_plan-02-3.md) |
| 20 | 测试函数名英文化与初始表结构约束定形的执行计划 | active | [20_测试函数名英文化与初始表结构约束定形的执行计划.md](20_测试函数名英文化与初始表结构约束定形的执行计划.md) |
| 21 | 批次 06.3 事件追加、seq 分配与预览截断的执行计划 | active | [21_批次_06_3_事件追加_seq_分配与预览截断的执行计划.md](21_批次_06_3_事件追加_seq_分配与预览截断的执行计划.md) |
| 22 | 批次 06.4 帧写入与 head 比较并交换的执行计划 | active | [22_批次_06_4_帧写入与_head_比较并交换的执行计划.md](22_批次_06_4_帧写入与_head_比较并交换的执行计划.md) |
<!-- /w1mer:rows -->

## Lifecycle

- Created via `w1mer new detail --title "..."`.
- A periodic **compact** promotes settled details into the stable `codebase/`
  docs, then the detail is archived.
- `CHANGES.md` holds recent architecture-impact deltas written by reviewers.
