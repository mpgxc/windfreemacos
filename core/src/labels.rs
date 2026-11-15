//! Portuguese names for SmartThings enum values, shared by the tray menu and the webview.

use std::collections::HashMap;

const LABELS: &[(&str, &str)] = &[
    ("auto", "Automático"),
    ("cool", "Resfriar"),
    ("dry", "Desumidificar"),
    ("wind", "Ventilar"),
    ("heat", "Aquecer"),
    ("low", "Baixa"),
    ("medium", "Média"),
    ("high", "Alta"),
    ("turbo", "Turbo"),
    ("fixed", "Fixo"),
    ("vertical", "Vertical"),
    ("horizontal", "Horizontal"),
    ("all", "Todas as direções"),
    ("off", "Nenhum"),
    ("sleep", "Dormir"),
    ("quiet", "Silencioso"),
    ("smart", "Smart"),
    ("speed", "Rápido"),
    ("windFree", "WindFree"),
    ("windFreeSleep", "WindFree noturno"),
];

pub fn label(value: &str) -> &str {
    LABELS.iter().find(|(key, _)| *key == value).map_or(value, |(_, label)| label)
}

pub fn all() -> HashMap<&'static str, &'static str> {
    LABELS.iter().copied().collect()
}
