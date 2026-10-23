#!/usr/bin/env bash
# 版本号推进：0.<N>.<P>，记于根 Cargo.toml 的 [workspace.package] version。
#
#   epic   新史诗开始：N 加一，P 归零（0.3.7 -> 0.4.0）
#   batch  同史诗内一个批次收尾：P 加一（0.3.7 -> 0.3.8）
#
# 由 Implementer / Fixer 在 batch-end 之前的最后一次提交里执行。一个批次只推进一次。
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

mode="${1:-}"
case "$mode" in
  epic|batch) ;;
  *) echo "用法：$0 <epic|batch>" >&2; exit 1 ;;
esac

[ -f Cargo.toml ] || { echo "缺少根 Cargo.toml" >&2; exit 1; }

python3 - "$mode" <<'PY'
import re, sys, pathlib

mode = sys.argv[1]
path = "Cargo.toml"
text = open(path, encoding="utf-8").read()

# 定位 [workspace.package] 段，避免误改 [package] 或依赖表里的 version。
m = re.search(r"(?ms)^\[workspace\.package\]\s*$(.*?)(?=^\[|\Z)", text)
if not m:
    sys.exit("Cargo.toml 里没有 [workspace.package] 段")
section = m.group(1)

vm = re.search(r'(?m)^version\s*=\s*"(\d+)\.(\d+)\.(\d+)"', section)
if not vm:
    sys.exit("[workspace.package] 里没有形如 0.N.P 的 version")

major, n, p = (int(x) for x in vm.groups())
if major != 0:
    sys.exit(f"版本主号必须是 0，当前 {major}.{n}.{p}")
old = f'{major}.{n}.{p}'

if mode == "epic":
    n, p = n + 1, 0
else:
    p = p + 1

new = f'{major}.{n}.{p}'
section_new = section[: vm.start()] + f'version = "{new}"' + section[vm.end():]
text = text[: m.start(1)] + section_new + text[m.end(1):]
open(path, "w", encoding="utf-8").write(text)
print(f"版本 {old} -> {new}")

# Cargo.lock 记录每个成员包的版本号，改完清单必须同步刷新，否则 CI 的 --locked
# 会直接失败。这里按块更新所有本地成员（无 source 行），不访问网络。
lock = pathlib.Path("Cargo.lock")
if lock.exists():
    body = lock.read_text(encoding="utf-8")
    blocks = body.split("[[package]]")
    out, changed = [blocks[0]], 0
    for blk in blocks[1:]:
        if "source = " not in blk and f'version = "{old}"' in blk:
            blk = blk.replace(f'version = "{old}"', f'version = "{new}"', 1)
            changed += 1
        out.append(blk)
    if changed:
        lock.write_text("[[package]]".join(out), encoding="utf-8")
        print(f"Cargo.lock 同步 {changed} 个本地成员为 {new}")
    else:
        print("警告：Cargo.lock 里没有找到版本为旧值的本地成员，请手动核对")
PY
