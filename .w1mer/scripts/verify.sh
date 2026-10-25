#!/usr/bin/env bash
# 本地与 CI 共用的验证入口。
#
# 阶段：
#   fmt   格式检查。
#   lint  分层门、门自测、内核纯净门、shellcheck、clippy 与 rustdoc。
#   test  构建、nextest 与 doctest。
#   msrv  用 1.85.0 工具链跑全目标检查；不在 all 内，因为本地不一定装了它。
#   all   fmt + lint + test，不含 msrv。
#
# 用法：verify.sh [fmt|lint|test|msrv|all]，缺省 all。
# 缺少 shellcheck 时直接失败；用 SHELLCHECK 环境变量可以指向自定义二进制。

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

stage=${1:-all}

section() { printf '\n== %s ==\n' "$1"; }

run_fmt() {
  section "格式"
  cargo fmt --all --check
}

run_lint() {
  section "分层与能力归属门"
  bash .w1mer/scripts/check_layers.sh

  section "分层门自测"
  bash .w1mer/scripts/check_layers_test.sh

  section "内核纯净门"
  bash .w1mer/scripts/check_kernel_purity.sh

  section "shell 脚本静态检查"
  local shellcheck_bin=${SHELLCHECK:-shellcheck}
  if ! command -v "$shellcheck_bin" >/dev/null 2>&1; then
    printf '缺少 shellcheck：安装后重试，或用 SHELLCHECK 指向二进制。\n' >&2
    printf 'Debian/Ubuntu: sudo apt-get install shellcheck\n' >&2
    exit 1
  fi
  "$shellcheck_bin" --version
  "$shellcheck_bin" .w1mer/scripts/*.sh

  section "clippy"
  cargo clippy --workspace --all-targets --locked -- -D warnings

  section "rustdoc"
  RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
}

run_test() {
  section "构建"
  cargo build --workspace --locked

  section "nextest"
  cargo nextest run --workspace --locked

  section "doctest"
  cargo test --doc --workspace --locked
}

run_msrv() {
  section "MSRV 1.85"
  local version
  if ! version=$(rustc +1.85.0 --version 2>/dev/null); then
    printf '缺少 1.85.0 工具链：rustup toolchain install 1.85.0 --profile minimal\n' >&2
    exit 1
  fi
  case "$version" in
    "rustc 1.85."*) printf '%s\n' "$version" ;;
    *)
      printf '期望 rustc 1.85.x，实际为 %s。\n' "$version" >&2
      exit 1
      ;;
  esac
  cargo +1.85.0 check --workspace --all-targets --locked
}

case "$stage" in
  fmt) run_fmt ;;
  lint) run_lint ;;
  test) run_test ;;
  msrv) run_msrv ;;
  all)
    run_fmt
    run_lint
    run_test
    ;;
  *)
    printf '未知阶段 %s；可用：fmt、lint、test、msrv、all。\n' "$stage" >&2
    exit 2
    ;;
esac
