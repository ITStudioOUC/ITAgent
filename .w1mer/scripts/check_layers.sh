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
# 传递依赖与 pub use 转出口都不在检查范围内。无出网的真正边界是 ADR 0010 的沙箱。
#
# 新增成员、放宽某条边、引入新的敏感能力时，先改本文件的常量，再同步
# .w1mer/codebase/STACK.md 与 STRUCTURE.md 的摘要。脚本是唯一权威来源。

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

die() { printf '分层门失败：%s\n' "$*" >&2; exit 1; }

# --locked 与仓库其余 cargo 步骤保持一致。注入依赖后锁文件会先过期，测试失败路径
# 前要先刷新 Cargo.lock，见 .w1mer/detail/01_plan-02.md 的 02.8 小节。
meta_file=$(mktemp)
trap 'rm -f "$meta_file"' EXIT

if ! cargo metadata --no-deps --locked --format-version 1 >"$meta_file" 2>/dev/null; then
  die "cargo metadata 执行失败（清单与 Cargo.lock 不一致时也会走到这里）"
fi

python3 - "$meta_file" <<'PY'
import json
import sys
from pathlib import Path

meta = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
packages = meta["packages"]
members = {package["name"] for package in packages}

# 每个成员允许依赖的 ita-* crate。键必须与 workspace 成员完全一致：新增成员不登记
# 会失败，成员删除或改名后留下陈旧条目也会失败。
ALLOWED = {
    "ita-kernel": set(),
    "ita-store": {"ita-kernel"},
    # 12.7 的模型代理帧与 12.8 的流式转发复用归一化消息类型。反向边 model -> proto
    # 禁止：编码解码只能由 proto 消费 model 的类型。
    "ita-proto": {"ita-kernel", "ita-model"},
    "ita-model": {"ita-kernel"},
    "ita-workflow": {"ita-kernel"},
    "ita-sandbox": {"ita-kernel"},
    # 11.9 的构建沙箱与 11.14 的 Fidelity 门要用 Sandbox trait 与 Fidelity 类型。
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
        kind = dependency.get("kind") or "普通"
        if dep_name in members:
            if dep_name == "ita-cli":
                errors.append(
                    f"{crate} 不得依赖 ita-cli：组合根只能指向库 crate，反向依赖会让"
                    "业务逻辑被拖进二进制。"
                )
            elif dep_name not in allowed:
                allowed_text = "、".join(sorted(allowed)) if allowed else "无"
                errors.append(
                    f"{crate} 不允许依赖 {dep_name}（{kind}依赖）；它允许的 ita-* 依赖"
                    f"是：{allowed_text}。"
                )
            else:
                edge_count += 1
        if kind == "普通":
            owners = CAPABILITIES.get(dep_name)
            if owners and crate not in owners:
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
