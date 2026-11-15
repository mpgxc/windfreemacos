//! Minimal SmartThings REST client for a Samsung room air conditioner.

use std::time::Duration;

use reqwest::{RequestBuilder, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{Error, Result};

const API: &str = "https://api.smartthings.com/v1";
const AIR_CONDITIONER_TYPE: &str = "oic.d.airconditioner";

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    token: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub label: String,
    pub is_air_conditioner: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcStatus {
    pub online: bool,
    pub power: bool,
    pub mode: Option<String>,
    pub supported_modes: Vec<String>,
    pub fan_mode: Option<String>,
    pub supported_fan_modes: Vec<String>,
    pub oscillation: Option<String>,
    pub supported_oscillations: Vec<String>,
    pub optional_mode: Option<String>,
    pub supported_optional_modes: Vec<String>,
    pub temperature: Option<f64>,
    pub humidity: Option<f64>,
    pub setpoint: Option<f64>,
    pub setpoint_min: f64,
    pub setpoint_max: f64,
    pub power_watts: Option<f64>,
    pub energy_wh: Option<f64>,
    pub filter_usage: Option<f64>,
    pub filter_status: Option<String>,
}

/// A device command, shared by the webview (`{ kind, value }`) and the tray menu.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum AcCommand {
    Power(bool),
    Mode(String),
    Setpoint(f64),
    FanMode(String),
    Oscillation(String),
    OptionalMode(String),
}

impl AcCommand {
    fn to_api(&self) -> Value {
        let (capability, command, arguments) = match self {
            Self::Power(on) => ("switch", if *on { "on" } else { "off" }, json!([])),
            Self::Mode(m) => ("airConditionerMode", "setAirConditionerMode", json!([m])),
            Self::Setpoint(t) => ("thermostatCoolingSetpoint", "setCoolingSetpoint", json!([t])),
            Self::FanMode(m) => ("airConditionerFanMode", "setFanMode", json!([m])),
            Self::Oscillation(m) => ("fanOscillationMode", "setFanOscillationMode", json!([m])),
            Self::OptionalMode(m) => ("custom.airConditionerOptionalMode", "setAcOptionalMode", json!([m])),
        };
        json!({ "commands": [{
            "component": "main",
            "capability": capability,
            "command": command,
            "arguments": arguments,
        }]})
    }
}

impl Client {
    pub fn new(token: &str) -> Result<Self> {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(15)).build()?;
        Ok(Self { http, token: token.trim().to_owned() })
    }

    // shortcut: ignores `_links.next` pagination (200 devices per page), fine for a home account.
    pub async fn devices(&self) -> Result<Vec<Device>> {
        let body = self.get("/devices").await?;
        let items = body["items"].as_array().map(Vec::as_slice).unwrap_or_default();
        Ok(items
            .iter()
            .map(|d| Device {
                id: d["deviceId"].as_str().unwrap_or_default().to_owned(),
                label: d["label"].as_str().or(d["name"].as_str()).unwrap_or("Dispositivo").to_owned(),
                is_air_conditioner: d["ocf"]["ocfDeviceType"] == AIR_CONDITIONER_TYPE,
            })
            .collect())
    }

    pub async fn status(&self, device_id: &str) -> Result<AcStatus> {
        let (health_path, status_path) =
            (format!("/devices/{device_id}/health"), format!("/devices/{device_id}/components/main/status"));
        let (health, status) = tokio::try_join!(self.get(&health_path), self.get(&status_path))?;
        Ok(AcStatus::parse(health["state"] == "ONLINE", &status))
    }

    pub async fn execute(&self, device_id: &str, command: &AcCommand) -> Result<()> {
        let request = self.http.post(format!("{API}/devices/{device_id}/commands")).json(&command.to_api());
        let body = self.send(request).await?;
        let failed = body["results"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|r| r["status"] == "FAILED");
        if failed {
            return Err(Error::Api { status: 200, body: body.to_string() });
        }
        Ok(())
    }

    async fn get(&self, path: &str) -> Result<Value> {
        self.send(self.http.get(format!("{API}{path}"))).await
    }

    async fn send(&self, request: RequestBuilder) -> Result<Value> {
        let response = request.bearer_auth(&self.token).send().await?;
        match response.status() {
            s if s.is_success() => Ok(response.json().await?),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(Error::Unauthorized),
            s => Err(Error::Api { status: s.as_u16(), body: response.text().await.unwrap_or_default() }),
        }
    }
}

fn attr<'a>(status: &'a Value, capability: &str, attribute: &str) -> Option<&'a Value> {
    status.get(capability)?.get(attribute)?.get("value").filter(|v| !v.is_null())
}

fn number(status: &Value, capability: &str, attribute: &str) -> Option<f64> {
    attr(status, capability, attribute)?.as_f64()
}

fn text(status: &Value, capability: &str, attribute: &str) -> Option<String> {
    attr(status, capability, attribute)?.as_str().map(str::to_owned)
}

fn list(status: &Value, capability: &str, attribute: &str) -> Vec<String> {
    attr(status, capability, attribute)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

impl AcStatus {
    /// Builds the status from the `components/main/status` payload.
    pub fn parse(online: bool, s: &Value) -> Self {
        const OPTIONAL: &str = "custom.airConditionerOptionalMode";
        let energy = attr(s, "powerConsumptionReport", "powerConsumption");
        Self {
            online,
            power: text(s, "switch", "switch").as_deref() == Some("on"),
            mode: text(s, "airConditionerMode", "airConditionerMode"),
            supported_modes: list(s, "airConditionerMode", "supportedAcModes"),
            fan_mode: text(s, "airConditionerFanMode", "fanMode"),
            supported_fan_modes: list(s, "airConditionerFanMode", "supportedAcFanModes"),
            oscillation: text(s, "fanOscillationMode", "fanOscillationMode"),
            supported_oscillations: list(s, "fanOscillationMode", "supportedFanOscillationModes"),
            optional_mode: text(s, OPTIONAL, "acOptionalMode"),
            supported_optional_modes: list(s, OPTIONAL, "supportedAcOptionalMode"),
            temperature: number(s, "temperatureMeasurement", "temperature"),
            humidity: number(s, "relativeHumidityMeasurement", "humidity"),
            setpoint: number(s, "thermostatCoolingSetpoint", "coolingSetpoint"),
            setpoint_min: number(s, "custom.thermostatSetpointControl", "minimumSetpoint").unwrap_or(16.0),
            setpoint_max: number(s, "custom.thermostatSetpointControl", "maximumSetpoint").unwrap_or(30.0),
            power_watts: energy.and_then(|e| e["power"].as_f64()),
            energy_wh: energy.and_then(|e| e["energy"].as_f64()),
            filter_usage: number(s, "custom.dustFilter", "dustFilterUsage"),
            filter_status: text(s, "custom.dustFilter", "dustFilterStatus"),
        }
    }

    /// Reflects an accepted command locally until the next poll confirms it.
    pub fn apply(&mut self, command: &AcCommand) {
        match command {
            AcCommand::Power(on) => self.power = *on,
            AcCommand::Mode(m) => self.mode = Some(m.clone()),
            AcCommand::Setpoint(t) => self.setpoint = Some(*t),
            AcCommand::FanMode(m) => self.fan_mode = Some(m.clone()),
            AcCommand::Oscillation(m) => self.oscillation = Some(m.clone()),
            AcCommand::OptionalMode(m) => self.optional_mode = Some(m.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_samsung_status_payload() {
        let payload = json!({
            "switch": { "switch": { "value": "on" } },
            "airConditionerMode": {
                "airConditionerMode": { "value": "cool" },
                "supportedAcModes": { "value": ["auto", "cool"] }
            },
            "temperatureMeasurement": { "temperature": { "value": 28, "unit": "C" } },
            "thermostatCoolingSetpoint": { "coolingSetpoint": { "value": 24 } },
            "relativeHumidityMeasurement": { "humidity": { "value": null } },
            "powerConsumptionReport": { "powerConsumption": { "value": { "power": 1174.0, "energy": 3956114 } } },
            "custom.airConditionerOptionalMode": { "acOptionalMode": { "value": "windFree" } }
        });
        let s = AcStatus::parse(true, &payload);
        assert!(s.power && s.online);
        assert_eq!(s.mode.as_deref(), Some("cool"));
        assert_eq!(s.supported_modes, ["auto", "cool"]);
        assert_eq!((s.temperature, s.setpoint, s.humidity), (Some(28.0), Some(24.0), None));
        assert_eq!((s.power_watts, s.energy_wh), (Some(1174.0), Some(3956114.0)));
        assert_eq!(s.optional_mode.as_deref(), Some("windFree"));
        assert_eq!((s.setpoint_min, s.setpoint_max), (16.0, 30.0));
    }

    #[test]
    fn serializes_commands_for_the_api() {
        let cmd: AcCommand = serde_json::from_value(json!({ "kind": "optionalMode", "value": "windFree" })).unwrap();
        let body = cmd.to_api();
        assert_eq!(body["commands"][0]["capability"], "custom.airConditionerOptionalMode");
        assert_eq!(body["commands"][0]["arguments"], json!(["windFree"]));
        assert_eq!(AcCommand::Power(false).to_api()["commands"][0]["command"], "off");
    }
}
