#!/usr/bin/env bash
# 内核纯净门：ita-kernel 不得依赖任何第三方 crate（含 dev 依赖），不得读时钟、
# 碰文件系统、起子进程或线程、读环境变量。
#
# 依赖部分用 cargo tree 与 cargo metadata 断言，行为部分用 clippy 的
# disallowed-types 与 disallowed-methods 断言，条目在 crates/ita-kernel/clippy.toml。
# 任一条件不满足即非零退出，可直接作为 CI 的一步。
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

die() { printf '内核纯净门失败：%s\n' "$*" >&2; exit 1; }

# 构建脚本在编译期能做任意 IO，也能把依赖藏进构建图，因此必须不存在。
# 只查固定路径不够：清单可以写 `build = "..."` 指向任意文件，所以改查元数据里
# 是否声明了 custom-build target，这条对两种写法都成立。
if ! metadata=$(cargo metadata --no-deps --format-version 1 2>/dev/null); then
  die "cargo metadata 执行失败"
fi
if printf '%s' "$metadata" | grep -q '"custom-build"'; then
  die "ita-kernel 不应有构建脚本（custom-build target）"
fi

# 依赖断言不加 --edges 过滤：普通、构建、dev 三类都不允许。
# 内核是纯逻辑层，测试用具也不该把它拖进异步运行时或第三方生态。
deps=$(cargo tree -p ita-kernel --prefix none)
if [ "$(printf '%s\n' "$deps" | wc -l)" -ne 1 ] || ! printf '%s\n' "$deps" | grep -qx 'ita-kernel v.*'; then
  printf '实际依赖树：\n%s\n' "$deps" >&2
  die "ita-kernel 出现依赖，内核只允许使用标准库"
fi

# clippy 按被检 crate 的清单目录查找 clippy.toml，条目因此只作用于内核。
# 写错的条目只打印普通警告、不受 -D warnings 约束，这里单独抓一次。
if ! lint_output=$(cargo clippy -p ita-kernel --all-targets --all-features -- -D warnings 2>&1); then
  printf '%s\n' "$lint_output" >&2
  die "内核出现被禁止的 IO 调用，见上面的 clippy 诊断"
fi
if printf '%s\n' "$lint_output" | grep -q 'does not refer to a reachable'; then
  printf '%s\n' "$lint_output" >&2
  die "clippy.toml 有条目不指向可达的类型或函数，需要修正拼写"
fi

printf '内核纯净门通过\n'
