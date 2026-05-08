//! install / uninstall — copy this binary into a stable location and register
//! it with launchd so it auto-starts on login. Idempotent both directions.
//! No rsync. No source-vs-install dir distinction. The binary is the artifact.

use crate::config::paths;
use crate::trust;
use std::fs;
use std::io::Write;
use std::process::Command;

const LAUNCHD_LABEL: &str = "com.ccft";

pub fn install() -> Result<(), Box<dyn std::error::Error>> {
    let src = std::env::current_exe()?;
    let dst = paths::install_bin();

    fs::create_dir_all(paths::install_bin_dir())?;
    fs::create_dir_all(paths::log_dir())?;
    fs::create_dir_all(paths::config_dir())?;
    fs::create_dir_all(paths::launch_agents_dir())?;

    // 1. Copy ourselves to ~/.local/bin/ccft if running from elsewhere.
    //
    // macOS gotcha: overwriting a code-signed binary in place leaves the
    // kernel's path-cache flagging that location as "tampered" → future
    // invocations get SIGKILLed before main() runs. The fix is rm + cp +
    // re-sign (force adhoc), which gives the OS a fresh inode and a fresh
    // signature anchored to the new path.
    if src != dst {
        let _ = bootout();
        if dst.exists() {
            fs::remove_file(&dst)?;
        }
        fs::copy(&src, &dst)?;
        let mut perms = fs::metadata(&dst)?.permissions();
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o755);
        fs::set_permissions(&dst, perms)?;

        // Re-sign adhoc at the new path. Failure here is non-fatal — the
        // freshly-copied file might already carry a usable signature on some
        // macOS versions — but the explicit codesign avoids the path-cache
        // kill on every macOS we've tested.
        let _ = Command::new("codesign")
            .args(["--force", "--sign", "-", dst.to_string_lossy().as_ref()])
            .status();
        println!("✓ installed binary {}", dst.display());
    } else {
        println!("✓ binary already at {}", dst.display());
    }

    // 2. Default config if none.
    let cfg_path = paths::config();
    if !cfg_path.exists() {
        let default_cfg = serde_json::json!({
            "host":            "127.0.0.1",
            "port":            7178,
            "system_override": "",
            "pain":            false,
            "ledger":          true
        });
        fs::write(
            &cfg_path,
            serde_json::to_string_pretty(&default_cfg)? + "\n",
        )?;
        println!("✓ wrote default config {}", cfg_path.display());
    } else {
        println!("✓ config exists at {}", cfg_path.display());
    }

    // 3. Generate CA on disk (no-op if present).
    trust::ensure_ca()?;

    // 4. Write plist + bootstrap (skip launchctl in isolated mode).
    write_plist(&dst)?;
    if paths::is_isolated() {
        println!(
            "✓ plist written; launchctl skipped (CCFT_PREFIX={})",
            paths::root().display()
        );
    } else {
        bootstrap()?;
        println!("✓ launchd service '{}' registered", LAUNCHD_LABEL);
    }

    println!();
    println!("ccft - an agentic self improvement tool — installed.");
    println!();
    if !paths::is_isolated() {
        trust::print_instructions();
    }
    Ok(())
}

pub fn uninstall() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Unload from launchd (idempotent; skip in isolated mode).
    if paths::is_isolated() {
        println!("✓ isolated mode — skipping launchctl bootout");
    } else {
        let _ = bootout();
        println!("✓ launchd service unloaded");
    }

    // 2. Remove plist.
    let plist = paths::plist();
    if plist.exists() {
        fs::remove_file(&plist)?;
        println!("✓ removed {}", plist.display());
    }

    // 3. Remove installed binary.
    let bin = paths::install_bin();
    if bin.exists() {
        fs::remove_file(&bin)?;
        println!("✓ removed {}", bin.display());
    }

    println!();
    println!("ccft uninstalled.");
    println!("  CA cert kept at: {}", paths::ca_dir().display());
    println!("  Config kept at:  {}", paths::config().display());
    println!("  Ledger kept at:  {}", paths::ledger().display());
    println!("  (delete by hand if you want a full purge)");
    Ok(())
}

fn write_plist(binary: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let log = paths::launchd_log();
    let plist_xml = format!(
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
        label = LAUNCHD_LABEL,
        bin = binary.display(),
        log = log.display(),
    );
    let mut f = fs::File::create(paths::plist())?;
    f.write_all(plist_xml.as_bytes())?;
    println!("✓ wrote {}", paths::plist().display());
    Ok(())
}

fn launchctl_user_target() -> String {
    format!("gui/{}", libc_uid())
}

fn libc_uid() -> u32 {
    // getuid is async-signal-safe and never fails. Modern Rust treats the
    // FFI declaration as safe-to-call when the function itself is sound.
    unsafe extern "C" {
        safe fn getuid() -> u32;
    }
    getuid()
}

fn bootstrap() -> Result<(), Box<dyn std::error::Error>> {
    // bootout first so an old definition can't get in the way.
    let _ = bootout();
    let target = launchctl_user_target();
    let plist = paths::plist();
    let status = Command::new("launchctl")
        .args(["bootstrap", &target, plist.to_string_lossy().as_ref()])
        .status()?;
    if !status.success() {
        return Err(format!("launchctl bootstrap failed: {}", status).into());
    }
    Ok(())
}

pub fn bootout() -> Result<(), Box<dyn std::error::Error>> {
    let target = format!("{}/{}", launchctl_user_target(), LAUNCHD_LABEL);
    // Idempotent: silence stderr ("Boot-out failed: 3: No such process") when
    // nothing's loaded. Caller treats both as success.
    let _ = Command::new("launchctl")
        .args(["bootout", &target])
        .stderr(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .status();
    Ok(())
}

pub fn kickstart() -> Result<(), Box<dyn std::error::Error>> {
    let target = format!("{}/{}", launchctl_user_target(), LAUNCHD_LABEL);
    let status = Command::new("launchctl")
        .args(["kickstart", "-k", &target])
        .status()?;
    if !status.success() {
        return Err(format!("launchctl kickstart failed: {}", status).into());
    }
    Ok(())
}

pub fn is_loaded() -> bool {
    if paths::is_isolated() {
        // In isolated mode, "loaded" means the plist + binary exist on disk.
        return paths::plist().exists() && paths::install_bin().exists();
    }
    let target = format!("{}/{}", launchctl_user_target(), LAUNCHD_LABEL);
    Command::new("launchctl")
        .args(["print", &target])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
