//! 角色分发的端到端测试：直接跑编译出的二进制，验证退出码与输出。

use std::process::Command;

fn ita(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ita")).args(args).output().expect("二进制应当可以启动")
}

#[test]
fn 版本号来自清单() {
    let out = ita(&["--version"]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).expect("输出应当是 UTF-8");
    assert_eq!(text.trim(), format!("ita {}", env!("CARGO_PKG_VERSION")));
}

#[test]
fn 三个角色都能正常退出() {
    for role in ["master", "worker", "tool-host"] {
        let out = ita(&[role]);
        assert!(out.status.success(), "角色 {role} 应当以 0 退出");
    }
}

#[test]
fn 未知角色以二退出并给出用法() {
    let out = ita(&["serve"]);
    assert_eq!(out.status.code(), Some(2));
    let text = String::from_utf8(out.stderr).expect("输出应当是 UTF-8");
    assert!(text.contains("用法：ita"), "错误信息里应当给出用法");
}
