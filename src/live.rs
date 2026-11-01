//! Renderers for the "live" screen providers.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use chrono::{Datelike, Local};
use sysinfo::System;

use crate::font;
use crate::model::{Frame, H, W};

const TEXT: [u8; 3] = [0xe6, 0xe8, 0xed];
const DIM: [u8; 3] = [0x7c, 0x82, 0x92];
const GREEN: [u8; 3] = [0x4c, 0xc3, 0x8a];
const AMBER: [u8; 3] = [0xf0, 0xa3, 0x3c];
const RED: [u8; 3] = [0xe5, 0x53, 0x4b];

const CONTEXT_LIMIT: u64 = 200_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Clock,
    System,
    Claude,
}

impl Provider {
    pub const ALL: [Provider; 3] = [Provider::Clock, Provider::System, Provider::Claude];

    pub fn label(self) -> &'static str {
        match self {
            Provider::Clock => "Clock",
            Provider::System => "System",
            Provider::Claude => "Claude",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Provider::Clock => "Time, weekday and year on the panel",
            Provider::System => "CPU load with a gauge, plus RAM and package temperature",
            Provider::Claude => "Claude Code context usage from the local session logs",
        }
    }
}

#[derive(Clone, Copy)]
pub struct Metrics {
    pub cpu: f32,
    pub ram: f32,
    pub temp: Option<f32>,
    pub context_tokens: u64,
    #[allow(dead_code)]
    pub context_age: Option<f64>,
}

pub struct LiveState {
    system: System,
}

impl LiveState {
    pub fn new() -> Self {
        let mut system = System::new().expect("sysinfo unavailable");
        system.refresh_cpu_usage();
        Self { system }
    }

    pub fn render(&mut self, provider: Provider) -> Frame {
        match provider {
            Provider::Clock => render_clock(),
            Provider::System => self.render_system(),
            Provider::Claude => render_claude(),
        }
    }

    pub fn metrics(&mut self) -> Metrics {
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        let cpu = self.system.global_cpu_usage().clamp(0.0, 100.0);
        let used = self.system.used_memory() as f32;
        let total = self.system.total_memory().max(1) as f32;
        let (context_tokens, context_age) = claude_context();
        Metrics {
            cpu,
            ram: 100.0 * used / total,
            temp: cpu_temp(),
            context_tokens,
            context_age,
        }
    }

    fn render_system(&mut self) -> Frame {
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();

        let cpu = self.system.global_cpu_usage().clamp(0.0, 100.0);
        let used = self.system.used_memory() as f32;
        let total = self.system.total_memory().max(1) as f32;
        let ram = 100.0 * used / total;
        let colour = load_colour(cpu);

        let mut frame = Frame::blank();
        font::draw_text(&mut frame.px, W, H, 5, 4, "CPU", 1, DIM, 1);
        let value = format!("{cpu:.0}%");
        font::draw_text_centered(&mut frame.px, W, H, 22, &value, 3, colour, 1);
        frame.gauge(6, 62, W as isize - 7, cpu / 100.0, colour);

        let footnote = match cpu_temp() {
            Some(temp) => format!("RAM {ram:.0}%  {temp:.0}C"),
            None => format!("RAM {ram:.0}%"),
        };
        font::draw_text_centered(&mut frame.px, W, H, 80, &footnote, 1, DIM, 1);
        frame
    }
}

fn render_clock() -> Frame {
    let now = Local::now();
    let mut frame = Frame::blank();
    let header = now.format("%a %d %b").to_string().to_uppercase();
    font::draw_text(&mut frame.px, W, H, 5, 4, &header, 1, DIM, 1);
    let time = now.format("%H:%M").to_string();
    font::draw_text_centered(&mut frame.px, W, H, 24, &time, 3, TEXT, 1);
    let year = now.year().to_string();
    font::draw_text_centered(&mut frame.px, W, H, 78, &year, 1, DIM, 1);
    frame
}

fn render_claude() -> Frame {
    let (tokens, age) = claude_context();
    let fraction = (tokens as f32 / CONTEXT_LIMIT as f32).clamp(0.0, 1.0);
    let colour = if fraction < 0.5 {
        GREEN
    } else if fraction < 0.8 {
        AMBER
    } else {
        RED
    };

    let mut frame = Frame::blank();
    font::draw_text(&mut frame.px, W, H, 5, 4, "CLAUDE CTX", 1, DIM, 1);
    let value = format!("{:.0}%", fraction * 100.0);
    font::draw_text_centered(&mut frame.px, W, H, 22, &value, 3, colour, 1);
    frame.gauge(6, 62, W as isize - 7, fraction, colour);
    let note = if tokens == 0 {
        "NO SESSION FOUND".to_string()
    } else {
        match age {
            Some(seconds) => format!("{} AGO", human_age(seconds).to_uppercase()),
            None => "LIVE".to_string(),
        }
    };
    font::draw_text_centered(&mut frame.px, W, H, 80, &note, 1, DIM, 1);
    frame
}

pub fn load_colour(cpu: f32) -> [u8; 3] {
    if cpu < 40.0 {
        GREEN
    } else if cpu < 75.0 {
        AMBER
    } else {
        RED
    }
}


pub fn cpu_temp() -> Option<f32> {
    for entry in fs::read_dir("/sys/class/hwmon").ok()? {
        let entry = entry.ok()?;
        let base = entry.path();
        for index in 0..4 {
            let path = base.join(format!("temp{index}_input"));
            let Ok(text) = fs::read_to_string(&path) else { continue };
            let Ok(value) = text.trim().parse::<f32>() else { continue };
            let celsius = value / 1000.0;
            if (20.0..120.0).contains(&celsius) {
                return Some(celsius);
            }
        }
    }
    None
}

fn human_age(seconds: f64) -> String {
    if seconds < 120.0 {
        "just now".into()
    } else if seconds < 7_200.0 {
        format!("{:.0}m", seconds / 60.0)
    } else if seconds < 172_800.0 {
        format!("{:.0}h", seconds / 3_600.0)
    } else {
        format!("{:.0}d", seconds / 86_400.0)
    }
}

/// Context size of the most recently touched Claude Code session.
///
/// Only the tail of the newest transcript is read, so this stays cheap enough to
/// call every few seconds.
pub fn claude_context() -> (u64, Option<f64>) {
    let Some((path, modified)) = newest_transcript() else {
        return (0, None);
    };
    let Some(line) = last_usage_line(&path) else {
        return (0, None);
    };
    let Ok(record) = serde_json::from_str::<serde_json::Value>(&line) else {
        return (0, None);
    };
    let Some(usage) = record.get("message").and_then(|m| m.get("usage")) else {
        return (0, None);
    };
    let pick = |key: &str| usage.get(key).and_then(|v| v.as_u64()).unwrap_or(0);
    let tokens = pick("input_tokens") + pick("cache_read_input_tokens") + pick("cache_creation_input_tokens");
    let age = modified.elapsed().ok().map(|d| d.as_secs_f64());
    (tokens, age)
}

fn newest_transcript() -> Option<(PathBuf, std::time::SystemTime)> {
    let root = dirs_projects()?;
    let mut newest: Option<(PathBuf, std::time::SystemTime)> = None;
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let Ok(modified) = meta.modified() else { continue };
            if newest.as_ref().is_none_or(|(_, best)| modified > *best) {
                newest = Some((path, modified));
            }
        }
    }
    newest
}

fn dirs_projects() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let path = PathBuf::from(home).join(".claude/projects");
    path.is_dir().then_some(path)
}

fn last_usage_line(path: &PathBuf) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let window: u64 = 256 * 1024;
    let start = len.saturating_sub(window);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = String::new();
    file.read_to_string(&mut buf).ok()?;
    buf.lines()
        .rev()
        .find(|line| line.contains("\"usage\""))
        .map(|line| line.to_string())
}

