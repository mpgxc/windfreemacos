//! Sample history persisted as JSON lines in the app data directory.

use std::{
    collections::VecDeque,
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{error::Result, smartthings::AcStatus};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub ts: u64,
    pub on: bool,
    pub temperature: Option<f64>,
    pub humidity: Option<f64>,
    pub setpoint: Option<f64>,
    pub power_watts: Option<f64>,
    /// Cumulative meter reading reported by the device.
    pub energy_wh: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Stats {
    pub min: f64,
    pub max: f64,
    pub avg: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub temperature: Option<Stats>,
    pub humidity: Option<Stats>,
    pub power_watts: Option<Stats>,
    pub energy_wh: Option<f64>,
    pub on_ratio: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct History {
    pub points: Vec<Sample>,
    pub summary: Summary,
}

pub struct MetricsStore {
    path: PathBuf,
    retention_secs: u64,
    samples: VecDeque<Sample>,
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Sample {
    pub fn from_status(ts: u64, s: &AcStatus) -> Self {
        Self {
            ts,
            on: s.power,
            temperature: s.temperature,
            humidity: s.humidity,
            setpoint: s.setpoint,
            power_watts: s.power_watts,
            energy_wh: s.energy_wh,
        }
    }
}

impl MetricsStore {
    /// Loads samples within retention and rewrites the file without the expired ones.
    // shortcut: compacts only at startup, so the file grows until the next launch.
    pub fn open(path: PathBuf, retention_days: u64) -> Result<Self> {
        let retention_secs = retention_days * 86_400;
        let cutoff = now().saturating_sub(retention_secs);
        let samples: VecDeque<Sample> = fs::read_to_string(&path)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str::<Sample>(line).ok())
            .filter(|s| s.ts >= cutoff)
            .collect();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut body = String::new();
        for s in &samples {
            body.push_str(&serde_json::to_string(s)?);
            body.push('\n');
        }
        fs::write(&path, body)?;
        Ok(Self { path, retention_secs, samples })
    }

    pub fn set_retention_days(&mut self, days: u64) {
        self.retention_secs = days * 86_400;
    }

    pub fn push(&mut self, sample: Sample) -> Result<()> {
        let mut file = OpenOptions::new().create(true).append(true).open(&self.path)?;
        writeln!(file, "{}", serde_json::to_string(&sample)?)?;
        let cutoff = sample.ts.saturating_sub(self.retention_secs);
        self.samples.push_back(sample);
        while self.samples.front().is_some_and(|s| s.ts < cutoff) {
            self.samples.pop_front();
        }
        Ok(())
    }

    pub fn history(&self, since: u64, max_points: usize) -> History {
        let range: Vec<&Sample> = self.samples.iter().filter(|s| s.ts >= since).collect();
        History { points: downsample(&range, max_points.max(1)), summary: summarize(&range) }
    }
}

fn stats(values: impl Iterator<Item = f64>) -> Option<Stats> {
    let (mut min, mut max, mut sum, mut n) = (f64::INFINITY, f64::NEG_INFINITY, 0.0, 0usize);
    for v in values {
        min = min.min(v);
        max = max.max(v);
        sum += v;
        n += 1;
    }
    (n > 0).then(|| Stats { min, max, avg: sum / n as f64 })
}

fn summarize(samples: &[&Sample]) -> Summary {
    let mut energy = samples.iter().filter_map(|s| s.energy_wh);
    let energy_wh = energy.next().zip(energy.next_back()).map(|(first, last)| (last - first).max(0.0));
    let on = samples.iter().filter(|s| s.on).count();
    Summary {
        temperature: stats(samples.iter().filter_map(|s| s.temperature)),
        humidity: stats(samples.iter().filter_map(|s| s.humidity)),
        power_watts: stats(samples.iter().filter_map(|s| s.power_watts)),
        energy_wh,
        on_ratio: if samples.is_empty() { 0.0 } else { on as f64 / samples.len() as f64 },
    }
}

/// Averages consecutive samples into at most `max_points` buckets for charting.
fn downsample(samples: &[&Sample], max_points: usize) -> Vec<Sample> {
    let size = samples.len().div_ceil(max_points).max(1);
    samples
        .chunks(size)
        .map(|chunk| {
            let avg = |f: fn(&Sample) -> Option<f64>| stats(chunk.iter().filter_map(|s| f(s))).map(|s| s.avg);
            let last = chunk[chunk.len() - 1];
            Sample {
                ts: last.ts,
                on: chunk.iter().any(|s| s.on),
                temperature: avg(|s| s.temperature),
                humidity: avg(|s| s.humidity),
                setpoint: avg(|s| s.setpoint),
                power_watts: avg(|s| s.power_watts),
                energy_wh: last.energy_wh,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(ts: u64, temp: f64, energy: f64, on: bool) -> Sample {
        Sample {
            ts,
            on,
            temperature: Some(temp),
            humidity: None,
            setpoint: Some(24.0),
            power_watts: Some(1000.0),
            energy_wh: Some(energy),
        }
    }

    #[test]
    fn history_downsamples_and_summarizes() {
        let dir = std::env::temp_dir().join(format!("windfree-test-{}", std::process::id()));
        let mut store = MetricsStore::open(dir.join("m.jsonl"), 7).unwrap();
        let t0 = now();
        for i in 0..10 {
            store.push(sample(t0 + i, 20.0 + i as f64, 100.0 + 10.0 * i as f64, i >= 5)).unwrap();
        }

        let h = store.history(t0 + 2, 3);
        assert_eq!(h.points.len(), 3, "8 samples into buckets of 3,3,2");
        assert_eq!(h.points[0].temperature, Some(23.0));
        assert_eq!(h.points[2].ts, t0 + 9);
        let t = h.summary.temperature.unwrap();
        assert_eq!((t.min, t.max, t.avg), (22.0, 29.0, 25.5));
        assert_eq!(h.summary.energy_wh, Some(70.0));
        assert_eq!(h.summary.on_ratio, 5.0 / 8.0);
        assert_eq!(h.summary.humidity, None);

        let reopened = MetricsStore::open(dir.join("m.jsonl"), 7).unwrap();
        assert_eq!(reopened.samples.len(), 10, "samples survive restart");
        fs::remove_dir_all(dir).unwrap();
    }
}
