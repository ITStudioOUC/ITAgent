# Detail Index (detail/)

> Dynamic layer: recent changes, field-level details, concerns — anything too
> volatile for the stable codebase layer. Each detail gets its own file; this
> INDEX is a build artifact (`w1mer build`).

## Details

| ID | Title | State | Doc |
|----|-------|-------|-----|
<!-- w1mer:rows -->

<!-- /w1mer:rows -->

## Lifecycle

- Created via `w1mer new detail --title "..."`.
- A periodic **compact** promotes settled details into the stable `codebase/`
  docs, then the detail is archived.
- `CHANGES.md` holds recent architecture-impact deltas written by reviewers.
