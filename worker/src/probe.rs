//! Measures CPU load and available memory to choose how many jobs to run at once.
use crate::run::cores;
use std::fs;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The first reading compares two CPU samples this far apart.
pub const SAMPLE: Duration = Duration::from_millis(100);
/// A quick reading without a fresh cached sample waits this long instead.
pub const QUICK_SAMPLE: Duration = Duration::from_millis(25);
/// A cached sample this recent replaces the wait. Younger samples span too few clock ticks to trust.
const CACHE_MIN_AGE: Duration = Duration::from_millis(20);
const CACHE_MAX_AGE: Duration = Duration::from_secs(5);
/// The background worker reserves this many cores for other work.
pub const RESERVE: usize = 1;
pub const DEFAULT_JOB_MEM_MB: u64 = 768;

#[derive(Clone, Copy)]
struct Ticks {
    busy: u64,
    total: u64,
}

fn ticks() -> Option<Ticks> {
    let stat = fs::read_to_string("/proc/stat").ok()?;
    let fields: Vec<u64> = stat
        .lines()
        .next()?
        .strip_prefix("cpu ")?
        .split_whitespace()
        .take(8)
        .filter_map(|f| f.parse().ok())
        .collect();
    if fields.len() < 5 {
        return None;
    }
    let total: u64 = fields.iter().sum();
    // idle plus iowait
    let idle = fields[3] + fields[4];
    Some(Ticks {
        busy: total.saturating_sub(idle),
        total,
    })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Every reading leaves its sample here, so the next process can measure load without sleeping.
fn cache_path() -> PathBuf {
    // SAFETY: getuid has no preconditions.
    std::env::temp_dir().join(format!("xstack-probe-{}", unsafe { libc::getuid() }))
}

/// Cached samples, newest first, each with the time it was taken.
fn samples() -> Vec<(Ticks, u64)> {
    let text = fs::read_to_string(cache_path()).unwrap_or_default();
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace().map(|f| f.parse::<u64>().ok());
            Some((
                Ticks {
                    busy: fields.next()??,
                    total: fields.next()??,
                },
                fields.next()??,
            ))
        })
        .collect()
}

fn age(at: u64) -> Duration {
    Duration::from_millis(now_ms().saturating_sub(at))
}

/// The newest cached sample old enough to measure against.
fn cached() -> Option<Ticks> {
    samples()
        .into_iter()
        .find(|&(_, at)| (CACHE_MIN_AGE..=CACHE_MAX_AGE).contains(&age(at)))
        .map(|(sample, _)| sample)
}

/// Keeps the new sample and the newest one at least `CACHE_MIN_AGE` old, so back-to-back readings never wait.
fn remember(sample: Ticks) {
    let path = cache_path();
    let mut text = format!("{} {} {}\n", sample.busy, sample.total, now_ms());
    if let Some((older, at)) = samples()
        .into_iter()
        .find(|&(_, at)| age(at) >= CACHE_MIN_AGE)
    {
        text.push_str(&format!("{} {} {at}\n", older.busy, older.total));
    }
    let staging = path.with_extension(std::process::id().to_string());
    if fs::write(&staging, text).is_ok() {
        let _ = fs::rename(&staging, &path);
    }
}

fn online_cpus() -> usize {
    let counted = fs::read_to_string("/proc/stat")
        .map(|s| {
            s.lines()
                .filter(|l| {
                    l.starts_with("cpu") && l.as_bytes().get(3).is_some_and(u8::is_ascii_digit)
                })
                .count()
        })
        .unwrap_or(0);
    if counted == 0 {
        cores()
    } else {
        counted
    }
}

fn mem_free_mb() -> Option<u64> {
    let info = fs::read_to_string("/proc/meminfo").ok()?;
    let kb: u64 = info
        .lines()
        .find_map(|l| l.strip_prefix("MemAvailable:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    Some(kb / 1024)
}

#[derive(Clone, Copy, Debug)]
pub struct Power {
    /// The process can use this many cores after affinity and cgroup limits.
    pub cores: usize,
    /// All processes used this many cores during the last sample.
    pub busy: f64,
    /// Available memory in MB, or `None` if the platform does not report it.
    pub mem_free_mb: Option<u64>,
}

impl Power {
    /// Chooses concurrency from idle cores minus `reserve` and available memory per `job_mem_mb`.
    /// Counts `ours` running jobs as reusable capacity because the sample includes their resource use.
    pub fn width(&self, reserve: usize, ours: usize, job_mem_mb: u64) -> usize {
        let ceiling = self.cores.saturating_sub(reserve).max(1);
        let others = (self.busy - ours as f64).max(0.0);
        let cpu = (self.cores as f64 - others).round().max(0.0) as usize;
        let memory = self.mem_free_mb.map_or(usize::MAX, |free| {
            (free / job_mem_mb.max(1)) as usize + ours
        });
        cpu.saturating_sub(reserve).min(memory).clamp(1, ceiling)
    }
}

/// Keeps the last CPU sample, so every later reading covers the time since the previous one without sleeping.
pub struct Meter {
    last: Option<Ticks>,
    online: usize,
}

impl Meter {
    /// Measures CPU load using two samples taken `SAMPLE` apart.
    pub fn start() -> (Meter, Power) {
        let mut meter = Meter {
            last: ticks(),
            online: online_cpus(),
        };
        sleep(SAMPLE);
        let power = meter.read();
        (meter, power)
    }

    /// Measures CPU load against the last cached sample from any process, or waits `QUICK_SAMPLE` without one.
    pub fn quick() -> (Meter, Power) {
        let mut meter = Meter {
            last: cached(),
            online: online_cpus(),
        };
        if meter.last.is_none() {
            meter.last = ticks();
            sleep(QUICK_SAMPLE);
        }
        let power = meter.read();
        (meter, power)
    }

    pub fn read(&mut self) -> Power {
        let now = ticks();
        let busy = match (self.last, now) {
            (Some(a), Some(b)) if b.total > a.total => {
                b.busy.saturating_sub(a.busy) as f64 / (b.total - a.total) as f64
                    * self.online as f64
            }
            _ => 0.0,
        };
        if let Some(sample) = now {
            self.last = now;
            remember(sample);
        }
        Power {
            cores: cores(),
            busy,
            mem_free_mb: mem_free_mb(),
        }
    }
}
