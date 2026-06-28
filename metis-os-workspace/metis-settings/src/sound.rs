//! PipeWire/PulseAudio audio device enumeration and default routing via `pactl`.

use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AudioDevice {
    pub name: String,
    pub description: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SoundSnapshot {
    pub sinks: Vec<AudioDevice>,
    pub sources: Vec<AudioDevice>,
    pub output_volume: u8,
    pub output_muted: bool,
    pub input_volume: u8,
    pub input_muted: bool,
}

pub fn load_snapshot() -> SoundSnapshot {
    let default_sink = pactl_line("get-default-sink").unwrap_or_default();
    let default_source = pactl_line("get-default-source").unwrap_or_default();
    let sinks = list_devices("sinks", &default_sink);
    let sources = list_devices("sources", &default_source);
    SoundSnapshot {
        output_volume: read_volume_percent(&["get-sink-volume", "@DEFAULT_SINK@"]).unwrap_or(0),
        output_muted: read_mute(&["get-sink-mute", "@DEFAULT_SINK@"]).unwrap_or(false),
        input_volume: read_volume_percent(&["get-source-volume", "@DEFAULT_SOURCE@"]).unwrap_or(0),
        input_muted: read_mute(&["get-source-mute", "@DEFAULT_SOURCE@"]).unwrap_or(false),
        sinks,
        sources,
    }
}

pub fn set_default_sink(name: &str) {
    let _ = run_pactl(&["set-default-sink", name]);
}

pub fn set_default_source(name: &str) {
    let _ = run_pactl(&["set-default-source", name]);
}

fn list_devices(kind: &str, default_name: &str) -> Vec<AudioDevice> {
    let output = match run_pactl(&["list", "short", kind]) {
        Some(o) => o,
        None => return Vec::new(),
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut devices = Vec::new();
    for line in text.lines() {
        let mut parts = line.split('\t');
        let Some(index) = parts.next() else { continue };
        let Some(name) = parts.next() else { continue };
        let description = parts.next().unwrap_or(name).to_string();
        if index.parse::<u32>().is_err() {
            continue;
        }
        // Skip monitor sources.
        if kind == "sources" && name.ends_with(".monitor") {
            continue;
        }
        devices.push(AudioDevice {
            name: name.to_string(),
            description,
            is_default: name == default_name,
        });
    }
    devices
}

fn read_volume_percent(args: &[&str]) -> Option<u8> {
    let output = run_pactl(args)?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.split_whitespace()
        .nth(4)
        .and_then(|s| s.trim_end_matches('%').parse().ok())
}

fn read_mute(args: &[&str]) -> Option<bool> {
    let output = run_pactl(args)?;
    let text = String::from_utf8_lossy(&output.stdout);
    if text.contains("yes") {
        Some(true)
    } else if text.contains("no") {
        Some(false)
    } else {
        None
    }
}

fn pactl_line(args: &str) -> Option<String> {
    let output = run_pactl(&[args])?;
    let line = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!line.is_empty()).then_some(line)
}

fn run_pactl(args: &[&str]) -> Option<std::process::Output> {
    let mut cmd = Command::new("pactl");
    cmd.args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut child = cmd.spawn().ok()?;
    let deadline = Instant::now() + Duration::from_millis(800);
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
