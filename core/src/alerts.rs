//! Threshold alerts with hysteresis: each alert fires once when crossed and re-arms only
//! after the reading moves back past the threshold by a margin.

use std::collections::HashSet;

use crate::{settings::AlertSettings, smartthings::AcStatus};

const TEMPERATURE_MARGIN: f64 = 0.5;
const HUMIDITY_MARGIN: f64 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Alert {
    TemperatureHigh,
    TemperatureLow,
    HumidityHigh,
    HumidityLow,
    Offline,
    Filter,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub title: String,
    pub body: String,
}

#[derive(Default)]
pub struct AlertTracker {
    active: HashSet<Alert>,
}

/// `Some(true)` = in alert, `Some(false)` = clear, `None` = inside the hysteresis band.
fn above(value: Option<f64>, limit: Option<f64>, margin: f64) -> Option<bool> {
    let Some(limit) = limit else { return Some(false) };
    let value = value?;
    if value > limit {
        Some(true)
    } else {
        (value <= limit - margin).then_some(false)
    }
}

fn below(value: Option<f64>, limit: Option<f64>, margin: f64) -> Option<bool> {
    above(value.map(|v| -v), limit.map(|l| -l), margin)
}

fn fmt(value: Option<f64>) -> String {
    value.map_or_else(|| "--".into(), |v| format!("{v:.0}"))
}

impl AlertTracker {
    pub fn evaluate(&mut self, cfg: &AlertSettings, s: &AcStatus) -> Vec<Notice> {
        if !cfg.enabled {
            self.active.clear();
            return Vec::new();
        }
        // Readings from an offline device are stale; keep their alerts as they are.
        let live = |state: Option<bool>| state.filter(|_| s.online);
        let checks = [
            (Alert::TemperatureHigh, live(above(s.temperature, cfg.temperature_above, TEMPERATURE_MARGIN))),
            (Alert::TemperatureLow, live(below(s.temperature, cfg.temperature_below, TEMPERATURE_MARGIN))),
            (Alert::HumidityHigh, live(above(s.humidity, cfg.humidity_above, HUMIDITY_MARGIN))),
            (Alert::HumidityLow, live(below(s.humidity, cfg.humidity_below, HUMIDITY_MARGIN))),
            (Alert::Offline, Some(cfg.offline && !s.online)),
            (Alert::Filter, Some(cfg.filter && s.filter_status.as_deref().is_some_and(|f| f != "normal"))),
        ];

        let mut notices = Vec::new();
        for (alert, state) in checks {
            match state {
                Some(true) if self.active.insert(alert) => notices.push(Self::notice(alert, cfg, s)),
                Some(false) => {
                    self.active.remove(&alert);
                }
                _ => {}
            }
        }
        notices
    }

    fn notice(alert: Alert, cfg: &AlertSettings, s: &AcStatus) -> Notice {
        let (title, body) = match alert {
            Alert::TemperatureHigh => (
                "Ambiente quente",
                format!("Temperatura em {} °C (limite {} °C).", fmt(s.temperature), fmt(cfg.temperature_above)),
            ),
            Alert::TemperatureLow => (
                "Ambiente frio",
                format!("Temperatura em {} °C (limite {} °C).", fmt(s.temperature), fmt(cfg.temperature_below)),
            ),
            Alert::HumidityHigh => (
                "Umidade alta",
                format!("Umidade em {}% (limite {}%).", fmt(s.humidity), fmt(cfg.humidity_above)),
            ),
            Alert::HumidityLow => (
                "Ar seco",
                format!("Umidade em {}% (limite {}%).", fmt(s.humidity), fmt(cfg.humidity_below)),
            ),
            Alert::Offline => ("Ar-condicionado offline", "O aparelho não está respondendo ao SmartThings.".into()),
            Alert::Filter => ("Limpe o filtro", "O ar-condicionado indica que o filtro precisa de limpeza.".into()),
        };
        Notice { title: title.into(), body }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(temperature: f64, online: bool) -> AcStatus {
        AcStatus { online, temperature: Some(temperature), ..Default::default() }
    }

    fn cfg() -> AlertSettings {
        AlertSettings { humidity_above: None, humidity_below: None, ..Default::default() }
    }

    #[test]
    fn fires_once_and_rearms_after_margin() {
        let (mut t, cfg) = (AlertTracker::default(), cfg());
        assert!(t.evaluate(&cfg, &status(29.0, true)).is_empty());
        assert_eq!(t.evaluate(&cfg, &status(31.0, true))[0].title, "Ambiente quente");
        assert!(t.evaluate(&cfg, &status(32.0, true)).is_empty(), "still active");
        assert!(t.evaluate(&cfg, &status(29.8, true)).is_empty(), "inside margin, stays active");
        assert!(t.evaluate(&cfg, &status(31.0, true)).is_empty(), "never re-armed");
        t.evaluate(&cfg, &status(29.5, true));
        assert_eq!(t.evaluate(&cfg, &status(31.0, true)).len(), 1, "re-armed");
    }

    #[test]
    fn offline_alert_ignores_stale_readings() {
        let (mut t, cfg) = (AlertTracker::default(), cfg());
        let notices = t.evaluate(&cfg, &status(35.0, false));
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].title, "Ar-condicionado offline");
    }

    #[test]
    fn disabled_threshold_and_low_alerts() {
        let mut t = AlertTracker::default();
        let cfg = AlertSettings { temperature_above: None, temperature_below: Some(18.0), ..cfg() };
        assert!(t.evaluate(&cfg, &status(40.0, true)).is_empty());
        assert_eq!(t.evaluate(&cfg, &status(17.0, true))[0].title, "Ambiente frio");
    }
}
