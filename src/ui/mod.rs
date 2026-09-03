//! Oberfläche: App-Zustand, dünnes Panel-Gerüst, Icons, Schriften.
//!
//! Verdrahtung:
//! - `MiniGramps` hält den gesamten Zustand. Zentrale Felder und ihre
//!   Verbraucher:
//!     * `data`        → `crate::model::TreeData` (Speichern/Laden/Anzeige),
//!     * `reference`   → Wurzel des Graphen (`tree::draw_tree`),
//!     * `selected`    → angezeigte Person (`sidebar::show_right`),
//!     * `expanded`/`max_generations`/`long_press_used` →
//!       `tree::draw_tree`-Parameter (Ausklappen, Generationenlimit, Touch),
//!     * `photo_cache` → `crate::media::photo_texture`,
//!     * `library`     → Speicherort (`save`) und Medien-Basisordner
//!       (`crate::media::import_media_file`, `photo_texture`).
//! - Aktionen der Baumkarten (`tree::TreeAction`) werden unten im zentralen
//!   Panel ausgewertet: Klick = öffnen, Shift/Doppelklick/langer Touch =
//!   Referenzperson, `+`/`−` = Generationen aus-/einklappen.
//! - Die Panels leben in eigenen Modulen: `header`, `sidebar`, `panels`
//!   (Farbwelten + Debug-Leiste), `dialogs`, `picker` (Beziehungspicker und
//!   Profil-Widgets), `tree` (Stammbaum).
//! - Laden: `crate::import::load_file` / `discover_projects`. Fotos:
//!   `crate::media::import_media_file` (relativer Pfad nach `library/media`,
//!   Gramps-Prinzip).

pub mod dialogs;
pub mod header;
pub mod panels;
pub mod picker;
pub mod sidebar;
pub mod tree;

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use eframe::{
    egui,
    egui::{Color32, TextureHandle, Vec2},
};
use rfd::FileDialog;

use crate::import::{
    default_library, load_file, load_last_project, load_project_manifest, save_last_project,
    save_project_manifest,
};
use crate::model::{ChildRelation, Gender, Person, TreeData, person};
use tree::{RelationKind, TreeAction, TreeOrientation, TreeView};

// Eingebettete Icons (Feather-SVGs) und Logo.
pub(crate) const ICON_OPEN: &[u8] = include_bytes!("../../assets/icons/folder.svg");
pub(crate) const ICON_SAVE: &[u8] = include_bytes!("../../assets/icons/save.svg");
pub(crate) const ICON_SETTINGS: &[u8] = include_bytes!("../../assets/icons/settings.svg");
pub(crate) const ICON_MINIMIZE: &[u8] = include_bytes!("../../assets/icons/minus.svg");
pub(crate) const ICON_MAXIMIZE: &[u8] = include_bytes!("../../assets/icons/square.svg");
pub(crate) const ICON_CLOSE: &[u8] = include_bytes!("../../assets/icons/x.svg");
pub(crate) const ICON_EXPORT: &[u8] = include_bytes!("../../assets/icons/upload.svg");
pub(crate) const ICON_ADD_PERSON: &[u8] = include_bytes!("../../assets/icons/user-plus.svg");
pub(crate) const ICON_PARTNER: &[u8] = include_bytes!("../../assets/icons/heart.svg");
pub(crate) const ICON_PARENT: &[u8] = include_bytes!("../../assets/icons/arrow-up.svg");
pub(crate) const ICON_SIBLING: &[u8] = include_bytes!("../../assets/icons/git-branch.svg");
pub(crate) const ICON_CHILD: &[u8] = include_bytes!("../../assets/icons/arrow-down.svg");
pub(crate) const ICON_EDIT: &[u8] = include_bytes!("../../assets/icons/edit-3.svg");
pub(crate) const ICON_REFERENCE: &[u8] = include_bytes!("../../assets/icons/star.svg");
pub(crate) const LOGO: &[u8] = include_bytes!("../../assets/icon.svg");

pub struct MiniGramps {
    pub data: TreeData,
    /// Angezeigte Person (rechte Seitenleiste). Nur Ansicht, kein Baum-Effekt.
    pub selected: Option<String>,
    /// Referenzperson = Wurzel des Stammbaums (`tree::draw_tree`).
    pub reference: Option<String>,
    /// Personen, deren weitere Generationen ausgeklappt sind.
    pub expanded: HashSet<String>,
    /// Manuelle Verschiebungen entlang der Verteilungsachse
    /// (Shift+Ziehen auf eine Karte; Kinder/Partner folgen).
    pub manual_offsets: HashMap<String, f32>,
    /// Debounce für Lang-Touch (`tree::draw_tree`), Reset im CentralPanel.
    pub long_press_used: bool,
    /// Aktiver Karten-Drag (Shift+Ziehen): Ziel-Person + zu verschiebende
    /// Menge. Gilt bis zum Loslassen – unabhängig davon, wo der Druck
    /// ursprünglich startede (sonst bricht der Drag am Kartenrand ab).
    pub card_drag: Option<(String, Vec<String>)>,
    /// Standard-Generationenzahl (Einstellungen; 0 = alle).
    pub max_generations: usize,
    /// Datenordner: Speicherort (`save`) und Medien-Basisordner (`media`).
    pub library: PathBuf,
    pub status: String,
    pub zoom: f32,
    /// Zoom-Fit beim nächsten Frame ausführen (nach Laden/Referenzwechsel).
    pub fit_pending: bool,
    pub pan: Vec2,
    pub show_editor: bool,
    pub show_open: bool,
    pub show_settings: bool,
    /// Projekteigenschaften hinter dem Logo in der Titelleiste.
    pub show_project: bool,
    /// Auswahl der kuenftigen Exportformate, geoeffnet aus dem Projektfenster.
    pub show_export: bool,
    /// Sortierung der Personenliste: nach Anzahl (true) oder Alphabet.
    pub group_by_count: bool,
    /// Textur-Cache (`media::photo_texture`), Schlüssel = `Person::id`
    /// bzw. Galerie-Pseudo-IDs `gallery-<id>-<index>`.
    pub photo_cache: HashMap<String, TextureHandle>,
    pub tree_view: TreeView,
    /// Im Öffnen-Dialog ausgewähltes Projekt (wird mit "Laden" geöffnet).
    pub selected_project: Option<PathBuf>,
    pub dark_mode: bool,
    pub tree_orientation: TreeOrientation,
    /// ID der im Modal-Editor geöffneten Person (None = neue Person).
    pub editing: Option<String>,
    /// Arbeitskopie für Inline- und Modal-Bearbeitung.
    pub draft: Person,
    /// Inline-Bearbeitung im Profil (Stift/Diskette in der Leiste).
    pub inline_edit: bool,
    /// Suchtext des Beziehungspickers.
    pub relation_query: String,
    /// Per Drag-and-drop eingefügte Datei mit unklarer Verwendung.
    pub pending_image: Option<PathBuf>,
    /// Zustand des `+ KIND`-Pickers: Bezugsperson, gewählter Partner, Art.
    pub pending_child_for: Option<String>,
    pub pending_child_partner: Option<String>,
    pub pending_child_relation: ChildRelation,
    /// Offene Beziehungskategorie (`+`-Schalter, nur im Bearbeitungsmodus).
    pub relation_picker: Option<RelationKind>,
    /// Aufgeklappte Beziehungszeile im Beziehungseditor: Kategorie + ID der
    /// Verwandten, deren Beziehungsart bearbeitet wird.
    pub relation_editor: Option<(RelationKind, String)>,
    /// EINGEKLAPpte Kategorien der rechten Leiste (Sitzungszustand).
    pub collapsed_sections: HashSet<String>,
    /// Ausgeblendete Kategorien (Rechtsklick auf Kategorietitel → Häkchen).
    pub hidden_sections: HashSet<String>,
    /// Erweiterte Namensfelder im Profil aktiv (Rechtsklick auf Namensfeld).
    pub name_details: bool,
    /// Offenes Kontextmenü: Schlüssel + Position (Kategorien oder Namen).
    pub section_menu: Option<(String, Vec2, egui::Pos2)>,
    /// 1 = Kategorie-Menü, 2 = Namens-Menü (Schlüssel-Semantik).
    pub section_menu_kind: u8,
    /// Debug-Log (Leiste unten + Terminal via `log`).
    /// Pfad der aktuell geöffneten Projektdatei (für `<stem>.layout.json`).
    pub current_data_path: Option<PathBuf>,
    started: std::time::Instant,
}

impl MiniGramps {
    pub fn new() -> Self {
        let library = default_library();
        let _ = fs::create_dir_all(&library);
        let mut app = Self {
            data: TreeData::demo(),
            selected: Some("p5".into()),
            reference: Some("p5".into()),
            expanded: HashSet::new(),
            manual_offsets: HashMap::new(),
            long_press_used: false,
            card_drag: None,
            max_generations: 5,
            library,
            status: "Beispielbaum geladen".into(),
            zoom: 1.0,
            fit_pending: false,
            pan: Vec2::ZERO,
            show_editor: false,
            show_open: false,
            show_settings: false,
            show_project: false,
            show_export: false,
            group_by_count: true,
            photo_cache: HashMap::new(),
            tree_view: TreeView::Descendants,
            selected_project: None,
            dark_mode: true,
            tree_orientation: TreeOrientation::Vertical,
            editing: None,
            draft: person("", "", "", "", Gender::Unknown),
            inline_edit: false,
            relation_query: String::new(),
            pending_image: None,
            pending_child_for: None,
            pending_child_partner: None,
            pending_child_relation: ChildRelation::Birth,
            relation_picker: None,
            relation_editor: None,
            collapsed_sections: HashSet::new(),
            hidden_sections: HashSet::new(),
            name_details: false,
            section_menu: None,
            section_menu_kind: 0,
            current_data_path: None,
            started: std::time::Instant::now(),
        };
        app.log(format!(
            "Start. Datenordner (Speicherort): {}",
            app.library.display()
        ));
        // Zuletzt geöffnetes Projekt automatisch wiederherstellen; schlägt
        // das fehlen (Datei weg, Format unbekannt), bleibt der Beispielbaum.
        if let Some(path) = load_last_project() {
            app.log(format!(
                "Letzte Sitzung wiederherstellen: {}",
                path.display()
            ));
            app.load_path(&path);
        }
        app
    }

    /// Log: Terminal (`println!`) + Debug-Leiste (`panels::show_debug`).
    pub fn log(&mut self, message: impl Into<String>) {
        let line = format!(
            "[{:>9.3}s] {}",
            self.started.elapsed().as_secs_f32(),
            message.into()
        );
        println!("{line}");
    }

    /// Projekt in den Datenordner schreiben (`<library>/familienbaum…json`).
    /// Zusätzlich wird das Baum-Layout in einer SEPARATEN Datei
    /// (`<projekt>.layout.json`) neben den Daten gespeichert: manuelle
    /// Verschiebungen, über IDs zugeordnet, relativ zum Eltern-Anker —
    /// angewendet in horizontaler wie vertikaler Ausrichtung.
    pub fn save(&mut self) {
        let path = self.library.join("familienbaum.minigramps.json");
        self.current_data_path = Some(path.clone());
        match serde_json::to_string_pretty(&self.data)
            .and_then(|s| fs::write(&path, s).map_err(serde_json::Error::io))
        {
            Ok(_) => {
                self.status = format!("Gespeichert: {}", path.display());
                self.log(format!("Gespeichert: {}", path.display()));
            }
            Err(e) => {
                self.status = format!("Speichern fehlgeschlagen: {e}");
                self.log(format!("Speichern fehlgeschlagen: {e}"));
            }
        }
        match save_project_manifest(&path, &self.data) {
            Ok(manifest) => self.log(format!("Manifest gespeichert: {}", manifest.display())),
            Err(error) => self.log(format!("Manifest speichern fehlgeschlagen: {error}")),
        }
        let layout = LayoutFile {
            version: LAYOUT_VERSION,
            entries: self
                .manual_offsets
                .iter()
                .map(|(id, offset)| LayoutEntry {
                    id: id.clone(),
                    offset: *offset,
                })
                .collect(),
        };
        let layout_file = layout_path(&path);
        match serde_json::to_string_pretty(&layout)
            .and_then(|text| fs::write(&layout_file, text).map_err(serde_json::Error::io))
        {
            Ok(_) => self.log(format!(
                "Layout gespeichert: {} ({} Einträge)",
                layout_file.display(),
                layout.entries.len()
            )),
            Err(e) => self.log(format!("Layout speichern fehlgeschlagen: {e}")),
        }
    }

    /// Manueller Dateidialog ("Datei manuell laden..." im Öffnen-Fenster).
    pub fn import_dialog(&mut self) {
        if let Some(path) = FileDialog::new()
            .add_filter(
                "Familien-Daten",
                &["json", "ged", "gedcom", "gramps", "xml"],
            )
            .pick_file()
        {
            self.log(format!("Manueller Ladeversuch: {}", path.display()));
            self.load_path(&path);
        }
    }

    /// Datei laden (`crate::import::load_file`) und Ansichtszustand zurücksetzen.
    /// Das Baum-Layout wird aus der separaten `.layout.json` geladen (falls
    /// vorhanden) und in beiden Ausrichtungen angewendet.
    pub fn load_path(&mut self, path: &Path) {
        self.log(format!("Lade: {}", path.display()));
        match load_file(path) {
            Ok(data) => {
                self.selected = data.people.first().map(|p| p.id.clone());
                self.reference = self.selected.clone();
                self.expanded.clear();
                self.log(format!(
                    "Geladen: {} Personen, {} Familien aus {}",
                    data.people.len(),
                    data.families.len(),
                    path.display()
                ));
                self.data = data;
                if let Some(manifest) = load_project_manifest(path) {
                    self.data.project.name = manifest.name;
                    self.data.project.format_version = manifest.format_version;
                }
                if self.data.project.name == "Unbenanntes Projekt" {
                    self.data.project.name = path
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .unwrap_or("Unbenanntes Projekt")
                        .to_string();
                }
                self.photo_cache.clear();
                self.manual_offsets = load_layout(path);
                self.current_data_path = Some(path.to_path_buf());
                if !self.manual_offsets.is_empty() {
                    self.log(format!(
                        "Layout geladen: {} Verschiebungen",
                        self.manual_offsets.len()
                    ));
                }
                // Nach dem Laden den ganzen Baum passend einpassen.
                self.fit_pending = true;
                // Projekt als "zuletzt geöffnet" merken (Start-Wiederherstellung).
                save_last_project(path);
                self.show_open = false;
                self.status = format!("Geöffnet: {}", path.display());
            }
            Err(e) => {
                self.status = format!("Laden fehlgeschlagen: {e}");
                self.log(format!("Laden fehlgeschlagen {}: {e}", path.display()));
            }
        }
    }

    /// Person als Referenz setzen (Baum-Wurzel). Genutzt von der
    /// TreeAction-Auswertung, den Listen (Shift/Dreifachklick) und dem
    /// Button "Als Referenz setzen" (sidebar.rs). Das Layout wird live
    /// gesichert, damit Verschiebungen über Referenzwechsel erhalten bleiben.
    pub fn set_reference(&mut self, id: &str) {
        let name = self
            .data
            .find(id)
            .map(|p| p.display_name())
            .unwrap_or_default();
        self.reference = Some(id.to_string());
        self.selected = Some(id.to_string());
        // Neue Referenz sofort mittig: Das Layout wird relativ zur
        // Referenz aufgebaut, der Schwenk wird auf null zurückgesetzt
        // (Zoom bleibt erhalten).
        self.pan = Vec2::ZERO;
        // … und der ganze Baum wird passend eingepasst.
        self.fit_pending = true;
        self.persist_layout();
        self.status = format!("Referenzperson: {name}");
        self.log(format!("Referenzperson gesetzt: {name}"));
    }

    /// Layout (manuelle Offsets) sofort in die `.layout.json` schreiben.
    fn persist_layout(&mut self) {
        let data_path = self
            .current_data_path
            .clone()
            .unwrap_or_else(|| self.library.join("familienbaum.minigramps.json"));
        let layout = LayoutFile {
            version: LAYOUT_VERSION,
            entries: self
                .manual_offsets
                .iter()
                .map(|(id, offset)| LayoutEntry {
                    id: id.clone(),
                    offset: *offset,
                })
                .collect(),
        };
        let file = layout_path(&data_path);
        match serde_json::to_string_pretty(&layout)
            .and_then(|text| fs::write(&file, text).map_err(serde_json::Error::io))
        {
            Ok(_) => self.log(format!(
                "Layout gesichert: {} ({} Einträge)",
                file.display(),
                layout.entries.len()
            )),
            Err(e) => self.log(format!("Layout sichern fehlgeschlagen: {e}")),
        }
    }
}

impl eframe::App for MiniGramps {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        let mut style = (*ctx.style()).clone();
        style.visuals = if self.dark_mode {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        if self.dark_mode {
            style.visuals.panel_fill = Color32::from_rgb(19, 27, 35);
            style.visuals.window_fill = Color32::from_rgb(27, 38, 49);
            // Grautöne etwas heller als der egui-Standard: Fließtext/Labels
            // (noninteractive, Standard 140) und Button-Beschriftung
            // (inactive, Standard 180).
            style.visuals.widgets.noninteractive.fg_stroke.color = Color32::from_gray(192);
            style.visuals.widgets.inactive.fg_stroke.color = Color32::from_gray(216);
        }
        ctx.set_style(style);

        // Panels in eigenen Modulen; Reihenfolge bestimmt das Layout.
        header::show(self, ctx);
        sidebar::show_left(self, ctx);
        sidebar::show_right(self, ctx);

        // Zentrales Panel: Stammbaum-Werkzeugleiste + Zeichenfläche.
        let canvas_color = panels::palette(self.dark_mode).canvas;
        let section_accent = panels::palette(self.dark_mode).section;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(canvas_color).inner_margin(1))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    ui.label(
                        egui::RichText::new("STAMMBAUM")
                            .size(15.0)
                            .strong()
                            .color(section_accent),
                    );
                    ui.separator();
                // Manuelle Offsets gelten in BEIDEN Ausrichtungen (Layout-
                // Datei) und bleiben beim Ansichtwechsel erhalten.
                ui.selectable_value(
                    &mut self.tree_view,
                    TreeView::Descendants,
                    egui::RichText::new("Nachfahrenbaum").size(11.0),
                );
                ui.selectable_value(
                    &mut self.tree_view,
                    TreeView::Ancestors,
                    egui::RichText::new("Vorfahrenbaum").size(11.0),
                );
                ui.selectable_value(
                    &mut self.tree_view,
                    TreeView::Fan,
                    egui::RichText::new("Ahnenfächer").size(11.0),
                );
                ui.separator();
                ui.selectable_value(
                    &mut self.tree_orientation,
                    TreeOrientation::Vertical,
                    egui::RichText::new("Vertikal").size(11.0),
                );
                ui.selectable_value(
                    &mut self.tree_orientation,
                    TreeOrientation::Horizontal,
                    egui::RichText::new("Horizontal").size(11.0),
                );
                ui.separator();
                    // Hinweis nur, wenn genug Platz (sonst automatisch aus).
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(8.0);
                        let hint =
                            "Mausrad: Zoom · Ziehen: Verschieben · Klick: Person · Shift+Klick: Referenz · Shift+Ziehen: Karte · Rahmen ziehen: Zweig";
                        let hint_width = ui
                            .painter()
                            .layout_no_wrap(
                                hint.to_owned(),
                                ui.style()
                                    .text_styles
                                    .get(&egui::TextStyle::Body)
                                    .cloned()
                                    .unwrap_or_default(),
                                Color32::WHITE,
                            )
                            .size()
                            .x;
                        if ui.available_width() >= hint_width + 24.0 {
                            ui.label(hint);
                        }
                    });
                });
                ui.add_space(14.0);
                let available = ui.available_size();
                let (response, painter) = ui.allocate_painter(available, egui::Sense::drag());
                // Dezentes Marken-Wasserzeichen unter allen Verbindungen und
                // Karten. Bei schmalen Flaechen wird es zusaetzlich begrenzt,
                // damit das quadratische Logo nicht abgeschnitten wird.
                let watermark_side = (response.rect.height() * 0.7)
                    .min(response.rect.width() * 0.9)
                    .max(1.0);
                let watermark_rect = egui::Rect::from_center_size(
                    response.rect.center(),
                    Vec2::splat(watermark_side),
                );
                egui::Image::from_bytes("bytes://minigramps-watermark.svg", LOGO)
                    .fit_to_exact_size(Vec2::splat(watermark_side))
                    .tint(if self.dark_mode {
                        Color32::from_black_alpha(77)
                    } else {
                        // Im Hellmodus dezenter (15 %).
                        Color32::from_black_alpha(38)
                    })
                    .paint_at(ui, watermark_rect);
                let scroll = ui.input(|i| i.raw_scroll_delta.y);
                if response.hovered() && scroll != 0.0 {
                    self.zoom = (self.zoom * (1.0 + scroll * 0.001)).clamp(0.15, 1.8);
                }
                // Lang-Touch-Debouncer und aktiven Karten-Drag zurücksetzen,
                // wenn nichts gedrückt ist. Drag-Ende → Layout live sichern.
                if !ui.input(|i| i.pointer.primary_down()) {
                    self.long_press_used = false;
                    if self.card_drag.take().is_some() {
                        self.persist_layout();
                    }
                }
                // Baum zeichnen und Klick-Aktionen auswerten (siehe tree.rs).
                let mut action: Option<TreeAction> = None;
                let viewed = self.selected.clone();
                let reference = self.reference.clone();
                    let expanded = self.expanded.clone();
                    let mut long_press_used = self.long_press_used;
                    let mut manual_offsets = self.manual_offsets.clone();
                    let mut card_drag = self.card_drag.take();
                    let mut frame_drag = false;
                    let content_bounds = tree::draw_tree(
                        &painter,
                        response.rect,
                        &self.data,
                        reference.as_deref(),
                        viewed.as_deref(),
                        &mut action,
                        &expanded,
                        &mut long_press_used,
                        &mut card_drag,
                        &mut frame_drag,
                        self.max_generations,
                        &mut manual_offsets,
                        &self.library,
                        &mut self.photo_cache,
                        self.tree_view,
                        self.tree_orientation,
                        self.zoom,
                        self.pan,
                    );
                    self.long_press_used = long_press_used;
                    self.manual_offsets = manual_offsets;
                    self.card_drag = card_drag;
                // Zoom-Fit: den gelieferten Inhaltsbereich (Layout-Koordo-
                // dinaten) passend in die Zeichenfläche skalieren und mittig
                // setzen — einmalig nach Laden/Referenzwechsel.
                if self.fit_pending {
                    self.fit_pending = false;
                    let canvas = response.rect.size();
                    let size = content_bounds.size();
                    if size.x > 1.0 && size.y > 1.0 {
                        let fit = ((canvas.x / size.x).min(canvas.y / size.y) * 0.92)
                            .clamp(0.15, 1.8);
                        self.zoom = fit;
                        // Baum-Mitte auf Canvas-Mitte: gezeichnet wird an
                        // center + pan + pos*zoom → pan = -mitte*zoom.
                        self.pan = content_bounds.center().to_vec2() * -fit;
                    }
                }
                // Canvas-Pan NACH dem Zeichnen auswerten: Ein Drag innerhalb
                // eines Paarrahmens verschiebt den Zweig (frame_drag) und
                // darf die Zeichenfläche nicht mitschieben.
                if response.dragged() && !ui.input(|i| i.modifiers.shift) && !frame_drag {
                    self.pan += response.drag_delta();
                }
                match action {
                    Some(TreeAction::View(id)) => self.selected = Some(id),
                    Some(TreeAction::Reference(id)) => self.set_reference(&id),
                    Some(TreeAction::ToggleExpand(id)) => {
                        if !self.expanded.remove(&id) {
                            self.expanded.insert(id);
                        }
                    }
                    None => {}
                }
            });

        // Dialoge (fixe Fenster, siehe dialogs.rs).
        dialogs::show_editor(self, ctx);
        dialogs::show_open(self, ctx);
        dialogs::show_settings(self, ctx);
        dialogs::show_project(self, ctx);
        dialogs::show_export(self, ctx);
        dialogs::show_image_intent(self, ctx);
    }
}

// --- Layout-Datei -----------------------------------------------------------

/// Separates Layout-File (`<projekt>.layout.json`) neben den Projektdaten:
/// manuelle Verschiebungen der Baumkarten, über IDs zugeordnet. Die Offsets
/// sind relativ zur verhandelten Position (Eltern-Anker) und werden in
/// horizontaler wie vertikaler Ausrichtung auf die jeweilige
/// Verteilungsachse angewendet.
///
/// Version 2 = hierarchische Vererbung (Offset nur bei der gezogenen Person,
/// Nachfahren/Vorfahren erben). Version 1 (flache Versätze auf alle
/// mitgezogenen Karten) ist inkompatibel und wird beim Laden ignoriert.
#[derive(serde::Serialize, serde::Deserialize)]
struct LayoutFile {
    version: u32,
    entries: Vec<LayoutEntry>,
}

/// Aktuelle Layout-Dateiversion (hierarchische Versätze).
const LAYOUT_VERSION: u32 = 2;

#[derive(serde::Serialize, serde::Deserialize)]
struct LayoutEntry {
    id: String,
    offset: f32,
}

fn layout_path(data_path: &Path) -> PathBuf {
    let stem = data_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("layout");
    data_path.with_file_name(format!("{stem}.layout.json"))
}

fn load_layout(data_path: &Path) -> HashMap<String, f32> {
    let Ok(text) = fs::read_to_string(layout_path(data_path)) else {
        return HashMap::new();
    };
    serde_json::from_str::<LayoutFile>(&text)
        .map(|file| {
            // Alte flache Layouts (Version 1) nicht hierarchisch
            // interpretieren — die Versätze entstehen neu.
            if file.version < LAYOUT_VERSION {
                return HashMap::new();
            }
            file.entries
                .into_iter()
                .map(|entry| (entry.id, entry.offset))
                .collect()
        })
        .unwrap_or_default()
}

// --- Icons & Schriften -----------------------------------------------------

/// Feather-SVGs nutzen `stroke="currentColor"`, das der SVG-Loader schwarz
/// rendert; Weiß + `.tint(text_color)` ergibt exakt die Schriftfarbe.
pub(crate) fn whitened_svg(bytes: &'static [u8]) -> Vec<u8> {
    if bytes.windows(12).any(|window| window == b"currentColor") {
        String::from_utf8_lossy(bytes)
            .replace("currentColor", "#ffffff")
            .into_bytes()
    } else {
        bytes.to_vec()
    }
}

/// Logo-SVG für die Titelleiste: Der Pfad hat kein fill-Attribut (würde
/// schwarz gerendert) und erhält weiße Füllung, die über
/// `.tint(text_color)` exakt die Schriftfarbe annimmt.
pub(crate) fn whitened_logo() -> Vec<u8> {
    String::from_utf8_lossy(LOGO)
        .replacen("<path ", "<path fill=\"#ffffff\" ", 1)
        .into_bytes()
}

pub(crate) fn icon(ui: &mut egui::Ui, bytes: &'static [u8], id: &str) {
    ui.add(
        egui::Image::from_bytes(format!("bytes://{id}.svg"), whitened_svg(bytes))
            .fit_to_exact_size(Vec2::splat(15.0))
            .tint(ui.visuals().text_color()),
    );
}

pub(crate) fn icon_button(
    ui: &mut egui::Ui,
    bytes: &'static [u8],
    id: &'static str,
    label: &str,
) -> egui::Response {
    let image = egui::Image::from_bytes(format!("bytes://{id}.svg"), whitened_svg(bytes))
        .fit_to_exact_size(Vec2::splat(16.0))
        .tint(ui.visuals().text_color());
    ui.add(egui::Button::image_and_text(image, label))
}

pub(crate) fn icon_only_button(
    ui: &mut egui::Ui,
    bytes: &'static [u8],
    id: &'static str,
) -> egui::Response {
    let image = egui::Image::from_bytes(format!("bytes://{id}.svg"), whitened_svg(bytes))
        .fit_to_exact_size(Vec2::splat(16.0))
        .tint(ui.visuals().text_color());
    ui.add(egui::Button::image(image))
}

pub(crate) fn icon_button_big(
    ui: &mut egui::Ui,
    bytes: &'static [u8],
    id: &'static str,
    tooltip: &str,
) -> egui::Response {
    let image = egui::Image::from_bytes(format!("bytes://{id}.svg"), whitened_svg(bytes))
        .fit_to_exact_size(Vec2::splat(24.0))
        .tint(ui.visuals().text_color());
    ui.add(egui::Button::image(image).min_size(Vec2::new(38.0, 34.0)))
        .on_hover_text(tooltip)
}

pub(crate) fn window_title(text: &str) -> egui::RichText {
    egui::RichText::new(text).size(13.0).strong()
}

// --- Start & globale Konfiguration -----------------------------------------

/// Einstiegspunkt: Fenster, App-Icon (Logo gerendert via resvg), Schriften.
pub fn run() -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1280., 760.])
        .with_min_inner_size([900., 560.])
        .with_decorations(false);
    if let Some(icon) = load_app_icon() {
        viewport = viewport.with_icon(icon);
    }
    eframe::run_native(
        "MiniGramps",
        eframe::NativeOptions {
            viewport,
            ..Default::default()
        },
        Box::new(|cc| {
            configure_fonts(&cc.egui_ctx);
            Ok(Box::new(MiniGramps::new()))
        }),
    )
}

/// App-Icon aus dem eingebetteten Logo (`assets/icon.svg`) rasterisieren.
fn load_app_icon() -> Option<egui::IconData> {
    let tree = resvg::usvg::Tree::from_data(LOGO, &resvg::usvg::Options::default()).ok()?;
    let size = tree.size();
    let side = 64;
    let scale = side as f32 / size.width().max(size.height());
    let mut pixmap = resvg::tiny_skia::Pixmap::new(side, side)?;
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Some(egui::IconData {
        width: side,
        height: side,
        rgba: pixmap.take(),
    })
}

/// Schriftarten: Libre Baskerville eingebettet (`assets/fonts`, OFL-Lizenz,
/// frei mitauslieferbar), SVG-Image-Loader für die Icons installieren.
fn configure_fonts(ctx: &egui::Context) {
    egui_extras::install_image_loaders(ctx);
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "libre-baskerville".into(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/LibreBaskerville-Regular.ttf"
        ))),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "libre-baskerville".into());
    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_current_color_in_svg() {
        let output = whitened_svg(b"<svg stroke=\"currentColor\"></svg>");
        assert!(output.windows(7).any(|window| window == b"#ffffff"));
        assert!(!output.windows(12).any(|window| window == b"currentColor"));
    }
}
