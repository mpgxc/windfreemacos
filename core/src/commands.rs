//! Tauri commands invoked by the webview.

use std::{collections::HashMap, process::Command};

use tauri::{AppHandle, State};

use crate::{
    error::Result,
    labels,
    metrics::{now, History},
    settings::{self, Settings},
    smartthings::{AcCommand, Client, Device},
    state::{self, lock, AppState, Snapshot},
    tray,
};

const TOKEN_PAGE: &str = "https://account.smartthings.com/tokens";
const CHART_POINTS: usize = 240;

#[tauri::command]
pub fn get_snapshot(state: State<AppState>) -> Snapshot {
    state.snapshot()
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Settings {
    state.settings()
}

#[tauri::command]
pub fn save_settings(app: AppHandle, state: State<AppState>, settings: Settings) -> Result<Settings> {
    let saved = state.save_settings(settings)?;
    tray::update(&app, &state.snapshot(), &saved.tray, false);
    crate::apply_appearance(&app, saved.appearance);
    Ok(saved)
}

/// Validates the token against the API before storing it in the Keychain.
#[tauri::command]
pub async fn save_token(state: State<'_, AppState>, token: String) -> Result<Vec<Device>> {
    let client = Client::new(&token)?;
    let devices = client.devices().await?;
    settings::token::store(token.trim())?;
    state.set_client(client);

    let mut current = state.settings();
    let air_conditioners: Vec<&Device> = devices.iter().filter(|d| d.is_air_conditioner).collect();
    if let ([only], None) = (air_conditioners.as_slice(), &current.device_id) {
        current.device_id = Some(only.id.clone());
        current.device_label = Some(only.label.clone());
    }
    state.save_settings(current)?;
    Ok(devices)
}

#[tauri::command]
pub async fn list_devices(state: State<'_, AppState>) -> Result<Vec<Device>> {
    state.client()?.devices().await
}

#[tauri::command]
pub async fn send_command(app: AppHandle, command: AcCommand) -> Result<()> {
    state::execute(&app, command).await
}

#[tauri::command]
pub fn get_history(state: State<AppState>, range_secs: u64) -> History {
    lock(&state.metrics).history(now().saturating_sub(range_secs), CHART_POINTS)
}

#[tauri::command]
pub fn get_labels() -> HashMap<&'static str, &'static str> {
    labels::all()
}

#[tauri::command]
pub fn open_token_page() -> Result<()> {
    Command::new("open").arg(TOKEN_PAGE).status()?;
    Ok(())
}
