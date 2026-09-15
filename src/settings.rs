//! Globale (projektübergreifende) Einstellungen.
//!
//! Werden als `settings.json` im Datenordner (`import::default_library`)
//! abgelegt und beim Start geladen. Ändern sich die Einstellungen, schreibt
//! `ui::MiniGramps` sie automatisch zurück (siehe `update`).

use std::path::PathBuf;

use crate::import::default_library;
use crate::model::Certainty;
use crate::ui::CardLayout;
use crate::ui::tree::TreeOrientation;

/// Persistente Benutzereinstellungen (projektübergreifend).
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub dark_mode: bool,
    pub max_generations: usize,
    pub tree_initial_person_limit: usize,
    pub tree_load_step: usize,
    /// Treffer-Schwelle für den Duplikat-Abgleich beim Anhängen (0–100 %).
    pub match_threshold: f32,
    /// Warnstufe Sicherheit: Infos bis zu dieser Stufe werden in Baum und
    /// Seitenleiste farblich hervorgehoben (Handlungsbedarf). None = Aus.
    pub warn_certainty: Option<Certainty>,
    /// Zoom-Faktor fürs Umschalten auf die Ganzfoto-Ansicht bei Personen
    /// MIT Foto (darunter Ganzfoto, darüber Avatar+Text).
    #[serde(default = "default_full_zoom")]
    pub photo_full_zoom: f32,
    /// Dasselbe für Personen OHNE Foto (Initialen-Großansicht).
    #[serde(default = "default_full_zoom")]
    pub initials_full_zoom: f32,
    /// Häufige Vornamen ab so vielen Gleichnamigen je Nachnamengruppe geben
    /// keinen Exakt-Boost (einstellbar im Prüfdialog, 2–10, Default 4).
    #[serde(default = "default_common_given_threshold")]
    pub common_given_threshold: usize,
    pub group_by_count: bool,
    pub layout_gap: f32,
    /// Extra-Abstand zwischen Nachbarkarten ohne Partner-Verbindung im
    /// Vorfahrenbaum (0 = kein Extra, nur Baum-Abstand).
    #[serde(default = "default_non_partner_gap")]
    pub non_partner_gap: f32,
    pub card_layout: CardLayout,
    pub compact_card_width: f32,
    pub portrait_card_width: f32,
    /// Symbole vor Geburts-/Todesdatum auf den Baumkarten (Elhaz-Runen).
    /// Eigene Defaults, damit alte Dateien ohne diese Schlüssel sie erhalten
    /// (bewusst leer gelöscht bleibt leer).
    #[serde(default = "default_birth_symbol")]
    pub birth_symbol: String,
    #[serde(default = "default_death_symbol")]
    pub death_symbol: String,
    pub tree_orientation: TreeOrientation,
    pub window_x: Option<f32>,
    pub window_y: Option<f32>,
    pub window_width: Option<f32>,
    pub window_height: Option<f32>,
    pub window_maximized: Option<bool>,
}

fn default_common_given_threshold() -> usize {
    4
}

fn default_full_zoom() -> f32 {
    0.8
}

fn default_non_partner_gap() -> f32 {
    crate::ui::tree::UNMARRIED_GAP_EXTRA
}

fn default_birth_symbol() -> String {
    "ᛉ".into()
}

fn default_death_symbol() -> String {
    "ᛦ".into()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            dark_mode: true,
            max_generations: 5,
            tree_initial_person_limit: 60,
            tree_load_step: 60,
            match_threshold: 80.0,
            warn_certainty: None,
            photo_full_zoom: default_full_zoom(),
            initials_full_zoom: default_full_zoom(),
            common_given_threshold: default_common_given_threshold(),
            group_by_count: true,
            layout_gap: 48.0,
            non_partner_gap: crate::ui::tree::UNMARRIED_GAP_EXTRA,
            card_layout: CardLayout::Compact,
            compact_card_width: 215.0,
            portrait_card_width: 160.0,
            birth_symbol: "ᛉ".into(),
            death_symbol: "ᛦ".into(),
            tree_orientation: TreeOrientation::Vertical,
            window_x: None,
            window_y: None,
            window_width: None,
            window_height: None,
            window_maximized: None,
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
