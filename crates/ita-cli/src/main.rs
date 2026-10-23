//! `ita` 单二进制的入口。
//!
//! 角色分发在这里完成，具体逻辑由各 crate 提供。二进制本身不承载业务逻辑，因此不会
//! 因为某个子系统增长而变复杂。

mod role;

use std::process::ExitCode;

use role::Role;

const USAGE: &str = "用法：ita [master|worker|tool-host]";

fn main() -> ExitCode {
    let arg = std::env::args().nth(1);
    if arg.as_deref() == Some("--version") || arg.as_deref() == Some("-V") {
        println!("ita {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }

    let Some(role) = Role::parse(arg.as_deref()) else {
        eprintln!("未知角色。{USAGE}");
        return ExitCode::from(2);
    };

    match role {
        // 三个角色当前都是空实现：等待各自的子系统落地后接入。
        Role::Master | Role::Worker | Role::ToolHost => {
            eprintln!("角色 {} 尚未实现", role.as_str());
        }
    }
    ExitCode::SUCCESS
}
