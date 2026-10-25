#!/usr/bin/env bash
# 分层门自测：用合成 metadata 覆盖成功与失败路径。
#
# 不修改真实的 Cargo.toml，也不碰 Cargo.lock：基线取自 cargo metadata --no-deps，
# 每个用例在临时目录里生成一份变异 JSON，再交给 check_layers.sh --metadata。
# 门的文案可能变化，断言只匹配关键词与退出码。

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

gate=.w1mer/scripts/check_layers.sh
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

cargo metadata --no-deps --locked --format-version 1 >"$tmp/base.json"

python3 - "$tmp/base.json" "$tmp" <<'PY'
import copy
import json
import sys
from pathlib import Path

base = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
out = Path(sys.argv[2])


def clone():
    return copy.deepcopy(base)


def find(meta, name):
    return next(package for package in meta["packages"] if package["name"] == name)


def write(name, meta):
    (out / f"{name}.json").write_text(json.dumps(meta, ensure_ascii=False), encoding="utf-8")


# 普通依赖跨越禁止边。
meta = clone()
find(meta, "ita-worker")["dependencies"].append({"name": "ita-store", "kind": None})
write("forbidden_edge", meta)

# dev 与 build 的 ita 边仍受检查。
meta = clone()
find(meta, "ita-worker")["dependencies"].append({"name": "ita-store", "kind": "dev"})
write("dev_edge", meta)

meta = clone()
find(meta, "ita-worker")["dependencies"].append({"name": "ita-store", "kind": "build"})
write("build_edge", meta)

# 普通依赖越权使用敏感能力。
meta = clone()
find(meta, "ita-server")["dependencies"].append({"name": "rusqlite", "kind": None})
write("capability_misuse", meta)

# dev 与 build 的能力依赖应当放行。
meta = clone()
find(meta, "ita-worker")["dependencies"].append({"name": "rusqlite", "kind": "dev"})
write("dev_capability_ok", meta)

meta = clone()
find(meta, "ita-worker")["dependencies"].append({"name": "rusqlite", "kind": "build"})
write("build_capability_ok", meta)

# 改名依赖不能绕过包名识别。
meta = clone()
find(meta, "ita-worker")["dependencies"].append(
    {"name": "ita-store", "kind": None, "rename": "db"}
)
write("renamed_edge", meta)

# 组合根不能被库 crate 反向依赖。
meta = clone()
find(meta, "ita-server")["dependencies"].append({"name": "ita-cli", "kind": None})
write("reverse_edge", meta)

# 未登记的二进制 target。
meta = clone()
find(meta, "ita-worker")["targets"].append({"name": "probe", "kind": ["bin"]})
write("extra_bin", meta)

# 未登记的新成员：packages 与 workspace_members 都加，触发允许表覆盖检查。
probe_id = "path+file:///tmp/ita-probe#0.1.0"
meta = clone()
meta["packages"].append(
    {
        "id": probe_id,
        "name": "ita-probe",
        "dependencies": [],
        "targets": [{"name": "ita_probe", "kind": ["lib"]}],
    }
)
meta["workspace_members"].append(probe_id)
write("unknown_member", meta)

# 陈旧成员条目：从 packages 与 workspace_members 同时移除 ita-exec。
meta = clone()
meta["packages"] = [p for p in meta["packages"] if p["name"] != "ita-exec"]
meta["workspace_members"] = [i for i in meta["workspace_members"] if "ita-exec" not in i]
write("stale_member", meta)

# 全量 metadata：多出不在 workspace_members 里的包，入口应当拒绝。
meta = clone()
meta["packages"].append(
    {
        "id": "registry+https://example.com/foreign#1.0.0",
        "name": "foreign",
        "dependencies": [],
        "targets": [{"name": "foreign", "kind": ["lib"]}],
    }
)
write("foreign_package", meta)
PY

cp "$tmp/base.json" "$tmp/baseline.json"

pass=0
fail=0

expect_pass() {
  local name=$1 output
  if ! output=$(bash "$gate" --metadata "$tmp/$name.json" 2>&1); then
    printf 'FAIL %s：期望通过，实际失败\n%s\n' "$name" "$output"
    fail=$((fail + 1))
    return
  fi
  printf 'PASS %s\n' "$name"
  pass=$((pass + 1))
}

expect_fail() {
  local name=$1 needle=$2 output status
  set +e
  output=$(bash "$gate" --metadata "$tmp/$name.json" 2>&1)
  status=$?
  set -e
  if [ "$status" -eq 0 ]; then
    printf 'FAIL %s：期望非零退出，实际 0\n%s\n' "$name" "$output"
    fail=$((fail + 1))
    return
  fi
  if ! grep -qF -- "$needle" <<<"$output"; then
    printf 'FAIL %s：输出未命中「%s」\n%s\n' "$name" "$needle" "$output"
    fail=$((fail + 1))
    return
  fi
  printf 'PASS %s\n' "$name"
  pass=$((pass + 1))
}

expect_pass baseline
expect_pass dev_capability_ok
expect_pass build_capability_ok

expect_fail forbidden_edge "不允许依赖 ita-store"
expect_fail dev_edge "不允许依赖 ita-store"
expect_fail build_edge "不允许依赖 ita-store"
expect_fail capability_misuse "越权"
expect_fail renamed_edge "不允许依赖 ita-store"
expect_fail reverse_edge "不得依赖 ita-cli"
expect_fail extra_bin "未登记的二进制 probe"
expect_fail unknown_member "ita-probe 没有在允许表里登记"
expect_fail stale_member "ita-exec 不是 workspace 成员"
expect_fail foreign_package "只接受 cargo metadata --no-deps 的输出"

# 错误路径：metadata 文件不存在。
set +e
missing_output=$(bash "$gate" --metadata "$tmp/does-not-exist.json" 2>&1)
missing_status=$?
set -e
if [ "$missing_status" -eq 2 ] && grep -qF -- "metadata 文件不存在" <<<"$missing_output"; then
  printf 'PASS missing_metadata_file\n'
  pass=$((pass + 1))
else
  printf 'FAIL missing_metadata_file：期望退出 2 并给出明确诊断\n%s\n' "$missing_output"
  fail=$((fail + 1))
fi

# 反证：删掉 rusqlite 的能力规则后，同一个越权样本必须被放行；否则说明自测没有
# 真正覆盖能力表，只是恒绿。
python3 - "$gate" "$tmp/gate_without_rusqlite.sh" <<'PY'
import sys
from pathlib import Path

text = Path(sys.argv[1]).read_text(encoding="utf-8")
text = text.replace('    "rusqlite": {"ita-store"},\n', "")
Path(sys.argv[2]).write_text(text, encoding="utf-8")
PY
if bash "$tmp/gate_without_rusqlite.sh" --metadata "$tmp/capability_misuse.json" >/dev/null 2>&1; then
  printf 'PASS capability_rule_is_load_bearing\n'
  pass=$((pass + 1))
else
  printf 'FAIL capability_rule_is_load_bearing：删掉规则后仍然拒绝，自测不可信\n'
  fail=$((fail + 1))
fi

printf '\n分层门自测：%d 项通过，%d 项失败。\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
