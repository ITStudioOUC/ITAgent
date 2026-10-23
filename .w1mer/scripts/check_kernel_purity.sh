#!/usr/bin/env bash
# 内核纯净门：ita-kernel 不得依赖任何带 IO 的 crate，也不得读时钟、碰文件系统、
# 起子进程或线程、读环境变量。
#
# 依赖部分由 cargo tree 断言，只允许 dev-dependencies；行为部分由 clippy 的
# disallowed-types 与 disallowed-methods 断言，条目在 crates/ita-kernel/clippy.toml。
# 任一条件不满足即非零退出，可直接作为 CI 的一步。
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

die() { printf '内核纯净门失败：%s\n' "$*" >&2; exit 1; }

# 构建脚本在编译期能做任意 IO，也能把依赖藏进构建图，因此必须不存在。
if [ -e crates/ita-kernel/build.rs ]; then
  die "ita-kernel 不应有 build.rs"
fi

# cargo tree 默认会把 dev 依赖也画出来，这里只要 normal 与 build 两类：
# 测试用具允许，随产物发布的依赖一个都不许有。
deps=$(cargo tree -p ita-kernel --edges normal,build --prefix none)
if [ "$(printf '%s\n' "$deps" | wc -l)" -ne 1 ] || ! printf '%s\n' "$deps" | grep -qx 'ita-kernel v.*'; then
  printf '实际依赖树：\n%s\n' "$deps" >&2
  die "ita-kernel 出现普通或构建依赖，只允许 dev-dependencies"
fi

# clippy 按被检 crate 的清单目录查找 clippy.toml，条目因此只作用于内核。
# 写错的条目只打印普通警告、不受 -D warnings 约束，这里单独抓一次。
if ! lint_output=$(cargo clippy -p ita-kernel --all-targets -- -D warnings 2>&1); then
  printf '%s\n' "$lint_output" >&2
  die "内核出现被禁止的 IO 调用，见上面的 clippy 诊断"
fi
if printf '%s\n' "$lint_output" | grep -q 'does not refer to a reachable'; then
  printf '%s\n' "$lint_output" >&2
  die "clippy.toml 有条目不指向可达的类型或函数，需要修正拼写"
fi

printf '内核纯净门通过\n'
