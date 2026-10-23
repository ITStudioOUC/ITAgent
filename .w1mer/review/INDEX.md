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
