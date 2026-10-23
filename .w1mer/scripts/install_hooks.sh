#!/usr/bin/env bash
# 安装仓库自带的 git 钩子。
#
# 钩子规范与脚本副本都在版本控制里（.w1mer/tools/），.git/hooks/ 不进仓库，
# 因此新克隆必须执行一次本脚本。安装方式是复制而非软链：Windows 上软链需要
# 额外权限，复制最稳，改钩子后重跑一次即可。
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
hooks_dir="$(git rev-parse --git-path hooks)"
mkdir -p "$hooks_dir"

for hook in commit-msg post-commit; do
  src=".w1mer/tools/$hook"
  [ -f "$src" ] || { echo "缺少 $src"; exit 1; }
  cp "$src" "$hooks_dir/$hook"
  chmod +x "$hooks_dir/$hook"
  echo "已安装 $hooks_dir/$hook"
done

echo "钩子安装完成。规范见 AGENTS.md 第 3 节。"
