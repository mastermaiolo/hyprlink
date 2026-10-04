//! "Este PC": real numbers about the machine the GUI runs on, read straight
//! from /proc and /sys (no extra crates). Secondary to the phone by design —
//! on the PC side, the remote device is the headline.

use std::fs;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Host {
    pub hostname: String,
    pub cpu_model: String,
    pub kernel: String,
    pub compositor: String,
    pub cpu_pct: f32,
    pub ram_used_gb: f32,
    pub ram_total_gb: f32,
    /// (percent, charging) — `None` on desktops.
    pub battery: Option<(u8, bool)>,
    pub uptime_s: u64,
}

#[derive(Default)]
pub struct Probe {
    last_cpu: Option<(u64, u64)>,
}

fn read(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

impl Probe {
    pub fn sample(&mut self) -> Host {
        let hostname = read("/proc/sys/kernel/hostname").unwrap_or_else(|| "localhost".into());
        let kernel = read("/proc/sys/kernel/osrelease").unwrap_or_default();
        let cpu_model = read("/proc/cpuinfo")
            .and_then(|s| {
                s.lines()
                    .find(|l| l.starts_with("model name"))
                    .and_then(|l| l.split(':').nth(1))
                    .map(|m| m.trim().replace("(R)", "").replace("(TM)", ""))
            })
            .unwrap_or_default();
        let compositor = if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
            "Hyprland".into()
        } else {
            std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "—".into())
        };

        // CPU usage from the delta of /proc/stat.
        let cpu_pct = read("/proc/stat")
            .and_then(|s| {
                let f: Vec<u64> = s
                    .lines()
                    .next()?
                    .split_whitespace()
                    .skip(1)
                    .filter_map(|v| v.parse().ok())
                    .collect();
                let idle = f.get(3)? + f.get(4).unwrap_or(&0);
                let total: u64 = f.iter().sum();
                let pct = self.last_cpu.map(|(pi, pt)| {
                    let dt = total.saturating_sub(pt).max(1) as f32;
                    100.0 * (1.0 - idle.saturating_sub(pi) as f32 / dt)
                });
                self.last_cpu = Some((idle, total));
                pct
            })
            .unwrap_or(0.0)
            .clamp(0.0, 100.0);

        let mem = read("/proc/meminfo").unwrap_or_default();
        let kb = |key: &str| -> f32 {
            mem.lines()
                .find(|l| l.starts_with(key))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(0.0)
        };
        let total = kb("MemTotal:");
        let avail = kb("MemAvailable:");

        let battery = fs::read_dir("/sys/class/power_supply")
            .ok()
            .and_then(|dir| {
                dir.flatten().find_map(|e| {
                    let p = e.path();
                    let kind = fs::read_to_string(p.join("type")).ok()?;
                    if kind.trim() != "Battery" {
                        return None;
                    }
                    let cap: u8 = fs::read_to_string(p.join("capacity"))
                        .ok()?
                        .trim()
                        .parse()
                        .ok()?;
                    let status = fs::read_to_string(p.join("status")).unwrap_or_default();
                    Some((cap, status.trim() == "Charging"))
                })
            });

        let uptime_s = read("/proc/uptime")
            .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok())
            .unwrap_or(0.0) as u64;

        Host {
            hostname,
            cpu_model,
            kernel,
            compositor,
            cpu_pct,
            ram_used_gb: (total - avail) / 1_048_576.0,
            ram_total_gb: total / 1_048_576.0,
            battery,
            uptime_s,
        }
    }
}
