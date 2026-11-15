//! Shared app state and the polling loop that feeds the tray, the webview, metrics and alerts.

use std::{
    path::PathBuf,
    sync::{Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::Notify;

use crate::{
    alerts::AlertTracker,
    error::{Error, Result},
    metrics::{now, MetricsStore, Sample},
    settings::{self, Settings},
    smartthings::{AcCommand, AcStatus, Client},
    tray,
};

pub const SNAPSHOT_EVENT: &str = "snapshot";
/// Time for an accepted command to show up in the cloud status.
const COMMAND_SETTLE: Duration = Duration::from_millis(2500);

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Connection {
    #[default]
    Connecting,
    Online,
    NoToken,
    NoDevice,
    Unauthorized,
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub connection: Connection,
    /// Last known status; kept across transient failures.
    pub status: Option<AcStatus>,
    pub error: Option<String>,
    pub updated_at: Option<u64>,
}

pub struct AppState {
    settings_path: PathBuf,
    pub(crate) settings: Mutex<Settings>,
    pub(crate) snapshot: Mutex<Snapshot>,
    pub(crate) metrics: Mutex<MetricsStore>,
    client: Mutex<Option<Client>>,
    alerts: Mutex<AlertTracker>,
    refresh: Notify,
}

/// A panicked holder cannot leave these plain-data values half-written in a harmful way.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl AppState {
    pub fn init(app: &AppHandle) -> Result<Self> {
        let settings_path = app.path().app_config_dir()?.join("settings.json");
        let settings = Settings::load(&settings_path);
        let metrics = MetricsStore::open(app.path().app_data_dir()?.join("metrics.jsonl"), settings.retention_days)?;
        let token = settings::token::load().unwrap_or_else(|e| {
            eprintln!("{e}");
            None
        });
        Ok(Self {
            settings_path,
            settings: Mutex::new(settings),
            snapshot: Mutex::default(),
            metrics: Mutex::new(metrics),
            client: Mutex::new(token.map(|t| Client::new(&t)).transpose()?),
            alerts: Mutex::default(),
            refresh: Notify::new(),
        })
    }

    pub fn settings(&self) -> Settings {
        lock(&self.settings).clone()
    }

    pub fn snapshot(&self) -> Snapshot {
        lock(&self.snapshot).clone()
    }

    pub fn needs_setup(&self) -> bool {
        lock(&self.client).is_none() || lock(&self.settings).device_id.is_none()
    }

    pub fn client(&self) -> Result<Client> {
        lock(&self.client).clone().ok_or(Error::MissingToken)
    }

    pub fn set_client(&self, client: Client) {
        *lock(&self.client) = Some(client);
    }

    pub fn save_settings(&self, settings: Settings) -> Result<Settings> {
        let settings = settings.sanitized();
        settings.save(&self.settings_path)?;
        lock(&self.metrics).set_retention_days(settings.retention_days);
        *lock(&self.settings) = settings.clone();
        self.request_refresh();
        Ok(settings)
    }

    pub fn request_refresh(&self) {
        self.refresh.notify_one();
    }

    async fn fetch(&self) -> Snapshot {
        let client = lock(&self.client).clone();
        let device = lock(&self.settings).device_id.clone();
        let (client, device) = match (client, device) {
            (None, _) => return Snapshot { connection: Connection::NoToken, ..Snapshot::default() },
            (_, None) => return Snapshot { connection: Connection::NoDevice, ..Snapshot::default() },
            (Some(client), Some(device)) => (client, device),
        };
        match client.status(&device).await {
            Ok(status) => Snapshot {
                connection: Connection::Online,
                status: Some(status),
                error: None,
                updated_at: Some(now()),
            },
            Err(e) => Snapshot {
                connection: if matches!(e, Error::Unauthorized) { Connection::Unauthorized } else { Connection::Failed },
                error: Some(e.to_string()),
                ..self.snapshot()
            },
        }
    }
}

pub async fn poll(app: AppHandle) {
    let state = app.state::<AppState>();
    loop {
        let snapshot = state.fetch().await;
        record(&app, &state, &snapshot);
        broadcast(&app, &state, snapshot);
        let interval = Duration::from_secs(lock(&state.settings).poll_interval_secs);
        tokio::select! {
            () = tokio::time::sleep(interval) => {}
            () = state.refresh.notified() => {}
        }
    }
}

/// Stores a metrics sample and raises alerts for a freshly fetched snapshot.
fn record(app: &AppHandle, state: &AppState, snapshot: &Snapshot) {
    let previous = lock(&state.snapshot).connection;
    if snapshot.connection == Connection::Unauthorized && previous != Connection::Unauthorized {
        notify(app, "Token do SmartThings expirado", "Gere um novo token em Ajustes para continuar controlando o ar.");
    }
    let Some(status) = snapshot.status.as_ref().filter(|_| snapshot.connection == Connection::Online) else {
        return;
    };
    if let Err(e) = lock(&state.metrics).push(Sample::from_status(now(), status)) {
        eprintln!("falha ao gravar métrica: {e}");
    }
    let alerts = lock(&state.settings).alerts.clone();
    for notice in lock(&state.alerts).evaluate(&alerts, status) {
        notify(app, &notice.title, &notice.body);
    }
}

/// Publishes a snapshot to the webview and the tray.
fn broadcast(app: &AppHandle, state: &AppState, snapshot: Snapshot) {
    let previous = std::mem::replace(&mut *lock(&state.snapshot), snapshot.clone());
    if let Err(e) = app.emit(SNAPSHOT_EVENT, &snapshot) {
        eprintln!("falha ao notificar a janela: {e}");
    }
    let menu_changed = previous.connection != snapshot.connection || previous.status != snapshot.status;
    tray::update(app, &snapshot, &lock(&state.settings).tray, menu_changed);
}

/// Sends a command, shows its effect immediately and re-polls once the device settles.
pub async fn execute(app: &AppHandle, command: AcCommand) -> Result<()> {
    let state = app.state::<AppState>();
    let client = state.client()?;
    let device = lock(&state.settings).device_id.clone().ok_or(Error::MissingDevice)?;
    client.execute(&device, &command).await?;

    let mut snapshot = state.snapshot();
    if let Some(status) = snapshot.status.as_mut() {
        status.apply(&command);
        broadcast(app, &state, snapshot);
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(COMMAND_SETTLE).await;
        app.state::<AppState>().request_refresh();
    });
    Ok(())
}

pub fn notify(app: &AppHandle, title: &str, body: &str) {
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        eprintln!("falha ao exibir notificação: {e}");
    }
}
