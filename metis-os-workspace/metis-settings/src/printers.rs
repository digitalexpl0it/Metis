//! CUPS printer listing via `lpstat`.

use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub struct PrinterInfo {
    pub name: String,
    pub state: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PrintersSnapshot {
    pub printers: Vec<PrinterInfo>,
    pub default_printer: Option<String>,
    pub cups_available: bool,
}

pub fn load_snapshot() -> PrintersSnapshot {
    if Command::new("lpstat").arg("-v").output().is_err() {
        return PrintersSnapshot::default();
    }
    let default_printer = lpstat_line(&["-d"]);
    let printers = list_printers(&default_printer);
    PrintersSnapshot {
        printers,
        default_printer,
        cups_available: true,
    }
}

pub fn open_printer_settings() {
    for cmd in ["system-config-printer", "cups"] {
        if Command::new(cmd)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok()
        {
            return;
        }
    }
    let _ = Command::new("xdg-open")
        .arg("http://localhost:631")
        .spawn();
}

fn list_printers(default: &Option<String>) -> Vec<PrinterInfo> {
    let output = match run_lpstat(&["-p"]) {
        Some(o) => o,
        None => return Vec::new(),
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut printers = Vec::new();
    for line in text.lines() {
        // printer HP_LaserJet is idle.  enabled since ...
        let Some(rest) = line.strip_prefix("printer ") else {
            continue;
        };
        let name = rest.split_whitespace().next().unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        let state = if line.contains("disabled") {
            "Disabled"
        } else if line.contains("idle") {
            "Idle"
        } else if line.contains("printing") {
            "Printing"
        } else {
            "Unknown"
        }
        .to_string();
        printers.push(PrinterInfo {
            is_default: default.as_deref() == Some(name.as_str()),
            name,
            state,
        });
    }
    printers.sort_by(|a, b| {
        b.is_default
            .cmp(&a.is_default)
            .then_with(|| a.name.cmp(&b.name))
    });
    printers
}

fn lpstat_line(args: &[&str]) -> Option<String> {
    let output = run_lpstat(args)?;
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("system default destination: ") {
            let name = rest.trim().to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

fn run_lpstat(args: &[&str]) -> Option<std::process::Output> {
    let mut cmd = Command::new("lpstat");
    cmd.args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let Ok(mut child) = cmd.spawn() else {
        return None;
    };
    let deadline = Instant::now() + Duration::from_millis(1200);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) => {}
            Err(_) => return None,
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
}
