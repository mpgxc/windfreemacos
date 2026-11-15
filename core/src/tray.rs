//! Menu bar item: readings as title, a control menu and a fan icon that spins while the AC runs.

use std::{f64::consts::TAU, time::Duration};

use tauri::{
    image::Image,
    menu::{CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder, Submenu, SubmenuBuilder},
    tray::TrayIconBuilder,
    AppHandle, Manager, Wry,
};

use crate::{
    error::Result,
    labels::label,
    settings::TraySettings,
    smartthings::{AcCommand, AcStatus},
    state::{self, lock, AppState, Connection, Snapshot},
};

const TRAY_ID: &str = "main";
/// 22 pt menu bar icon at 2x.
const ICON_SIZE: u32 = 44;
/// 5° per frame over the 120° symmetry period of the three blades.
const FRAMES: usize = 24;
const IDLE_CHECK: Duration = Duration::from_millis(500);

const ID_POWER: &str = "power";
const ID_WARMER: &str = "warmer";
const ID_COOLER: &str = "cooler";
const ID_OPEN: &str = "open";
const ID_SETTINGS: &str = "settings";
const ID_REFRESH: &str = "refresh";
const ID_QUIT: &str = "quit";

pub fn create(app: &AppHandle) -> Result<()> {
    let state = app.state::<AppState>();
    let snapshot = state.snapshot();
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(fan_frame(0.0))
        .icon_as_template(true)
        .title(title(&lock(&state.settings).tray, &snapshot))
        .tooltip("WindFree")
        .menu(&build_menu(app, &snapshot)?)
        .on_menu_event(|app, event| on_menu(app, event.id().as_ref()))
        .build(app)?;
    Ok(())
}

pub fn update(app: &AppHandle, snapshot: &Snapshot, cfg: &TraySettings, rebuild_menu: bool) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let mut result = tray.set_title(Some(title(cfg, snapshot)));
    if rebuild_menu {
        result = result.and_then(|()| tray.set_menu(Some(build_menu(app, snapshot)?)));
    }
    if let Err(e) = result {
        eprintln!("falha ao atualizar a barra de menus: {e}");
    }
}

/// Text next to the icon, e.g. `28°  42%  →24°`.
pub fn title(cfg: &TraySettings, snapshot: &Snapshot) -> String {
    let s = snapshot.status.as_ref();
    let fields = [
        (cfg.show_temperature, "", s.and_then(|s| s.temperature), "°"),
        (cfg.show_humidity, "", s.and_then(|s| s.humidity), "%"),
        (cfg.show_setpoint, "→", s.and_then(|s| s.setpoint), "°"),
    ];
    fields
        .iter()
        .filter(|(shown, ..)| *shown)
        .map(|(_, prefix, value, unit)| match value {
            Some(v) => format!("{prefix}{v:.0}{unit}"),
            None => format!("{prefix}--"),
        })
        .collect::<Vec<_>>()
        .join("  ")
}

fn header(snapshot: &Snapshot) -> String {
    match (snapshot.connection, snapshot.status.as_ref()) {
        (Connection::NoToken, _) => "Configure o token em Ajustes".into(),
        (Connection::NoDevice, _) => "Selecione o aparelho em Ajustes".into(),
        (Connection::Unauthorized, _) => "Token do SmartThings expirado".into(),
        (Connection::Connecting, _) => "Conectando…".into(),
        (Connection::Failed, _) => "Sem conexão com o SmartThings".into(),
        (Connection::Online, Some(s)) if !s.online => "Aparelho offline".into(),
        (Connection::Online, Some(s)) if s.power => {
            let mut parts = vec!["Ligado".to_owned()];
            parts.extend(s.mode.as_deref().map(|m| label(m).to_owned()));
            parts.extend(s.optional_mode.as_deref().filter(|m| *m != "off").map(|m| label(m).to_owned()));
            parts.join(" · ")
        }
        (Connection::Online, _) => "Desligado".into(),
    }
}

fn reading(value: Option<f64>, unit: &str) -> String {
    value.map_or_else(|| "--".into(), |v| format!("{v:.0}{unit}"))
}

fn build_menu(app: &AppHandle, snapshot: &Snapshot) -> tauri::Result<Menu<Wry>> {
    let mut menu = MenuBuilder::new(app).item(&MenuItemBuilder::new(header(snapshot)).enabled(false).build(app)?);
    if let Some(s) = &snapshot.status {
        let readings = format!(
            "Ambiente {}  ·  Umidade {}  ·  Programado {}",
            reading(s.temperature, " °C"),
            reading(s.humidity, "%"),
            reading(s.setpoint, " °C"),
        );
        menu = menu
            .item(&MenuItemBuilder::new(readings).enabled(false).build(app)?)
            .separator()
            .text(ID_POWER, if s.power { "Desligar" } else { "Ligar" })
            .text(ID_WARMER, "Aumentar temperatura  +1 °C")
            .text(ID_COOLER, "Diminuir temperatura  −1 °C")
            .item(&choices(app, "Modo", "mode", &s.supported_modes, s.mode.as_deref())?)
            .item(&choices(app, "Ventilação", "fan", &s.supported_fan_modes, s.fan_mode.as_deref())?)
            .item(&choices(app, "Direção do vento", "swing", &s.supported_oscillations, s.oscillation.as_deref())?)
            .item(&choices(app, "Modo especial", "optional", &s.supported_optional_modes, s.optional_mode.as_deref())?);
    }
    menu.separator()
        .text(ID_OPEN, "Abrir painel…")
        .text(ID_SETTINGS, "Ajustes…")
        .text(ID_REFRESH, "Atualizar agora")
        .separator()
        .text(ID_QUIT, "Sair do WindFree")
        .build()
}

fn choices(app: &AppHandle, title: &str, prefix: &str, options: &[String], current: Option<&str>) -> tauri::Result<Submenu<Wry>> {
    let mut submenu = SubmenuBuilder::new(app, title).enabled(!options.is_empty());
    for option in options {
        let item = CheckMenuItemBuilder::with_id(format!("{prefix}:{option}"), label(option))
            .checked(current == Some(option.as_str()))
            .build(app)?;
        submenu = submenu.item(&item);
    }
    submenu.build()
}

fn on_menu(app: &AppHandle, id: &str) {
    let state = app.state::<AppState>();
    match id {
        ID_OPEN => crate::show_window(app, None),
        ID_SETTINGS => crate::show_window(app, Some("settings")),
        ID_REFRESH => state.request_refresh(),
        ID_QUIT => app.exit(0),
        _ => {
            let Some(command) = menu_command(id, lock(&state.snapshot).status.as_ref()) else { return };
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = state::execute(&app, command).await {
                    state::notify(&app, "Comando não enviado", &e.to_string());
                }
            });
        }
    }
}

fn menu_command(id: &str, status: Option<&AcStatus>) -> Option<AcCommand> {
    let s = status?;
    match id {
        ID_POWER => Some(AcCommand::Power(!s.power)),
        ID_WARMER | ID_COOLER => {
            let delta = if id == ID_WARMER { 1.0 } else { -1.0 };
            Some(AcCommand::Setpoint((s.setpoint? + delta).clamp(s.setpoint_min, s.setpoint_max)))
        }
        _ => {
            let (prefix, value) = id.split_once(':')?;
            let value = value.to_owned();
            match prefix {
                "mode" => Some(AcCommand::Mode(value)),
                "fan" => Some(AcCommand::FanMode(value)),
                "swing" => Some(AcCommand::Oscillation(value)),
                "optional" => Some(AcCommand::OptionalMode(value)),
                _ => None,
            }
        }
    }
}

/// Frame delay matching the fan speed, or `None` when the fan should stand still.
fn spin_interval(animate: bool, s: Option<&AcStatus>) -> Option<Duration> {
    let s = s.filter(|s| animate && s.power && s.online)?;
    let millis = match s.fan_mode.as_deref() {
        Some("low") => 70,
        Some("high") => 33,
        Some("turbo") => 25,
        _ => 45,
    };
    Some(Duration::from_millis(millis))
}

pub async fn animate(app: AppHandle) {
    // Three blades repeat every third of a turn, so the frames only need to cover that.
    let frames: Vec<Image<'static>> =
        (0..FRAMES).map(|i| fan_frame(TAU / 3.0 * i as f64 / FRAMES as f64)).collect();
    let state = app.state::<AppState>();
    let mut frame = 0;
    loop {
        let animate = lock(&state.settings).tray.animate_fan;
        let interval = spin_interval(animate, lock(&state.snapshot).status.as_ref());
        let next = if interval.is_some() { (frame + 1) % FRAMES } else { 0 };
        if next != frame {
            frame = next;
            if let Some(tray) = app.tray_by_id(TRAY_ID) {
                // `set_icon` alone installs a non-template image; a separate template call
                // renders twice per frame and makes the icon blink.
                let result = tray.set_icon_with_as_template(Some(frames[frame].clone()), true);
                if let Err(e) = result {
                    eprintln!("falha ao animar o ícone: {e}");
                }
            }
        }
        tokio::time::sleep(interval.unwrap_or(IDLE_CHECK)).await;
    }
}

/// Renders a template (alpha-only) fan icon rotated by `angle` radians, 4x4 supersampled.
fn fan_frame(angle: f64) -> Image<'static> {
    const SUPERSAMPLE: u32 = 4;
    let center = ICON_SIZE as f64 / 2.0;
    let mut rgba = vec![0u8; (ICON_SIZE * ICON_SIZE * 4) as usize];
    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let mut hits = 0;
            for sy in 0..SUPERSAMPLE {
                for sx in 0..SUPERSAMPLE {
                    let px = (x as f64 + (sx as f64 + 0.5) / SUPERSAMPLE as f64 - center) / center;
                    let py = (y as f64 + (sy as f64 + 0.5) / SUPERSAMPLE as f64 - center) / center;
                    hits += u32::from(fan_covers(px, py, angle));
                }
            }
            rgba[((y * ICON_SIZE + x) * 4 + 3) as usize] = (hits * 255 / (SUPERSAMPLE * SUPERSAMPLE)) as u8;
        }
    }
    Image::new_owned(rgba, ICON_SIZE, ICON_SIZE)
}

/// Fan geometry in unit coordinates: outer ring, hub and three curved blades.
fn fan_covers(x: f64, y: f64, angle: f64) -> bool {
    const BLADE_CURVE: f64 = 1.3;
    let r = x.hypot(y);
    match r {
        r if r > 1.0 => false,
        r if r >= 0.88 => true,
        r if r < 0.2 => true,
        r if r > 0.76 => false,
        r => (3.0 * (y.atan2(x) - angle - BLADE_CURVE * r)).cos() > 0.35,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_follows_settings_and_missing_values() {
        let status = AcStatus { temperature: Some(27.6), humidity: None, setpoint: Some(24.0), ..Default::default() };
        let snapshot = Snapshot { status: Some(status), ..Default::default() };
        let all = TraySettings::default();
        assert_eq!(title(&all, &snapshot), "28°  --  →24°");
        let only_setpoint = TraySettings { show_temperature: false, show_humidity: false, ..all };
        assert_eq!(title(&only_setpoint, &snapshot), "→24°");
    }

    #[test]
    fn setpoint_steps_respect_device_limits() {
        let s = AcStatus { setpoint: Some(30.0), setpoint_min: 16.0, setpoint_max: 30.0, ..Default::default() };
        assert_eq!(menu_command(ID_WARMER, Some(&s)), Some(AcCommand::Setpoint(30.0)));
        assert_eq!(menu_command(ID_COOLER, Some(&s)), Some(AcCommand::Setpoint(29.0)));
        assert_eq!(menu_command("optional:windFree", Some(&s)), Some(AcCommand::OptionalMode("windFree".into())));
        assert_eq!(menu_command("bogus:x", Some(&s)), None);
    }
}
