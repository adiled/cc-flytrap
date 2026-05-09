//! install / uninstall — copy the binary to `~/.local/bin/ccft`, set up CA
//! and config, and register with the platform's user-mode service manager
//! (launchd / systemd-user). Idempotent both directions.

use crate::config::paths;
use crate::service;
use crate::trust;
use std::fs;
#[cfg(target_os = "macos")]
use std::process::Command;

pub fn install() -> Result<(), Box<dyn std::error::Error>> {
    let src = std::env::current_exe()?;
    let dst = paths::install_bin();

    fs::create_dir_all(paths::install_bin_dir())?;
    fs::create_dir_all(paths::log_dir())?;
    fs::create_dir_all(paths::config_dir())?;

    // 1. Copy ourselves to the install location if running from elsewhere.
    if src != dst {
        let _ = service::bootout();
        if dst.exists() {
            fs::remove_file(&dst)?;
        }
        fs::copy(&src, &dst)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&dst)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&dst, perms)?;
        }

        // macOS gotcha: overwriting a code-signed binary in place leaves
        // the kernel's path-cache flagging that location as "tampered" →
        // future invocations get SIGKILLed before main() runs. Re-sign
        // adhoc at the new path. Failure here is non-fatal.
        #[cfg(target_os = "macos")]
        {
            let _ = Command::new("codesign")
                .args(["--force", "--sign", "-", dst.to_string_lossy().as_ref()])
                .status();
        }
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
        fs::write(&cfg_path, serde_json::to_string_pretty(&default_cfg)? + "\n")?;
        println!("✓ wrote default config {}", cfg_path.display());
    } else {
        println!("✓ config exists at {}", cfg_path.display());
    }

    // 3. CA on disk.
    trust::ensure_ca()?;

    // 4. Register with the platform's service manager (or skip on Windows).
    if service::supported() {
        service::write_unit(&dst)?;
        if paths::is_isolated() {
            println!(
                "✓ service unit written; {} skipped (CCFT_PREFIX={})",
                service::manager_name(),
                paths::root().display()
            );
        } else {
            service::register()?;
            println!(
                "✓ {} service '{}' registered",
                service::manager_name(),
                service::LABEL
            );
        }
    } else {
        println!(
            "ℹ service auto-start not implemented on this platform — run `ccft run` manually."
        );
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
    if service::supported() {
        service::unregister()?;
        if paths::is_isolated() {
            println!("✓ isolated mode — service unregister was a no-op");
        } else {
            println!("✓ {} service unregistered", service::manager_name());
        }
    }

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
