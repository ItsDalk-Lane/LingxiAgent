//! Chromium launcher using `--remote-debugging-pipe` (fd 3 = commands to
//! browser, fd 4 = responses from browser). No TCP debug port is opened.

use std::fs::File;
use std::io;
use std::os::unix::io::{FromRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command};

pub struct LaunchedBrowser {
    pub child: Child,
    pub cmd_write: File,
    pub rsp_read: File,
}

fn make_pipe() -> io::Result<(RawFd, RawFd)> {
    let mut fds = [0i32; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((fds[0], fds[1]))
}

/// Launch the browser binary with CDP pipe transport.
///
/// `extra_args` are appended after the isolation/defaults set (e.g.
/// `--proxy-server=...` for the proxy phase).
pub fn launch(
    chrome: &Path,
    user_data_dir: &Path,
    extra_args: &[String],
) -> io::Result<LaunchedBrowser> {
    let (cmd_r, cmd_w) = make_pipe()?; // browser reads commands from cmd_r (fd3)
    let (rsp_r, rsp_w) = make_pipe()?; // browser writes responses to rsp_w (fd4)

    let mut cmd = Command::new(chrome);
    cmd.arg("--remote-debugging-pipe")
        .arg(format!("--user-data-dir={}", user_data_dir.display()))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-session-crashed-bubble")
        .arg("--hide-crash-restore-bubble")
        .arg("--disable-background-networking")
        .arg("--disable-component-update")
        .arg("--disable-sync")
        .arg("--metrics-recording-only")
        .arg("--window-size=1280,860")
        .arg("about:blank");
    for a in extra_args {
        cmd.arg(a);
    }
    // fd wiring in the child: commands readable on fd3, responses writable on fd4.
    // pre_exec runs after fork; dup2 is async-signal-safe.
    unsafe {
        cmd.pre_exec(move || {
            if libc::dup2(cmd_r, 3) < 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::dup2(rsp_w, 4) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = cmd.spawn()?;
    // Parent keeps: write side of the command pipe, read side of the response pipe.
    let cmd_write = unsafe { File::from_raw_fd(cmd_w) };
    let rsp_read = unsafe { File::from_raw_fd(rsp_r) };
    // Close parent's copies of the child ends.
    unsafe {
        libc::close(cmd_r);
        libc::close(rsp_w);
    }
    Ok(LaunchedBrowser {
        child,
        cmd_write,
        rsp_read,
    })
}
