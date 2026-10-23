# Review Index (review/)

> Purpose: the Reviewer re-reviews tasks completed last batch, closing the
> correctness gap on top of implementer self-tests. Review docs live here;
> nothing is fixed directly by the reviewer.

## Status states

`pending` `ok` `issues`

## Review docs

| # | Task | State | Doc |
|---|------|-------|-----|
<!-- w1mer:rows -->
| R_01_1 | 01.1 | ok | [R_01_1.md](R_01_1.md) |
| R_02_1 | 02.1 | ok | [R_02_1.md](R_02_1.md) |
| R_02_2_1 | 02.2.1 | ok | [R_02_2_1.md](R_02_2_1.md) |
| R_02_3 |  | ok | [R_02_3.md](R_02_3.md) |
| R_02_4 | 02.4 | ok | [R_02_4.md](R_02_4.md) |
| R_02_5 | 02.5 | ok | [R_02_5.md](R_02_5.md) |
| R_02_6 | 02.6 | ok | [R_02_6.md](R_02_6.md) |
| R_02_7 | 02.7 | ok | [R_02_7.md](R_02_7.md) |
| R_03_1 | 03.1 | ok | [R_03_1.md](R_03_1.md) |
| R_03_2 | 03.2 | ok | [R_03_2.md](R_03_2.md) |
| R_03_3 | 03.3 | ok | [R_03_3.md](R_03_3.md) |
| R_03_4 |  | ok | [R_03_4.md](R_03_4.md) |
| R_03_5 | 03.5 | ok | [R_03_5.md](R_03_5.md) |
| R_04_1 | 04.1 | ok | [R_04_1.md](R_04_1.md) |
| R_04_2 | 04.2 | ok | [R_04_2.md](R_04_2.md) |
| R_04_3 | 04.3 | ok | [R_04_3.md](R_04_3.md) |
| R_04_4 | 04.4 | ok | [R_04_4.md](R_04_4.md) |
| R_04_6 | 04.6 | ok | [R_04_6.md](R_04_6.md) |
| R_05_1 | 05.1 | ok | [R_05_1.md](R_05_1.md) |
| R_05_2 | 05.2 | ok | [R_05_2.md](R_05_2.md) |
<!-- /w1mer:rows -->

## Reviewer checklist

- Base review on **git-committed** code, never working-tree half-done work.
- Conclusions must not rely on the changed code self-verifying: derive
  correctness from the unchanged side (surrounding code, call contracts,
  test expectations, spec semantics).
- For each change ask: *"if this were wrong, who would notice — can existing
  tests catch it?"*
- Record a short architecture-impact note for the codebase changelog.
- Pre-existing problems (not caused by this task) go to the orchestrator only.
