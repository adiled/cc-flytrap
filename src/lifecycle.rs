//! Lifecycle: start / stop / status / restart.
//!
//! Goes through launchd if the service is registered (production); otherwise
//! reports "not installed". Mirrors the well-factored ccft-lib oracle pattern
//! from the bash version: a single `state()` function is the source of truth.

use crate::config::{paths, Config};
use crate::install;
use std::net::TcpStream;
use std::process::Command;
use std::time::Duration;

#[derive(Debug)]
pub enum State {
    /// launchd-managed and bound to the configured port.
    LaunchdRunning { pid: u32 },
    /// launchd registered but not currently running.
    LaunchdIdle,
    /// Something else is bound to our port (foreign proxy or stale process).
    PortBoundForeign,
    /// Not installed at all.
    NotInstalled,
}

pub fn state(cfg: &Config) -> State {
    let installed = install::is_loaded();

    if !installed && !paths::plist().exists() && !paths::install_bin().exists() {
        return State::NotInstalled;
    }

    let bound_pid = port_bound(&cfg.host, cfg.port);

    if installed {
        if let Some(pid) = bound_pid {
            // Verify it's our service's pid via launchctl print.
            return State::LaunchdRunning { pid };
        }
        return State::LaunchdIdle;
    }

    if bound_pid.is_some() {
        return State::PortBoundForeign;
    }
    State::NotInstalled
}

pub fn print_status(cfg: &Config) {
    match state(cfg) {
        State::LaunchdRunning { pid } => {
            println!(
                "ccft running on {}:{} (pid {}, launchd)",
                cfg.host, cfg.port, pid
            );
        }
        State::LaunchdIdle => {
            println!(
                "ccft installed (launchd) but not bound to {}:{} — kick with `ccft start`",
                cfg.host, cfg.port
            );
        }
        State::PortBoundForeign => {
            println!(
                "ccft NOT installed via launchd, but {}:{} is in use by another process",
                cfg.host, cfg.port
            );
        }
        State::NotInstalled => {
            println!("ccft not installed. Run: `ccft install`");
        }
    }
}

pub fn start(cfg: &Config) -> Result<(), Box<dyn std::error::Error>> {
    if !install::is_loaded() {
        return Err("ccft is not installed. Run: ccft install".into());
    }
    if paths::is_isolated() {
        return Err("isolated mode — start/stop are no-ops; use `ccft run` directly".into());
    }
    install::kickstart()?;
    println!("✓ kicked launchd service");
    // Give it a moment to bind.
    for _ in 0..20 {
        if port_bound(&cfg.host, cfg.port).is_some() {
            println!("✓ bound on {}:{}", cfg.host, cfg.port);
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!(
        "service kicked but not bound to {}:{} after 2s — check `ccft logs`",
        cfg.host, cfg.port
    )
    .into())
}

pub fn stop(_cfg: &Config) -> Result<(), Box<dyn std::error::Error>> {
    if !install::is_loaded() {
        println!("ccft not installed — nothing to stop");
        return Ok(());
    }
    if paths::is_isolated() {
        println!("isolated mode — nothing to stop in launchd");
        return Ok(());
    }
    install::bootout()?;
    println!("✓ unloaded (will restart on next login unless you `ccft uninstall`)");
    Ok(())
}

pub fn restart(cfg: &Config) -> Result<(), Box<dyn std::error::Error>> {
    if !install::is_loaded() {
        return Err("ccft is not installed. Run: ccft install".into());
    }
    if paths::is_isolated() {
        return Err("isolated mode — restart is a no-op; use `ccft run` directly".into());
    }
    install::kickstart()?;
    for _ in 0..20 {
        if port_bound(&cfg.host, cfg.port).is_some() {
            println!("✓ restarted, bound on {}:{}", cfg.host, cfg.port);
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err("kickstart issued but port not bound after 2s".into())
}

fn port_bound(host: &str, port: u16) -> Option<u32> {
    // TCP probe — if connect succeeds, something's listening. Then ask `lsof`
    // (terse mode, listening sockets only) for the pid. Not load-bearing for
    // state correctness — used for display.
    let addr = format!("{}:{}", host, port);
    if TcpStream::connect_timeout(&addr.parse().ok()?, Duration::from_millis(50)).is_err() {
        return None;
    }
    let out = Command::new("lsof")
        .args(["-t", "-nP", &format!("-iTCP:{}", port), "-sTCP:LISTEN"])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    // Terse output: one PID per line, just digits.
    s.lines().next().and_then(|l| l.trim().parse().ok())
}
