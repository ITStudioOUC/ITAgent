# Performance Work Index (performance/)

> Records per-domain bottleneck reviews, optimization plans, and retest
> results. Explorers write analysis directly here.

## Domain docs

| Doc | Domain | State |
|-----|--------|-------|
| PERF_GC.md | garbage collection | pending |
| PERF_PROPERTY.md | property / shape / IC | pending |
| PERF_ARRAY.md | array / index properties | pending |
| PERF_COERCION.md | coercion | pending |
| PERF_STRING.md | strings | pending |
| PERF_CALL.md | calls / frames / closures | pending |
| PERF_IC_DISPATCH.md | IC / dispatch loop | pending |
| PERF_COMPILE.md | compile / startup / cache | pending |

## Optimization records

| ID | Title | Domain | State | Doc |
|----|-------|--------|-------|-----|
<!-- w1mer:rows -->

<!-- /w1mer:rows -->

## Retest discipline

- Benchmarks are the primary measure; run a focused sub-benchmark (one at a
  time), compare against baseline.
- After each optimization: semantic check (unit tests) + benchmark comparison,
  record data, mark state.

## Records (newest first)

(empty)
