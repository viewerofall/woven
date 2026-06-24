//! Lightweight system info reader for woven-lite info bar.
//! Reads /proc and /sys directly — no external deps.

use std::fs;

#[derive(Default, Clone, Debug)]
pub struct SysSnapshot {
    pub cpu_pct:      f32,         // 0–100
    pub mem_used_kb:  u64,
    pub mem_total_kb: u64,
    pub battery_pct:  Option<f32>, // None if no battery
    pub charging:     bool,
}

impl SysSnapshot {
    pub fn mem_pct(&self) -> f32 {
        if self.mem_total_kb == 0 { return 0.0; }
        (self.mem_used_kb as f32 / self.mem_total_kb as f32) * 100.0
    }
}

// ── CPU ───────────────────────────────────────────────────────────────────────

/// One sample of /proc/stat cpu line
#[derive(Default, Clone, Copy)]
struct CpuTick {
    idle:  u64,
    total: u64,
}

fn read_cpu_tick() -> CpuTick {
    let s = fs::read_to_string("/proc/stat").unwrap_or_default();
    for line in s.lines() {
        if line.starts_with("cpu ") {
            let nums: Vec<u64> = line
                .split_whitespace()
                .skip(1)
                .filter_map(|v| v.parse().ok())
                .collect();
            if nums.len() >= 4 {
                let idle  = nums[3] + nums.get(4).copied().unwrap_or(0); // idle + iowait
                let total = nums.iter().sum();
                return CpuTick { idle, total };
            }
        }
    }
    CpuTick::default()
}

/// Stateful collector: call `sample()` twice with a sleep between for a delta.
pub struct CpuSampler {
    prev: CpuTick,
}

impl CpuSampler {
    pub fn new() -> Self {
        Self { prev: read_cpu_tick() }
    }

    /// Returns CPU% since last call.
    pub fn sample(&mut self) -> f32 {
        let cur = read_cpu_tick();
        let d_total = cur.total.saturating_sub(self.prev.total);
        let d_idle  = cur.idle.saturating_sub(self.prev.idle);
        self.prev = cur;
        if d_total == 0 { return 0.0; }
        (1.0 - d_idle as f32 / d_total as f32) * 100.0
    }
}

// ── Memory ────────────────────────────────────────────────────────────────────

fn read_mem() -> (u64, u64) {
    let s = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let mut total = 0u64;
    let mut available = 0u64;
    for line in s.lines() {
        if line.starts_with("MemTotal:") {
            total = parse_kb(line);
        } else if line.starts_with("MemAvailable:") {
            available = parse_kb(line);
        }
    }
    let used = total.saturating_sub(available);
    (used, total)
}

fn parse_kb(line: &str) -> u64 {
    line.split_whitespace()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

// ── Battery ───────────────────────────────────────────────────────────────────

fn read_battery() -> (Option<f32>, bool) {
    // Try BAT0, BAT1
    for bat in &["BAT0", "BAT1"] {
        let base = format!("/sys/class/power_supply/{}", bat);
        let cap  = fs::read_to_string(format!("{}/capacity", base))
            .ok()
            .and_then(|s| s.trim().parse::<f32>().ok());
        if let Some(pct) = cap {
            let status   = fs::read_to_string(format!("{}/status", base))
                .unwrap_or_default();
            let charging = status.trim() == "Charging" || status.trim() == "Full";
            return (Some(pct), charging);
        }
    }
    (None, false)
}

// ── Public snapshot ───────────────────────────────────────────────────────────

pub struct SysCollector {
    cpu: CpuSampler,
}

impl SysCollector {
    pub fn new() -> Self {
        Self { cpu: CpuSampler::new() }
    }

    pub fn snapshot(&mut self) -> SysSnapshot {
        let cpu_pct               = self.cpu.sample();
        let (mem_used, mem_total) = read_mem();
        let (battery_pct, charging) = read_battery();
        SysSnapshot { cpu_pct, mem_used_kb: mem_used, mem_total_kb: mem_total, battery_pct, charging }
    }
}
