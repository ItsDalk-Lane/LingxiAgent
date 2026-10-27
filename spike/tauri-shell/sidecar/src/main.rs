//! R01-T06 sidecar 原型：固定二进制、受限参数、行式 JSON 握手协议。
//!
//! 协议（stdout 首行，握手）：
//!   {"lingxi_sidecar_hello":true,"name":"lingxi-t06-sidecar","version":"1.0.0","protocol":1}
//! 之后 stdin 每行一个 JSON 命令，stdout 每行一个 JSON 响应：
//!   {"cmd":"ping","seq":N}      -> {"ok":true,"seq":N,"pong":true}
//!   {"cmd":"version","seq":N}   -> {"ok":true,"seq":N,"version":"1.0.0","protocol":1}
//!   {"cmd":"shutdown","seq":N}  -> {"ok":true,"seq":N,"bye":true} 然后 exit(0)
//!   未知命令                    -> {"ok":false,"seq":N,"error":"unknown_command"}
//!
//! 受限参数：只接受 `--report-version X`（测试钩子，用于版本不匹配负向用例；
//! 宿主正常路径永不传参）。任何其他参数 => exit(64)（EX_USAGE），不执行。
//! 无 shell、无任意命令执行能力。

use std::io::{BufRead, Write};

const REAL_VERSION: &str = "1.0.0";
const PROTOCOL: u32 = 1;

fn main() {
    let mut report_version = REAL_VERSION.to_string();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.len() {
        0 => {}
        2 if args[0] == "--report-version" => {
            report_version = args[1].clone();
        }
        _ => {
            eprintln!("lingxi-t06-sidecar: unsupported arguments (fixed binary, restricted args)");
            std::process::exit(64); // EX_USAGE
        }
    }

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    // 握手：启动后第一行必须是 hello，宿主据此校验身份与版本。
    let hello = format!(
        "{{\"lingxi_sidecar_hello\":true,\"name\":\"lingxi-t06-sidecar\",\"version\":\"{}\",\"protocol\":{}}}",
        report_version, PROTOCOL
    );
    writeln!(stdout, "{}", hello).expect("write hello");
    stdout.flush().expect("flush hello");

    let mut seq_fallback = 0u64;
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break, // stdin 关闭 => 宿主已退出 => 自行退出
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        seq_fallback += 1;
        let response = if let Some(cmd) = extract_str(trimmed, "\"cmd\"") {
            let seq = extract_u64(trimmed, "\"seq\"").unwrap_or(seq_fallback);
            match cmd.as_str() {
                "ping" => format!("{{\"ok\":true,\"seq\":{},\"pong\":true}}", seq),
                "version" => format!(
                    "{{\"ok\":true,\"seq\":{},\"version\":\"{}\",\"protocol\":{}}}",
                    seq, report_version, PROTOCOL
                ),
                "shutdown" => {
                    writeln!(stdout, "{{\"ok\":true,\"seq\":{},\"bye\":true}}", seq).ok();
                    stdout.flush().ok();
                    std::process::exit(0);
                }
                _ => format!(
                    "{{\"ok\":false,\"seq\":{},\"error\":\"unknown_command\"}}",
                    seq
                ),
            }
        } else {
            format!(
                "{{\"ok\":false,\"seq\":{},\"error\":\"malformed\"}}",
                seq_fallback
            )
        };
        if writeln!(stdout, "{}", response).is_err() {
            break; // stdout 关闭 => 退出
        }
        stdout.flush().ok();
    }
    // stdin EOF（宿主退出/管道断裂）=> 正常退出，不变孤儿。
    std::process::exit(0);
}

/// 极简字段提取（sidecar 协议是宿主生成的固定形态，不需要完整 JSON 解析器；
/// 宿主侧用 serde_json 做权威解析）。只容忍固定格式，畸形输入返回错误行。
fn extract_str(line: &str, key: &str) -> Option<String> {
    let idx = line.find(key)?;
    let rest = &line[idx + key.len()..];
    let rest = rest.trim_start_matches(':').trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn extract_u64(line: &str, key: &str) -> Option<u64> {
    let idx = line.find(key)?;
    let rest = &line[idx + key.len()..];
    let rest = rest.trim_start_matches(':').trim_start();
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

