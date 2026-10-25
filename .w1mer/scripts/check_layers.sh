#!/usr/bin/env bash
# 分层与能力归属门：把 AGENTS.md 第 7 节的架构不变量变成可执行检查。
#
# 三件事：
#   1. 依赖方向：每个 workspace 成员只允许依赖脚本里登记过的 ita-* crate。
#   2. 能力归属：SQLite 只能在存储层、HTTP 只能在 server、沙箱原语只能在沙箱层。
#   3. 产物形态：二进制 target 必须逐个登记，当前只允许 ita-cli 产出 ita。
#
# 检查对象是清单里声明的直接依赖，不是解析后的传递依赖。dev 与 build 依赖参与
# ita-* 归属检查，但不参与能力归属检查——它们不进产物，测试与基准需要宽松。
#
# 这道门是分层守卫，不是安全边界：std::net、tokio::net、构建脚本内的网络访问、
# 传递依赖与 pub use 转出口都不在检查范围内；被允许使用 libc 的 crate 仍可直接
# 发系统调用。无出网的真正边界是沙箱策略，不是这里。
#
# 新增成员、放宽某条边、引入新的敏感能力时，先改本文件的常量，再同步
# .w1mer/codebase/STACK.md 的摘要与 STRUCTURE.md 的允许表。脚本是唯一权威来源。

set -euo pipefail

usage() {
  printf '用法：%s [--metadata <文件>]\n' "$0" >&2
  printf '  --metadata 只接受 cargo metadata --no-deps 的输出，供自测与离线诊断使用。\n' >&2
}

metadata_input=""
case "${1:-}" in
  "") ;;
  --metadata)
    if [ "$#" -ne 2 ] || [ -z "${2:-}" ]; then
      usage
      exit 2
    fi
    metadata_input=$2
    ;;
  *)
    usage
    exit 2
    ;;
esac

meta_file=$(mktemp)
err_file=$(mktemp)
trap 'rm -f "$meta_file" "$err_file"' EXIT

if [ -n "$metadata_input" ]; then
  if [ ! -f "$metadata_input" ]; then
    printf '分层门失败：metadata 文件不存在：%s\n' "$metadata_input" >&2
    exit 2
  fi
  cp "$metadata_input" "$meta_file"
else
  cd "$(git rev-parse --show-toplevel)"

  # 与仓库其余 cargo 步骤一致带 --locked。--no-deps 只读清单、不解析锁文件，所以
  # 清单改动不会让这一步因锁文件过期而失败；真失败时把原始诊断原样打出来。
  if ! cargo metadata --no-deps --locked --format-version 1 >"$meta_file" 2>"$err_file"; then
    printf '分层门失败：cargo metadata 执行失败，原因如下：\n' >&2
    sed 's/^/  /' "$err_file" >&2
    exit 1
  fi
fi

python3 - "$meta_file" <<'PY'
import json
import sys
from pathlib import Path

try:
    meta = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
except (OSError, json.JSONDecodeError) as error:
    print(f"分层门失败：metadata 不是合法 JSON（{error}）", file=sys.stderr)
    sys.exit(1)

packages = meta.get("packages")
if not isinstance(packages, list):
    print("分层门失败：metadata 缺少 packages 数组。", file=sys.stderr)
    sys.exit(1)
for package in packages:
    if not isinstance(package, dict) or "id" not in package or "name" not in package:
        print("分层门失败：packages 里的条目缺少 id 或 name。", file=sys.stderr)
        sys.exit(1)

workspace_ids = set(meta.get("workspace_members", []))
package_ids = {package["id"] for package in packages}
if package_ids != workspace_ids:
    extra = sorted(package_ids - workspace_ids)
    missing = sorted(workspace_ids - package_ids)
    details = []
    if extra:
        details.append("多出包 " + "、".join(extra))
    if missing:
        details.append("缺少包 " + "、".join(missing))
    print(
        "分层门失败：--metadata 只接受 cargo metadata --no-deps 的输出；"
        + "；".join(details),
        file=sys.stderr,
    )
    sys.exit(1)

members = {package["name"] for package in packages}

# 每个成员允许依赖的 ita-* crate。键必须与 workspace 成员完全一致：新增成员不登记
# 会失败，成员删除或改名后留下陈旧条目也会失败。
ALLOWED = {
    "ita-kernel": set(),
    "ita-store": {"ita-kernel"},
    # 模型代理帧与流式转发复用归一化消息类型。反向边 model -> proto 禁止：编码
    # 解码只能由 proto 消费 model 的类型。
    "ita-proto": {"ita-kernel", "ita-model"},
    "ita-model": {"ita-kernel"},
    "ita-workflow": {"ita-kernel"},
    "ita-sandbox": {"ita-kernel"},
    # 工具安装期的构建沙箱与可用性门要用 sandbox 的 Sandbox trait 与平台探针；
    # 策略与保真度类型在 kernel，不在这里重复定义。
    "ita-tools": {"ita-kernel", "ita-proto", "ita-sandbox"},
    "ita-exec": {"ita-kernel", "ita-proto", "ita-sandbox", "ita-tools"},
    "ita-worker": {"ita-kernel", "ita-proto", "ita-model"},
    "ita-server": {
        "ita-kernel",
        "ita-store",
        "ita-proto",
        "ita-model",
        "ita-workflow",
        "ita-tools",
    },
    "ita-cli": {
        "ita-kernel",
        "ita-store",
        "ita-proto",
        "ita-sandbox",
        "ita-exec",
        "ita-tools",
        "ita-model",
        "ita-workflow",
        "ita-server",
        "ita-worker",
    },
}

# 敏感能力只允许出现在指定 crate 的普通依赖里。只写直接依赖：传递依赖由上游决定，
# 不在这道门的职责内。
CAPABILITIES = {
    # 数据库：单库单写者，只由 server 通过存储层打开。
    "rusqlite": {"ita-store"},
    "libsqlite3-sys": {"ita-store"},
    "sqlx": {"ita-store"},
    "diesel": {"ita-store"},
    "postgres": {"ita-store"},
    "tokio-postgres": {"ita-store"},
    "sled": {"ita-store"},
    "redb": {"ita-store"},
    "rocksdb": {"ita-store"},
    # 网络与 TLS：只有 server 代理模型流量，worker、执行器与沙箱不得直接出网。
    "reqwest": {"ita-server"},
    "hyper": {"ita-server"},
    "ureq": {"ita-server"},
    "isahc": {"ita-server"},
    "surf": {"ita-server"},
    "curl": {"ita-server"},
    "socket2": {"ita-server"},
    "mio": {"ita-server"},
    "rustls": {"ita-server"},
    "native-tls": {"ita-server"},
    "openssl": {"ita-server"},
    "h2": {"ita-server"},
    # 沙箱原语：只有沙箱层能自己实现强制，执行器通过 trait 委派。
    "landlock": {"ita-sandbox"},
    "seccompiler": {"ita-sandbox"},
    "libseccomp": {"ita-sandbox"},
    # 进程与信号：sandbox 需要系统调用，exec 需要进程组与 subreaper，server 需要
    # 停掉执行器进程组。server 的 deny(unsafe_code) 会挡住裸 libc，因此 libc 只给前两者。
    "libc": {"ita-sandbox", "ita-exec"},
    "nix": {"ita-sandbox", "ita-exec", "ita-server"},
    "rustix": {"ita-sandbox", "ita-exec", "ita-server"},
    # 标识符：时间与随机只在内核之外，签发与落库分别由 server 与存储层负责。
    "ulid": {"ita-store", "ita-server"},
}

# 允许产出的二进制 target。当前只有一个 ita；新增模板类二进制要先在这里登记。
ALLOWED_BINS = {("ita-cli", "ita")}

errors = []

for name in sorted(members - ALLOWED.keys()):
    errors.append(
        f"workspace 成员 {name} 没有在允许表里登记；新增成员时必须同时更新本脚本、"
        "STACK.md 与 STRUCTURE.md。"
    )
for name in sorted(ALLOWED.keys() - members):
    errors.append(
        f"允许表里的 {name} 不是 workspace 成员，条目已陈旧；成员改名或删除后要同步清理。"
    )
for capability, owners in CAPABILITIES.items():
    if not owners:
        errors.append(
            f"能力 {capability} 的归属集合为空；要么删除该规则，要么写明允许的 crate。"
        )
    for owner in sorted(set(owners) - members):
        errors.append(
            f"能力 {capability} 的归属 {owner} 不是 workspace 成员，条目已陈旧。"
        )

edge_count = 0
for package in packages:
    crate = package["name"]
    allowed = ALLOWED.get(crate, set())
    for dependency in package["dependencies"]:
        dep_name = dependency["name"]
        kind = dependency.get("kind")
        kind_text = "普通" if kind is None else kind
        if dep_name in members:
            if dep_name == "ita-cli":
                errors.append(
                    f"{crate} 不得依赖 ita-cli：组合根只能指向库 crate，反向依赖会让"
                    "业务逻辑被拖进二进制。"
                )
            elif dep_name not in allowed:
                allowed_text = "、".join(sorted(allowed)) if allowed else "无"
                errors.append(
                    f"{crate} 不允许依赖 {dep_name}（{kind_text}依赖）；它允许的 ita-*"
                    f"依赖是：{allowed_text}。"
                )
            else:
                edge_count += 1
        if kind is None:
            owners = CAPABILITIES.get(dep_name)
            if owners is not None and crate not in owners:
                owners_text = "、".join(sorted(owners))
                errors.append(
                    f"{crate} 的普通依赖 {dep_name} 越权：该能力只允许出现在 "
                    f"{owners_text}。"
                )

bins = {
    (package["name"], target["name"])
    for package in packages
    for target in package["targets"]
    if "bin" in target["kind"]
}
for crate, name in sorted(bins - ALLOWED_BINS):
    errors.append(
        f"{crate} 产出了未登记的二进制 {name}；单二进制分发是既定形态，新增产物必须"
        "在脚本的 ALLOWED_BINS 里显式登记并说明用途。"
    )
for crate, name in sorted(ALLOWED_BINS - bins):
    errors.append(
        f"登记的二进制 {name}（{crate}）没有出现在 target 列表里，条目已陈旧。"
    )

if errors:
    print("分层门失败：", file=sys.stderr)
    for error in errors:
        print(f"  - {error}", file=sys.stderr)
    sys.exit(1)

print(
    f"分层门通过：{len(members)} 个成员，{edge_count} 条 ita 依赖边，"
    f"{len(CAPABILITIES)} 条能力规则，{len(bins)} 个二进制。"
)
PY
