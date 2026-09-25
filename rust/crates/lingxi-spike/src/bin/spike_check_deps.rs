//! R01-T03 install check (acceptance R01-A06): verifies the runtime
//! dependencies this prototype actually needs and reports each one explicitly.
//!
//! Checks:
//!   1. SQLite engine reachable (bundled build: statically compiled in — the
//!      check proves the binary does NOT need a system libsqlite3).
//!   2. TLS client constructible with platform root certificates (rustls +
//!      rustls-platform-verifier via reqwest default-tls).
//!   3. Every dynamic library the binary links against exists on disk
//!      (mach-o `otool -L` on macOS, `ldd` on Linux). A missing entry is a
//!      hard failure (exit 3), never silently ignored.
//!
//! Exit codes: 0 = all present, 3 = missing runtime dependency (diagnosis on
//! stderr), 1 = other failure.

use serde_json::json;

fn check_sqlite() -> Result<String, String> {
    let conn =
        rusqlite::Connection::open_in_memory().map_err(|e| format!("open in-memory: {e}"))?;
    let version: String = conn
        .query_row("SELECT sqlite_version()", [], |r| r.get(0))
        .map_err(|e| format!("sqlite_version(): {e}"))?;
    Ok(version)
}

fn check_tls_backend() -> Result<&'static str, String> {
    // Building the client exercises the rustls platform-verifier initialisation
    // path (native root store access). Verification is never disabled.
    reqwest::Client::builder()
        .build()
        .map_err(|e| format!("build reqwest client: {e}"))?;
    Ok("rustls + rustls-platform-verifier (platform native roots)")
}

#[cfg(target_os = "macos")]
fn linked_libraries(exe: &std::path::Path) -> Result<Vec<String>, String> {
    let out = std::process::Command::new("otool")
        .arg("-L")
        .arg(exe)
        .output()
        .map_err(|e| format!("run otool: {e}"))?;
    if !out.status.success() {
        return Err(format!("otool -L exited {}", out.status));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(text
        .lines()
        .skip(1) // first line is the binary itself
        .filter_map(|l| l.split_whitespace().next())
        .map(|s| s.to_string())
        .collect())
}

#[cfg(target_os = "linux")]
fn linked_libraries(exe: &std::path::Path) -> Result<Vec<String>, String> {
    let out = std::process::Command::new("ldd")
        .arg(exe)
        .output()
        .map_err(|e| format!("run ldd: {e}"))?;
    if !out.status.success() {
        return Err(format!("ldd exited {}", out.status));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(text
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            // "libfoo.so.1 => /lib/x86_64-linux-gnu/libfoo.so.1 (0x...)" or absolute path
            if let Some((_, path)) = l.split_once("=>") {
                let p = path.trim().split_whitespace().next().unwrap_or("");
                if p.starts_with('/') {
                    return Some(p.to_string());
                }
                None
            } else if l.starts_with('/') {
                l.split_whitespace().next().map(|s| s.to_string())
            } else {
                None
            }
        })
        .collect())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn linked_libraries(_exe: &std::path::Path) -> Result<Vec<String>, String> {
    // Windows and others: dependency enumeration not implemented in this
    // spike; the check reports UNSUPPORTED rather than pretending to pass.
    Err("linked-library enumeration not implemented for this platform".into())
}

fn main() {
    let mut failures: Vec<String> = Vec::new();

    let sqlite = match check_sqlite() {
        Ok(v) => json!({"status": "ok", "sqlite_version": v, "linkage": "bundled (static)"}),
        Err(e) => {
            failures.push(format!("sqlite: {e}"));
            json!({"status": "missing", "error": e})
        }
    };

    let tls = match check_tls_backend() {
        Ok(backend) => json!({"status": "ok", "backend": backend}),
        Err(e) => {
            failures.push(format!("tls: {e}"));
            json!({"status": "missing", "error": e})
        }
    };

    let exe = std::env::current_exe().unwrap_or_else(|_| "<unknown>".into());
    let dylibs = match linked_libraries(&exe) {
        Ok(libs) => {
            let mut entries = Vec::new();
            for lib in &libs {
                // System dylibs on macOS may live in the dyld shared cache and
                // not as real files; presence in cache is verified by the OS at
                // load time, so only real-path entries are existence-checked.
                let exists = std::path::Path::new(lib).exists();
                let in_cache = lib.starts_with("/usr/lib/") || lib.starts_with("/System/");
                let status = if exists {
                    "ok"
                } else if in_cache {
                    "ok-dyld-shared-cache"
                } else {
                    failures.push(format!("linked library missing: {lib}"));
                    "missing"
                };
                entries.push(json!({"path": lib, "status": status}));
            }
            json!(entries)
        }
        Err(e) => {
            failures.push(format!("dylib enumeration: {e}"));
            json!({"status": "unsupported", "error": e})
        }
    };

    let report = json!({
        "check": "lingxi-spike install-check (R01-A06)",
        "exe": exe.display().to_string(),
        "sqlite": sqlite,
        "tls": tls,
        "linked_libraries": dylibs,
        "result": if failures.is_empty() { "OK" } else { "MISSING_DEPENDENCY" },
    });
    println!("{report}");

    if failures.is_empty() {
        println!("SPIKE_CHECK_DEPS_OK");
    } else {
        for f in &failures {
            eprintln!("MISSING_DEPENDENCY: {f}");
        }
        std::process::exit(3);
    }
}
