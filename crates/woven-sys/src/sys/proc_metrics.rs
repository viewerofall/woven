//! Reads /proc/[pid]/stat to compute CPU% and memory per process.
//! Aggregates per workspace by matching PIDs to Window.pid fields.

use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::time::Instant;
use woven_common::types::{ProcessMetrics, WorkspaceMetrics, Workspace};

/// Cached tick data for delta CPU calculation
#[derive(Default)]
struct ProcTick {
    utime:    u64,
    stime:    u64,
    sampled:  Option<Instant>,
}

#[derive(Default)]
pub struct MetricsCollector {
    prev_ticks:  HashMap<u32, ProcTick>,
    clock_ticks: u64,   // sysconf(_SC_CLK_TCK), almost always 100
}

impl MetricsCollector {
    pub fn new() -> Self {
        // _SC_CLK_TCK via libc would be ideal; 100 is correct on all modern Linux
        Self { clock_ticks: 100, ..Default::default() }
    }

    /// Collect metrics for a set of PIDs, return per-PID results
    pub fn collect(&mut self, pids: &[u32]) -> Vec<ProcessMetrics> {
        let now = Instant::now();
        let mut results = Vec::with_capacity(pids.len());

        for &pid in pids {
            if let Ok(m) = self.read_pid(pid, now) {
                results.push(m);
            }
        }
        results
    }

    /// Aggregate flat process metrics up to workspace level
    pub fn aggregate(
        &mut self,
        workspaces: &[Workspace],
    ) -> Vec<WorkspaceMetrics> {
        workspaces.iter().map(|ws| {
            let pids: Vec<u32> = ws.windows.iter()
                .filter_map(|w| w.pid)
                .collect();

            let procs    = self.collect(&pids);
            let cpu_total = procs.iter().map(|p| p.cpu_pct).sum();
            let mem_total = procs.iter().map(|p| p.mem_kb).sum();

            WorkspaceMetrics {
                workspace_id: ws.id,
                cpu_total,
                mem_total_kb: mem_total,
                procs,
            }
        }).collect()
    }

    fn read_pid(&mut self, pid: u32, now: Instant) -> Result<ProcessMetrics> {
        let stat_path = format!("/proc/{}/stat", pid);
        let stat      = fs::read_to_string(&stat_path)?;
        let fields    = parse_stat(&stat)?;

        let utime = fields[13];
        let stime = fields[14];
        let total = utime + stime;

        // memory: field 23 = vsize bytes, field 24 = rss pages
        let rss_pages: u64 = fields[23];
        let mem_kb    = rss_pages * 4;   // 4KB pages on x86-64

        let cpu_pct = if let Some(prev) = self.prev_ticks.get(&pid) {
            if let Some(prev_time) = prev.sampled {
                let elapsed_secs = (now - prev_time).as_secs_f32();
                let tick_delta   = total.saturating_sub(prev.utime + prev.stime) as f32;
                (tick_delta / self.clock_ticks as f32) / elapsed_secs * 100.0
            } else { 0.0 }
        } else { 0.0 };

        self.prev_ticks.insert(pid, ProcTick {
            utime: total, stime: 0, sampled: Some(now)
        });

        Ok(ProcessMetrics { pid, cpu_pct, mem_kb })
    }
}

/// Parse the relevant numeric fields out of /proc/[pid]/stat
/// Fields are space-separated but the comm field (2) can contain spaces
/// so we skip past the closing ')' first.
fn parse_stat(raw: &str) -> Result<Vec<u64>> {
    let after_comm = raw.rfind(')')
        .ok_or_else(|| anyhow::anyhow!("malformed stat"))?;
    let rest = &raw[after_comm + 2..];   // skip ') '

    // we need fields 14,15,24 (0-indexed from after comm = fields 1,2,11)
    // easier to just collect all and index
    let mut fields = vec![0u64, 0u64]; // placeholder for pid, comm
    for part in rest.split_whitespace() {
        fields.push(part.parse().unwrap_or(0));
    }
    Ok(fields)
}
