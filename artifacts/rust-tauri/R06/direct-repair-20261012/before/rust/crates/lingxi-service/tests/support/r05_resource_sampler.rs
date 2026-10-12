//! 仅用于 R05 资源补证的系统采样，不参与生产资源管理。

use std::collections::BTreeSet;
use std::path::Path;

pub fn process_ids(root: i32) -> Result<Vec<i32>, String> {
    let child = std::process::Command::new("ps")
        .args(["-axo", "pid=,ppid="])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("UNKNOWN: ps 进程树启动失败: {e}"))?;
    // 采样命令本身不属于被测负载，必须按实际 PID 排除。
    let instrument = child.id() as i32;
    let output = child
        .wait_with_output()
        .map_err(|e| format!("UNKNOWN: ps 进程树读取失败: {e}"))?;
    if !output.status.success() {
        return Err(format!("UNKNOWN: ps 进程树退出 {:?}", output.status.code()));
    }
    let rows: Result<Vec<(i32, i32)>, String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() != 2 {
                return Err(format!("UNKNOWN: 无效进程树行 {line:?}"));
            }
            Ok((
                fields[0]
                    .parse()
                    .map_err(|_| format!("UNKNOWN: 无效 PID {line:?}"))?,
                fields[1]
                    .parse()
                    .map_err(|_| format!("UNKNOWN: 无效 PPID {line:?}"))?,
            ))
        })
        .collect();
    let rows = rows?;
    if !rows.iter().any(|(pid, _)| *pid == root) {
        return Err(format!("UNKNOWN: 根进程 {root} 不存在"));
    }
    let mut ids = BTreeSet::from([root]);
    loop {
        let before = ids.len();
        for (pid, parent) in &rows {
            if *pid != instrument && ids.contains(parent) {
                ids.insert(*pid);
            }
        }
        if before == ids.len() {
            break;
        }
    }
    Ok(ids.into_iter().collect())
}

pub fn tcp_of(pid: i32) -> Result<Vec<String>, String> {
    let output = std::process::Command::new("lsof")
        .args(["-p", &pid.to_string(), "-F", "fpPTn"])
        .output()
        .map_err(|e| format!("UNKNOWN: lsof TCP 启动失败: {e}"))?;
    if !output.status.success() {
        return Err(format!("UNKNOWN: lsof TCP 退出 {:?}", output.status.code()));
    }
    parse_tcp(&String::from_utf8_lossy(&output.stdout), pid)
}

pub fn parse_tcp(text: &str, pid: i32) -> Result<Vec<String>, String> {
    let mut identity = false;
    let mut numeric_fd = false;
    let mut tcp = false;
    let mut established = false;
    let mut name = String::new();
    let mut sockets = Vec::new();
    let flush = |sockets: &mut Vec<String>, numeric_fd, tcp, established, name: &str| {
        if numeric_fd && tcp && established {
            sockets.push(name.to_string());
        }
    };
    for line in text.lines().chain(std::iter::once("fEND")) {
        if let Some(value) = line.strip_prefix('p') {
            if value != pid.to_string() {
                return Err("UNKNOWN: TCP 进程身份不一致".into());
            }
            identity = true;
        } else if let Some(value) = line.strip_prefix('f') {
            flush(&mut sockets, numeric_fd, tcp, established, &name);
            numeric_fd = !value.is_empty() && value.chars().all(|c| c.is_ascii_digit());
            tcp = false;
            established = false;
            name.clear();
        } else if line == "PTCP" {
            tcp = true;
        } else if line == "TST=ESTABLISHED" {
            established = true;
        } else if let Some(value) = line.strip_prefix('n') {
            name = value.to_string();
        }
    }
    if !identity {
        return Err("UNKNOWN: TCP 输出没有进程身份，不能记零".into());
    }
    if sockets.iter().any(String::is_empty) {
        return Err("UNKNOWN: TCP 记录没有连接地址".into());
    }
    Ok(sockets)
}

pub fn files(root: &Path) -> Result<Vec<serde_json::Value>, String> {
    fn walk(root: &Path, dir: &Path, entries: &mut Vec<serde_json::Value>) -> Result<(), String> {
        let children = std::fs::read_dir(dir)
            .map_err(|e| format!("UNKNOWN: 文件清单目录 {}: {e}", dir.display()))?;
        for entry in children {
            let entry = entry.map_err(|e| format!("UNKNOWN: 文件清单条目: {e}"))?;
            let path = entry.path();
            let kind = entry
                .file_type()
                .map_err(|e| format!("UNKNOWN: 文件类型: {e}"))?;
            if kind.is_symlink() {
                return Err(format!("UNKNOWN: 清单出现未经授权链接 {}", path.display()));
            }
            if kind.is_dir() {
                walk(root, &path, entries)?;
            } else if kind.is_file() {
                entries.push(serde_json::json!({
                    "path": path.strip_prefix(root).map_err(|e| e.to_string())?.to_string_lossy(),
                    "bytes": entry.metadata().map_err(|e| format!("UNKNOWN: 文件长度: {e}"))?.len(),
                }));
            } else {
                return Err(format!("UNKNOWN: 清单出现特殊文件 {}", path.display()));
            }
        }
        Ok(())
    }
    let mut entries = Vec::new();
    walk(root, root, &mut entries)?;
    entries.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    Ok(entries)
}
