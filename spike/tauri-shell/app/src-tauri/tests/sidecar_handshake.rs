//! Sidecar 握手/协议集成测试：直接驱动**真实 sidecar 二进制**（SIDECAR_BIN 环境变量
//! 指向构建产物），覆盖：hello 接受、版本不匹配拒绝、垃圾输入拒绝、ping 往返、
//! shutdown 有序退出、未知命令报错、受限参数拒绝。不经过 Tauri（进程语义层测试）。

use spike_lib::sidecar::validate_hello;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn sidecar_bin() -> String {
    std::env::var("SIDECAR_BIN")
        .expect("SIDECAR_BIN must point at the built lingxi-t06-sidecar binary")
}

fn spawn(args: &[&str]) -> std::process::Child {
    Command::new(sidecar_bin())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn sidecar")
}

fn read_line(child: &mut std::process::Child) -> String {
    let stdout = child.stdout.as_mut().expect("stdout");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).expect("read line");
    line.trim().to_string()
}

#[test]
fn hello_accept_real_binary() {
    let mut child = spawn(&[]);
    let hello = read_line(&mut child);
    let v = validate_hello(&hello).expect("hello must validate");
    assert_eq!(v["version"], "1.0.0");
    assert_eq!(v["protocol"], 1);
    drop(child.stdin.take());
    let _ = child.wait();
}

#[test]
fn hello_reject_version_mismatch() {
    // --report-version 是 sidecar 自带测试钩子；宿主正常路径永不传参。
    let mut child = spawn(&["--report-version", "9.9.9"]);
    let hello = read_line(&mut child);
    let err = validate_hello(&hello).expect_err("must reject mismatched version");
    assert!(err.contains("version mismatch"), "unexpected reason: {err}");
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn hello_reject_garbage_and_missing_fields() {
    assert!(validate_hello("not json at all").is_err());
    assert!(validate_hello("{\"lingxi_sidecar_hello\":false}").is_err());
    assert!(validate_hello("{\"lingxi_sidecar_hello\":true,\"name\":\"other\",\"version\":\"1.0.0\",\"protocol\":1}").is_err());
    assert!(validate_hello("{\"lingxi_sidecar_hello\":true,\"name\":\"lingxi-t06-sidecar\",\"version\":\"1.0.0\",\"protocol\":2}").is_err());
}

#[test]
fn ping_roundtrip_real_binary() {
    let mut child = spawn(&[]);
    let _hello = read_line(&mut child);
    let stdin = child.stdin.as_mut().expect("stdin");
    writeln!(stdin, "{{\"cmd\":\"ping\",\"seq\":7}}").expect("write ping");
    stdin.flush().unwrap();
    let resp = read_line(&mut child);
    let v: serde_json::Value = serde_json::from_str(&resp).expect("json response");
    assert_eq!(v["ok"], true);
    assert_eq!(v["pong"], true);
    assert_eq!(v["seq"], 7);
    drop(child.stdin.take());
    let _ = child.wait();
}

#[test]
fn unknown_command_rejected() {
    let mut child = spawn(&[]);
    let _hello = read_line(&mut child);
    let stdin = child.stdin.as_mut().expect("stdin");
    writeln!(stdin, "{{\"cmd\":\"exec_shell\",\"seq\":9}}").unwrap();
    stdin.flush().unwrap();
    let resp = read_line(&mut child);
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["ok"], false);
    assert_eq!(v["error"], "unknown_command");
    drop(child.stdin.take());
    let _ = child.wait();
}

#[test]
fn shutdown_orderly_exit_zero() {
    let mut child = spawn(&[]);
    let _hello = read_line(&mut child);
    let stdin = child.stdin.as_mut().expect("stdin");
    writeln!(stdin, "{{\"cmd\":\"shutdown\",\"seq\":3}}").unwrap();
    stdin.flush().unwrap();
    let resp = read_line(&mut child);
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["bye"], true);
    let status = child.wait().expect("wait");
    assert!(status.success(), "orderly shutdown must exit 0, got {status:?}");
}

#[test]
fn restricted_args_rejected() {
    let out = Command::new(sidecar_bin())
        .args(["--evil", "x"])
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(64), "unsupported args must exit 64");
}

#[test]
fn stdin_eof_self_exit_no_orphan() {
    // 宿主死亡 = stdin EOF；sidecar 必须自行退出，不变孤儿。
    let mut child = spawn(&[]);
    let _hello = read_line(&mut child);
    drop(child.stdin.take());
    let status = child.wait().expect("wait");
    assert!(status.success());
}
