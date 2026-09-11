//! Globale (projektübergreifende) Einstellungen.
//!
//! Werden als `settings.json` im Datenordner (`import::default_library`)
//! abgelegt und beim Start geladen. Ändern sich die Einstellungen, schreibt
//! `ui::MiniGramps` sie automatisch zurück (siehe `update`).

use std::path::PathBuf;

use crate::import::default_library;
use crate::ui::CardLayout;
use crate::ui::tree::TreeOrientation;

/// Persistente Benutzereinstellungen (projektübergreifend).
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub dark_mode: bool,
    pub max_generations: usize,
    pub group_by_count: bool,
    pub layout_gap: f32,
    pub card_layout: CardLayout,
    pub tree_orientation: TreeOrientation,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            dark_mode: true,
            max_generations: 5,
            group_by_count: true,
            layout_gap: 48.0,
            card_layout: CardLayout::Compact,
            tree_orientation: TreeOrientation::Vertical,
        }
    }
}

/// Pfad der globalen Einstellungsdatei.
pub fn path() -> PathBuf {
    default_library().join("settings.json")
}

/// Einstellungen laden (fehlende/kaputte Datei → Standardwerte).
pub fn load() -> AppSettings {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Einstellungen schreiben (Fehler werden bewusst ignoriert).
pub fn save(settings: &AppSettings) {
    if let Ok(text) = serde_json::to_string_pretty(settings) {
        let _ = std::fs::write(path(), text);
    }
}
