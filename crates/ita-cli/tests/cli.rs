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
    // 直接读根清单的版本号：用测试 crate 自身的版本会让漏推版本时仍通过。
    let workspace_version = std::fs::read_to_string("../../Cargo.toml")
        .expect("应当能读到根清单")
        .lines()
        .find_map(|line| line.strip_prefix("version = \""))
        .and_then(|rest| rest.strip_suffix('"'))
        .map(str::to_owned)
        .expect("根清单应当有 workspace 版本号");
    assert_eq!(text.trim(), format!("ita {workspace_version}"));
}

#[test]
fn 三个角色都能正常退出() {
    for role in ["master", "worker", "tool-host"] {
        let out = ita(&[role]);
        assert!(out.status.success(), "角色 {role} 应当以 0 退出");
    }
}

#[test]
fn 短选项与长选项一致() {
    let short = ita(&["-V"]);
    let long = ita(&["--version"]);
    assert!(short.status.success());
    assert_eq!(short.stdout, long.stdout);
}

#[test]
fn 无参数启动按_master_处理() {
    // 缺省角色是 master；两个子命令都未实现，因此只断言退出码与提示。
    let out = ita(&[]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stderr).expect("输出应当是 UTF-8");
    assert!(text.contains("master"), "缺省角色应当明确是 master");
}

#[test]
fn 尚未实现的角色把提示写到标准错误() {
    let out = ita(&["worker"]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stderr).expect("输出应当是 UTF-8");
    assert!(text.contains("尚未实现"), "占位实现必须在标准错误里说明");
    assert!(out.stdout.is_empty(), "标准输出应当保持干净");
}

#[test]
fn 未知角色以二退出并给出用法() {
    let out = ita(&["serve"]);
    assert_eq!(out.status.code(), Some(2));
    let text = String::from_utf8(out.stderr).expect("输出应当是 UTF-8");
    assert!(text.contains("用法：ita"), "错误信息里应当给出用法");
}
