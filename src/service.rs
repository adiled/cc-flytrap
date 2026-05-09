//! Cross-platform user-mode service registration.
//!
//! Wraps each platform's "run this binary at login, restart on exit" mechanism
//! behind one API:
//!
//!   - macOS:   launchd (~/Library/LaunchAgents/com.ccft.plist + launchctl)
//!   - Linux:   systemd-user (~/.config/systemd/user/com.ccft.service + systemctl --user)
//!   - Windows: not implemented — install copies the binary; service mode is
//!              a no-op (run `ccft run` manually or wrap with NSSM/sc.exe).
//!
//! The label `com.ccft` is the same across platforms so messages / logs read
//! consistently.

use std::path::{Path, PathBuf};

pub const LABEL: &str = "com.ccft";

/// Where the unit/plist file lives on this platform.
pub fn unit_path() -> PathBuf {
    platform::unit_path()
}

/// Write the unit/plist file pointing at the installed binary. Idempotent.
pub fn write_unit(bin: &Path) -> Result<(), Box<dyn std::error::Error>> {
    platform::write_unit(bin)
}

/// Register with the platform's service manager so the daemon auto-starts.
pub fn register() -> Result<(), Box<dyn std::error::Error>> {
    platform::register()
}

/// Unregister from the service manager. Idempotent — silent if not registered.
pub fn unregister() -> Result<(), Box<dyn std::error::Error>> {
    platform::unregister()
}

/// Kick the daemon (start, or restart if already running).
pub fn kickstart() -> Result<(), Box<dyn std::error::Error>> {
    platform::kickstart()
}

/// Stop the daemon. The unit stays registered — on next login the service
/// manager will respawn it. To remove permanently use `unregister`.
pub fn bootout() -> Result<(), Box<dyn std::error::Error>> {
    platform::bootout()
}

/// Is the unit registered with the service manager (i.e., would it auto-start
/// at next login)?
pub fn is_registered() -> bool {
    platform::is_registered()
}

/// Does this platform support automatic service registration? (False on
/// Windows for now.)
pub fn supported() -> bool {
    platform::SUPPORTED
}

/// Human-readable name of the service manager — used in status messages.
pub fn manager_name() -> &'static str {
    platform::MANAGER_NAME
}

// ─── macOS: launchd ──────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use crate::config::paths;
    use std::fs;
    use std::io::Write;
    use std::process::Command;

    pub const SUPPORTED: bool = true;
    pub const MANAGER_NAME: &str = "launchd";

    pub fn unit_path() -> PathBuf {
        paths::root()
            .join("Library")
            .join("LaunchAgents")
            .join(format!("{}.plist", LABEL))
    }

    fn unit_dir() -> PathBuf {
        unit_path().parent().unwrap().to_path_buf()
    }

    pub fn write_unit(bin: &Path) -> Result<(), Box<dyn std::error::Error>> {
        fs::create_dir_all(unit_dir())?;
        let log = paths::launchd_log();
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{bin}</string>
        <string>run</string>
    </array>
    <key>RunAtLoad</key><true/>
    <key>KeepAlive</key><true/>
    <key>StandardOutPath</key><string>{log}</string>
    <key>StandardErrorPath</key><string>{log}</string>
</dict>
</plist>
"#,
            label = LABEL,
            bin = bin.display(),
            log = log.display(),
        );
        let mut f = fs::File::create(unit_path())?;
        f.write_all(plist.as_bytes())?;
        Ok(())
    }

    pub fn register() -> Result<(), Box<dyn std::error::Error>> {
        if paths::is_isolated() {
            return Ok(()); // CCFT_PREFIX-isolated test: skip launchctl
        }
        // Bootout first so an old definition can't get in the way.
        let _ = bootout();
        let target = launchctl_user_target();
        let status = Command::new("launchctl")
            .args(["bootstrap", &target, unit_path().to_string_lossy().as_ref()])
            .status()?;
        if !status.success() {
            return Err(format!("launchctl bootstrap failed: {}", status).into());
        }
        Ok(())
    }

    pub fn unregister() -> Result<(), Box<dyn std::error::Error>> {
        if !paths::is_isolated() {
            let _ = bootout();
        }
        if unit_path().exists() {
            fs::remove_file(unit_path())?;
        }
        Ok(())
    }

    pub fn kickstart() -> Result<(), Box<dyn std::error::Error>> {
        let target = format!("{}/{}", launchctl_user_target(), LABEL);
        let status = Command::new("launchctl")
            .args(["kickstart", "-k", &target])
            .status()?;
        if !status.success() {
            return Err(format!("launchctl kickstart failed: {}", status).into());
        }
        Ok(())
    }

    pub fn bootout() -> Result<(), Box<dyn std::error::Error>> {
        let target = format!("{}/{}", launchctl_user_target(), LABEL);
        // Idempotent: silence stderr ("Boot-out failed: 3: No such process").
        let _ = Command::new("launchctl")
            .args(["bootout", &target])
            .stderr(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .status();
        Ok(())
    }

    pub fn is_registered() -> bool {
        if paths::is_isolated() {
            return unit_path().exists() && paths::install_bin().exists();
        }
        let target = format!("{}/{}", launchctl_user_target(), LABEL);
        Command::new("launchctl")
            .args(["print", &target])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn launchctl_user_target() -> String {
        format!("gui/{}", libc_uid())
    }

    fn libc_uid() -> u32 {
        unsafe extern "C" {
            safe fn getuid() -> u32;
        }
        getuid()
    }
}

// ─── Linux: systemd-user ─────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use crate::config::paths;
    use std::fs;
    use std::process::Command;

    pub const SUPPORTED: bool = true;
    pub const MANAGER_NAME: &str = "systemd";

    pub fn unit_path() -> PathBuf {
        paths::root()
            .join(".config")
            .join("systemd")
            .join("user")
            .join(format!("{}.service", LABEL))
    }

    fn unit_dir() -> PathBuf {
        unit_path().parent().unwrap().to_path_buf()
    }

    pub fn write_unit(bin: &Path) -> Result<(), Box<dyn std::error::Error>> {
        fs::create_dir_all(unit_dir())?;
        let unit = format!(
            r#"[Unit]
Description=ccft - Claude Code flytrap
After=network-online.target

[Service]
Type=simple
ExecStart={bin} run
Restart=always
RestartSec=2

[Install]
WantedBy=default.target
"#,
            bin = bin.display(),
        );
        fs::write(unit_path(), unit)?;
        Ok(())
    }

    pub fn register() -> Result<(), Box<dyn std::error::Error>> {
        if paths::is_isolated() {
            return Ok(());
        }
        let _ = Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status();
        let status = Command::new("systemctl")
            .args(["--user", "enable", "--now", LABEL])
            .status()?;
        if !status.success() {
            return Err(format!("systemctl --user enable failed: {}", status).into());
        }
        Ok(())
    }

    pub fn unregister() -> Result<(), Box<dyn std::error::Error>> {
        if !paths::is_isolated() {
            let _ = Command::new("systemctl")
                .args(["--user", "disable", "--now", LABEL])
                .stderr(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .status();
        }
        if unit_path().exists() {
            fs::remove_file(unit_path())?;
        }
        if !paths::is_isolated() {
            let _ = Command::new("systemctl")
                .args(["--user", "daemon-reload"])
                .status();
        }
        Ok(())
    }

    pub fn kickstart() -> Result<(), Box<dyn std::error::Error>> {
        let status = Command::new("systemctl")
            .args(["--user", "restart", LABEL])
            .status()?;
        if !status.success() {
            return Err(format!("systemctl --user restart failed: {}", status).into());
        }
        Ok(())
    }

    pub fn bootout() -> Result<(), Box<dyn std::error::Error>> {
        let _ = Command::new("systemctl")
            .args(["--user", "stop", LABEL])
            .stderr(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .status();
        Ok(())
    }

    pub fn is_registered() -> bool {
        if paths::is_isolated() {
            return unit_path().exists() && paths::install_bin().exists();
        }
        Command::new("systemctl")
            .args(["--user", "is-enabled", LABEL])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

// ─── Windows: not yet implemented ────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod platform {
    use super::*;

    pub const SUPPORTED: bool = false;
    pub const MANAGER_NAME: &str = "(none)";

    pub fn unit_path() -> PathBuf {
        PathBuf::new()
    }

    pub fn write_unit(_bin: &Path) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    pub fn register() -> Result<(), Box<dyn std::error::Error>> {
        eprintln!(
            "Note: ccft service auto-start is not yet implemented on Windows."
        );
        eprintln!("      Run `ccft run` manually, or wrap with NSSM / sc.exe.");
        Ok(())
    }

    pub fn unregister() -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    pub fn kickstart() -> Result<(), Box<dyn std::error::Error>> {
        Err("ccft service mode not supported on Windows yet".into())
    }

    pub fn bootout() -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    pub fn is_registered() -> bool {
        false
    }
}
