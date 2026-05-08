//! trust — surface the CA cert + the env vars Claude Code needs.
//!
//! This used to be auto-applied to ~/.claude.json by the bash installer, which
//! caused real damage when the proxy went down (claude → connection refused).
//! Default here is print + you decide. `--apply` flag opts in to mutating
//! ~/.claude.json with a backup.

use crate::config::{paths, Config};
use crate::proxy;
use serde_json::Value;
use std::fs;

pub fn ensure_ca() -> Result<(), Box<dyn std::error::Error>> {
    if paths::ca_pem().exists() && paths::ca_key().exists() {
        return Ok(());
    }
    // Run the async generator on a small ad-hoc runtime.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        proxy::load_or_generate_ca().await?;
        Ok::<_, Box<dyn std::error::Error>>(())
    })?;
    Ok(())
}

pub fn print_instructions() {
    let cfg = Config::load();
    let ca = paths::ca_pem();
    println!("To route Claude through ccft, set:");
    println!();
    println!("  export HTTPS_PROXY=http://{}:{}", cfg.host, cfg.port);
    println!("  export NODE_EXTRA_CA_CERTS={}", ca.display());
    println!();
    println!("Or persist for Claude Code: `ccft trust --apply`  (writes the env block into ~/.claude.json with a backup)");
    println!("To remove:                  `ccft trust --revoke`");
    println!();
    println!("Verify:");
    println!("  HTTPS_PROXY=http://{host}:{port} \\", host = cfg.host, port = cfg.port);
    println!("  NODE_EXTRA_CA_CERTS={} \\", ca.display());
    println!("  claude -p \"hi\"");
}

pub fn apply() -> Result<(), Box<dyn std::error::Error>> {
    ensure_ca()?;
    let cfg = Config::load();
    let claude_json = paths::home().join(".claude.json");
    let mut data: Value = if claude_json.exists() {
        let raw = fs::read_to_string(&claude_json)?;
        // Backup before any mutation.
        let bak = claude_json.with_extension("json.bak");
        fs::write(&bak, &raw)?;
        println!("✓ backup → {}", bak.display());
        serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    let env = data
        .as_object_mut()
        .ok_or("~/.claude.json root is not an object")?
        .entry("env")
        .or_insert_with(|| serde_json::json!({}));
    let env_obj = env.as_object_mut().ok_or("env is not an object")?;
    env_obj.insert(
        "HTTPS_PROXY".into(),
        Value::String(format!("http://{}:{}", cfg.host, cfg.port)),
    );
    env_obj.insert(
        "NODE_EXTRA_CA_CERTS".into(),
        Value::String(paths::ca_pem().to_string_lossy().to_string()),
    );

    fs::write(&claude_json, serde_json::to_string_pretty(&data)? + "\n")?;
    println!("✓ wrote env block to {}", claude_json.display());
    println!("  Restart Claude Code to pick up the new env.");
    Ok(())
}

pub fn revoke() -> Result<(), Box<dyn std::error::Error>> {
    let claude_json = paths::home().join(".claude.json");
    if !claude_json.exists() {
        println!("~/.claude.json missing — nothing to revoke");
        return Ok(());
    }
    let raw = fs::read_to_string(&claude_json)?;
    let bak = claude_json.with_extension("json.bak");
    fs::write(&bak, &raw)?;
    println!("✓ backup → {}", bak.display());

    let mut data: Value = serde_json::from_str(&raw)?;
    if let Some(obj) = data.as_object_mut() {
        if let Some(env) = obj.get_mut("env").and_then(Value::as_object_mut) {
            env.remove("HTTPS_PROXY");
            env.remove("HTTP_PROXY");
            env.remove("NODE_EXTRA_CA_CERTS");
        }
    }
    fs::write(&claude_json, serde_json::to_string_pretty(&data)? + "\n")?;
    println!("✓ removed proxy env from {}", claude_json.display());
    Ok(())
}

pub fn print_ca() -> Result<(), Box<dyn std::error::Error>> {
    ensure_ca()?;
    let pem = fs::read_to_string(paths::ca_pem())?;
    print!("{}", pem);
    Ok(())
}
