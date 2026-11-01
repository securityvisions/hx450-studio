//! systemd user-service management for the background live loop.
//!
//! One unit name, shared with the Python CLI: whichever tool installs last owns
//! it, so there is never more than one pusher fighting over the keyboard.

use std::path::PathBuf;
use std::process::Command;

use crate::live::Provider;

pub const UNIT: &str = "hiska-keg450-live";

fn service_name() -> String {
    format!("{UNIT}.service")
}

pub fn unit_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join(".config/systemd/user")
            .join(service_name()),
    )
}

pub fn unit_contents(provider: Provider, interval: u64) -> String {
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "hx450-studio".into());
    format!(
        "[Unit]\n\
         Description=HISKA HX-KEG450 live display\n\
         Documentation=https://github.com/securityvisions/hx450-studio\n\
         After=default.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={exe} --headless --provider {} --interval {interval}\n\
         Restart=always\n\
         RestartSec=10\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        provider.cli_name()
    )
}

fn systemctl(args: &[&str]) -> Result<(), String> {
    let status = Command::new("systemctl")
        .args(["--user"])
        .args(args)
        .status()
        .map_err(|e| format!("cannot run systemctl: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("systemctl --user {} failed", args.join(" ")))
    }
}

pub fn is_active() -> bool {
    Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", &service_name()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn install(provider: Provider, interval: u64) -> Result<(), String> {
    let path = unit_path().ok_or("HOME is not set")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {parent:?}: {e}"))?;
    }
    std::fs::write(&path, unit_contents(provider, interval))
        .map_err(|e| format!("cannot write {path:?}: {e}"))?;
    systemctl(&["daemon-reload"])?;
    systemctl(&["enable", "--now", &service_name()])
}

pub fn uninstall() -> Result<(), String> {
    let _ = systemctl(&["disable", "--now", &service_name()]);
    if let Some(path) = unit_path() {
        let _ = std::fs::remove_file(path);
    }
    systemctl(&["daemon-reload"])
}
