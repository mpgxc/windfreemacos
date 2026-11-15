use std::{fs, path::Path};

use serde::{Deserialize, Serialize};
use tauri::Theme;

use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub device_id: Option<String>,
    pub device_label: Option<String>,
    pub poll_interval_secs: u64,
    pub retention_days: u64,
    pub appearance: Appearance,
    pub tray: TraySettings,
    pub alerts: AlertSettings,
}

/// Window theme; `System` follows the macOS appearance.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

impl Appearance {
    pub fn theme(self) -> Option<Theme> {
        match self {
            Self::System => None,
            Self::Light => Some(Theme::Light),
            Self::Dark => Some(Theme::Dark),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TraySettings {
    pub show_temperature: bool,
    pub show_humidity: bool,
    pub show_setpoint: bool,
    pub animate_fan: bool,
}

/// Thresholds set to `None` are disabled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AlertSettings {
    pub enabled: bool,
    pub temperature_above: Option<f64>,
    pub temperature_below: Option<f64>,
    pub humidity_above: Option<f64>,
    pub humidity_below: Option<f64>,
    pub offline: bool,
    pub filter: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            device_id: None,
            device_label: None,
            poll_interval_secs: 30,
            retention_days: 7,
            appearance: Appearance::System,
            tray: TraySettings::default(),
            alerts: AlertSettings::default(),
        }
    }
}

impl Default for TraySettings {
    fn default() -> Self {
        Self { show_temperature: true, show_humidity: true, show_setpoint: true, animate_fan: true }
    }
}

impl Default for AlertSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            temperature_above: Some(30.0),
            temperature_below: None,
            humidity_above: Some(70.0),
            humidity_below: Some(30.0),
            offline: true,
            filter: true,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        let Ok(bytes) = fs::read(path) else { return Self::default() };
        serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            eprintln!("configurações inválidas em {}: {e}; usando padrão", path.display());
            Self::default()
        })
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }

    /// Clamps values that come from the webview; the SmartThings API rate-limits fast polling.
    pub fn sanitized(mut self) -> Self {
        self.poll_interval_secs = self.poll_interval_secs.clamp(10, 600);
        self.retention_days = self.retention_days.clamp(1, 90);
        self
    }
}

/// The SmartThings token lives in the macOS Keychain, never in the settings file.
pub mod token {
    use keyring::{Entry, Error as KeyringError};

    use crate::error::Result;

    const SERVICE: &str = "dev.mpgxc.windfree";
    const ACCOUNT: &str = "smartthings-token";

    pub fn load() -> Result<Option<String>> {
        match Entry::new(SERVICE, ACCOUNT)?.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn store(token: &str) -> Result<()> {
        Ok(Entry::new(SERVICE, ACCOUNT)?.set_password(token)?)
    }
}
