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
use crate::store::{DataStore, FileSystemStore};
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
pub(crate) const ICON_CHEVRON_LEFT: &[u8] = include_bytes!("../../assets/icons/chevron-left.svg");
pub(crate) const ICON_CHEVRON_RIGHT: &[u8] = include_bytes!("../../assets/icons/chevron-right.svg");
pub(crate) const ICON_TRASH: &[u8] = include_bytes!("../../assets/icons/trash-2.svg");
pub(crate) const ICON_EXTERNAL_LINK: &[u8] = include_bytes!("../../assets/icons/external-link.svg");
pub(crate) const ICON_CENTER: &[u8] = include_bytes!("../../assets/icons/crosshair.svg");
pub(crate) const ICON_RESET: &[u8] = include_bytes!("../../assets/icons/refresh-cw.svg");
pub(crate) const ICON_UNDO: &[u8] = include_bytes!("../../assets/icons/rotate-ccw.svg");
pub(crate) const ICON_REDO: &[u8] = include_bytes!("../../assets/icons/rotate-cw.svg");
pub(crate) const ICON_POINTER: &[u8] = include_bytes!("../../assets/icons/mouse-pointer.svg");
pub(crate) const ICON_ZOOM: &[u8] = include_bytes!("../../assets/icons/zoom-in.svg");
pub(crate) const LOGO: &[u8] = include_bytes!("../../assets/icon.svg");
const MAX_INTERACTIVE_ZOOM: f32 = 8.0;

/// Angefragter Personenwechsel während offener ungespeicherter Bearbeitung.
#[derive(Clone, Debug)]
pub struct PendingSelect {
    pub target: String,
    pub set_reference: bool,
}

#[derive(Clone)]
pub struct HistoryEntry {
    pub name: String,
    pub data: TreeData,
    pub manual_offsets: HashMap<String, f32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TreeTool {
    Cursor,
    Zoom,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CardLayout {
    Compact,
    Portrait,
}

impl CardLayout {
    pub(crate) fn height(self) -> f32 {
        match self {
            Self::Compact => 78.0,
            Self::Portrait => 158.0,
        }
    }
}

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
    /// Maus-Status vom Vorgängerframe zur Erkennung von Drag-Start/Ende.
    pub drag_mouse_was_down: bool,
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
    /// Projektname beim Beginn der Texteingabe, damit die komplette Änderung
    /// als ein Undo-Schritt gespeichert wird.
    pub project_name_before_edit: Option<String>,
    /// Auswahl der kuenftigen Exportformate, geoeffnet aus dem Projektfenster.
    pub show_export: bool,
    /// Sortierung der Personenliste: nach Anzahl (true) oder Alphabet.
    pub group_by_count: bool,
    /// Vorbereitete linke Personenliste; wird nur nach Daten- oder
    /// Sortieränderungen neu gruppiert und sortiert.
    pub people_groups: Vec<(String, Vec<(String, Gender, String)>)>,
    pub people_groups_dirty: bool,
    /// Textur-Cache (`media::photo_texture`), Schlüssel = `Person::id`
    /// bzw. Galerie-Pseudo-IDs `gallery-<id>-<index>`.
    pub photo_cache: HashMap<String, TextureHandle>,
    pub tree_view: TreeView,
    /// Im Öffnen-Dialog ausgewähltes Projekt (wird mit "Laden" geöffnet).
    pub selected_project: Option<PathBuf>,
    pub dark_mode: bool,
    /// Darstellung der Personenkarten im Stammbaum.
    pub card_layout: CardLayout,
    pub tree_orientation: TreeOrientation,
    /// ID der im Modal-Editor geöffneten Person (None = neue Person).
    pub editing: Option<String>,
    /// Arbeitskopie für Inline- und Modal-Bearbeitung.
    pub draft: Person,
    /// Inline-Bearbeitung im Profil (Stift/Diskette in der Leiste).
    pub inline_edit: bool,
    /// Suchtext des Beziehungspickers.
    pub relation_query: String,
    /// Nachname des Beziehungspickers.
    pub relation_family_name: String,
    /// Per Drag-and-drop eingefügte Datei mit unklarer Verwendung.
    pub pending_image: Option<PathBuf>,
    /// Vollbildansicht eines Galerie-Bildes.
    pub lightbox_image: Option<String>,
    /// Sender für asynchrone Lightbox-Dekodierung (`crate::media::AsyncImage`).
    pub lightbox_tx: std::sync::mpsc::Sender<crate::media::AsyncImage>,
    /// Empfänger für fertige asynchrone Lightbox-Bilder.
    pub lightbox_rx: std::sync::mpsc::Receiver<crate::media::AsyncImage>,
    /// Cache-Schlüssel der bereits angelaufenen Lightbox-Dekodier-Jobs.
    pub lightbox_loading: HashSet<String>,
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
    /// Schließen angefordert, aber ungespeicherte Änderungen prüfen.
    pub pending_close: bool,
    /// Angefragte Personen-Auswahl bei laufender unsicherer Bearbeitung
    /// (Wechsel-Dialog in `dialogs::show_pending_select_confirm`).
    pub pending_select: Option<PendingSelect>,
    /// Undo-Verlauf: Datenschnappschüsse VOR jeder Mutation (max. 100).
    pub undo_stack: Vec<HistoryEntry>,
    /// Redo-Verlauf: verlassene Zustände für Strg+Umschalt+Z / Strg+Y.
    pub redo_stack: Vec<HistoryEntry>,
    /// Aktives Werkzeug auf der Baumzeichenfläche.
    pub tree_tool: TreeTool,
    /// Startpunkt des Auswahlrahmens des Lupenwerkzeugs (Bildschirmkoordinaten).
    pub zoom_selection_start: Option<egui::Pos2>,
    /// Server-Verbindung (Öffnen-Dialog): Basis-URL + Token (Sitzung).
    pub server_url: String,
    pub server_token: String,
    /// Verbindungsstatus für das Logo (Hover: Details).
    pub server_online: bool,
    pub server_base: Option<String>,
    /// Debug-Log (Leiste unten + Terminal via `log`).
    /// Pfad der aktuell geöffneten Projektdatei (für `<stem>.layout.json`).
    pub current_data_path: Option<PathBuf>,
    started: std::time::Instant,
}

impl MiniGramps {
    pub fn new() -> Self {
        let library = default_library();
        let _ = fs::create_dir_all(&library);
        let (lightbox_tx, lightbox_rx) = std::sync::mpsc::channel();
        let mut app = Self {
            data: TreeData::demo(),
            selected: Some("p5".into()),
            reference: Some("p5".into()),
            expanded: HashSet::new(),
            manual_offsets: HashMap::new(),
            long_press_used: false,
            card_drag: None,
            drag_mouse_was_down: false,
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
            project_name_before_edit: None,
            show_export: false,
            group_by_count: true,
            people_groups: Vec::new(),
            people_groups_dirty: true,
            photo_cache: HashMap::new(),
            tree_view: TreeView::Descendants,
            selected_project: None,
            dark_mode: true,
            card_layout: CardLayout::Compact,
            tree_orientation: TreeOrientation::Vertical,
            editing: None,
            draft: person("", "", "", "", Gender::Unknown),
            inline_edit: false,
            relation_query: String::new(),
            relation_family_name: String::new(),
            pending_image: None,
            lightbox_image: None,
            lightbox_tx,
            lightbox_rx,
            lightbox_loading: HashSet::new(),
            pending_child_for: None,
            pending_child_partner: None,
            pending_child_relation: ChildRelation::Birth,
            relation_picker: None,
            relation_editor: None,
            collapsed_sections: HashSet::new(),
            hidden_sections: HashSet::new(),
            pending_close: false,
            pending_select: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            tree_tool: TreeTool::Cursor,
            zoom_selection_start: None,
            server_url: String::new(),
            server_token: String::new(),
            server_online: false,
            server_base: None,
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
        // Erst verwaiste Mediendateien und Cache-Bilder bereinigen, um Plattenplatz zu sparen!
        crate::media::cleanup_unused_media(&self.library, &self.data);

        let path = self.library.join("familienbaum.minigramps.json");
        self.current_data_path = Some(path.clone());
        let store = FileSystemStore::for_data_file(&path);
        let entries: Vec<(String, f32)> = self
            .manual_offsets
            .iter()
            .map(|(id, offset)| (id.clone(), *offset))
            .collect();
        match store
            .write_data(&self.data)
            .and_then(|_| store.write_layout(&entries))
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
                self.people_groups_dirty = true;
                self.undo_stack.clear();
                self.redo_stack.clear();
                self.pending_select = None;
                self.inline_edit = false;
                self.show_editor = false;
                self.editing = None;
                self.relation_picker = None;
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
                self.manual_offsets = FileSystemStore::for_data_file(path).read_layout();
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

    /// Projekt vom Server laden (Öffnen-Dialog → Server: URL + Login).
    pub fn load_from_server(&mut self, base_url: &str, token: &str) {
        let store = crate::store::ServerStore::new(
            crate::store::UreqTransport,
            crate::store::ServerConfig {
                base_url: base_url.to_string(),
                token: (!token.is_empty()).then(|| token.to_string()),
            },
        );
        // Offline-Cache je Server-URL (offline am Online-Projekt arbeiten).
        let cache_root = default_library().join("server-cache").join(
            base_url
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect::<String>(),
        );
        match store.read_data() {
            Ok(mut data) => {
                if let Some(manifest) = store.read_manifest() {
                    data.project.name = manifest.name;
                }
                self.manual_offsets = store.read_layout();
                // Cache schreiben, damit offline weitergearbeitet werden kann.
                let cache = FileSystemStore::for_root(cache_root);
                let _ = cache.write_data(&data);
                let _ = cache.write_layout(
                    &self
                        .manual_offsets
                        .iter()
                        .map(|(id, offset)| (id.clone(), *offset))
                        .collect::<Vec<_>>(),
                );
                self.server_online = true;
                self.server_base = Some(base_url.to_string());
                self.selected = data.people.first().map(|p| p.id.clone());
                self.reference = self.selected.clone();
                self.expanded.clear();
                self.data = data;
                self.people_groups_dirty = true;
                self.undo_stack.clear();
                self.redo_stack.clear();
                self.pending_select = None;
                self.inline_edit = false;
                self.show_editor = false;
                self.editing = None;
                self.relation_picker = None;
                self.photo_cache.clear();
                self.fit_pending = true;
                self.show_open = false;
                self.status = format!("Geöffnet: {base_url}");
                self.log(format!("Vom Server geladen: {base_url}"));
            }
            Err(e) => {
                // Offline? Lokale Kopie des Online-Projekts laden.
                let cache = FileSystemStore::for_root(cache_root);
                match cache.read_data() {
                    Ok(mut data) => {
                        if let Some(manifest) = cache.read_manifest() {
                            data.project.name = manifest.name;
                        }
                        self.manual_offsets = cache.read_layout();
                        self.server_online = false;
                        self.server_base = Some(base_url.to_string());
                        self.selected = data.people.first().map(|p| p.id.clone());
                        self.reference = self.selected.clone();
                        self.expanded.clear();
                        self.data = data;
                        self.people_groups_dirty = true;
                        self.undo_stack.clear();
                        self.redo_stack.clear();
                        self.pending_select = None;
                        self.inline_edit = false;
                        self.show_editor = false;
                        self.editing = None;
                        self.relation_picker = None;
                        self.photo_cache.clear();
                        self.fit_pending = true;
                        self.show_open = false;
                        self.current_data_path = None;
                        self.status = format!("Offline-Kopie geladen: {base_url}");
                        self.log(format!("Offline-Kopie geladen: {base_url}"));
                    }
                    Err(_) => {
                        self.status = format!("Server-Laden fehlgeschlagen: {e}");
                        self.log(format!("Server-Laden fehlgeschlagen: {e}"));
                    }
                }
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

    /// Arbeitskopie hat im Vergleich zur Datenbank ungespeicherte Änderungen?
    pub fn draft_has_changes(&self) -> bool {
        match self.data.find(&self.draft.id) {
            Some(stored) => stored != &self.draft,
            None => false,
        }
    }

    /// Zustand VOR einer Datenänderung sichern (Undo). Jede neue Änderung
    /// leert den Redo-Verlauf; die Historie ist auf 100 Schritte begrenzt.
    pub fn snapshot(&mut self, name: impl Into<String>) {
        self.people_groups_dirty = true;
        while self.undo_stack.len() >= 100 {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(HistoryEntry {
            name: name.into(),
            data: self.data.clone(),
            manual_offsets: self.manual_offsets.clone(),
        });
        self.redo_stack.clear();
    }

    /// Snapshot für eine gerade begonnene Layoutänderung; die Daten sind noch
    /// unverändert, der vorherige Offset-Zustand kommt aus dem Drag-Startframe.
    fn snapshot_layout_before(
        &mut self,
        name: impl Into<String>,
        manual_offsets: HashMap<String, f32>,
    ) {
        while self.undo_stack.len() >= 100 {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(HistoryEntry {
            name: name.into(),
            data: self.data.clone(),
            manual_offsets,
        });
        self.redo_stack.clear();
    }

    pub fn undo_action_name(&self) -> Option<&str> {
        self.undo_stack.last().map(|entry| entry.name.as_str())
    }

    pub fn redo_action_name(&self) -> Option<&str> {
        self.redo_stack.last().map(|entry| entry.name.as_str())
    }

    pub fn undo(&mut self) {
        let Some(previous) = self.undo_stack.pop() else {
            return;
        };
        let name = previous.name;
        self.redo_stack.push(HistoryEntry {
            name: name.clone(),
            data: std::mem::replace(&mut self.data, previous.data),
            manual_offsets: std::mem::replace(
                &mut self.manual_offsets,
                previous.manual_offsets,
            ),
        });
        self.refresh_after_rollback();
        self.status = format!("Rückgängig: {name}");
    }

    pub fn redo(&mut self) {
        let Some(next) = self.redo_stack.pop() else {
            return;
        };
        let name = next.name;
        self.undo_stack.push(HistoryEntry {
            name: name.clone(),
            data: std::mem::replace(&mut self.data, next.data),
            manual_offsets: std::mem::replace(&mut self.manual_offsets, next.manual_offsets),
        });
        self.refresh_after_rollback();
        self.status = format!("Wiederholt: {name}");
    }

    /// Nach Undo/Redo: Bearbeitung beenden, Auswahl an die Daten angleichen.
    fn refresh_after_rollback(&mut self) {
        self.people_groups_dirty = true;
        self.inline_edit = false;
        self.show_editor = false;
        self.editing = None;
        self.relation_picker = None;
        self.relation_editor = None;
        self.pending_select = None;
        self.photo_cache.clear();
        let keep = self
            .selected
            .clone()
            .filter(|id| self.data.find(id).is_some());
        if let Some(id) = keep {
            if let Some(person) = self.data.find(&id).cloned() {
                self.draft = person;
                return;
            }
        }
        if let Some(first) = self.data.people.first() {
            self.selected = Some(first.id.clone());
            self.draft = first.clone();
        } else {
            self.selected = None;
            self.draft = person("", "", "", "", Gender::Unknown);
        }
    }

    /// Ungespeicherte Arbeitskopie in die Datenbank übernehmen (Undo-Schritt
    /// wird gesichert). Gemeinsame Logik von Diskette, Editor und
    /// Personenwechsel-Dialog.
    pub fn commit_draft(&mut self) {
        let name = self.draft.display_name();
        self.snapshot(format!("Profil bearbeiten: {name}"));
        if let Some(person) = self
            .data
            .people
            .iter_mut()
            .find(|person| person.id == self.draft.id)
        {
            *person = self.draft.clone();
        }
        let _ = crate::media::write_round_avatar_now(&self.library, &self.draft);
        crate::media::clear_person_photo_cache(&mut self.photo_cache, &self.draft.id);
    }

    /// Person auswählen. Läuft gerade eine Bearbeitung mit ungespeicherten
    /// Änderungen, wird erst der Wechsel-Dialog eingeblendet
    /// (`pending_select`), statt sofort zu wechseln.
    pub fn request_select(&mut self, id: &str, set_reference: bool) {
        if self.inline_edit
            && self.draft_has_changes()
            && self.selected.as_deref() != Some(id)
        {
            self.pending_select = Some(PendingSelect {
                target: id.into(),
                set_reference,
            });
            return;
        }
        self.apply_select(id, set_reference);
    }

    fn apply_select(&mut self, id: &str, set_reference: bool) {
        self.selected = Some(id.into());
        self.relation_picker = None;
        if set_reference {
            self.set_reference(id);
        }
    }

    /// Vom Wechsel-Dialog bestätigten Zielwechsel ausführen.
    pub(crate) fn apply_pending_select(&mut self) {
        if let Some(pending) = self.pending_select.take() {
            self.apply_select(&pending.target, pending.set_reference);
        }
    }

    /// Zeichenfläche (Strg+Z / Strg+Y) zurück- und vorlaufen lassen.
    pub fn handle_undo_redo_shortcuts(&mut self, ctx: &egui::Context) {
        // Während ein Textfeld fokussiert ist, gehört Strg+Z dem Texteditor.
        if ctx.wants_keyboard_input() {
            return;
        }
        let (undo, redo) = ctx.input(|i| {
            let command = i.modifiers.command;
            let undo = command
                && !i.modifiers.shift
                && i.key_pressed(egui::Key::Z);
            let redo = (command && i.key_pressed(egui::Key::Y))
                || (command && i.modifiers.shift && i.key_pressed(egui::Key::Z));
            (undo, redo)
        });
        if undo {
            self.undo();
        } else if redo {
            self.redo();
        }
    }

    /// Zoomt um den Mauspunkt, sodass derselbe Layoutpunkt unter dem Cursor
    /// bleibt. Faktor > 1 zoomt hinein, Faktor < 1 heraus.
    fn zoom_at(&mut self, canvas: egui::Rect, pointer: egui::Pos2, factor: f32) {
        let old_zoom = self.zoom;
        let new_zoom = (old_zoom * factor).clamp(0.15, MAX_INTERACTIVE_ZOOM);
        if (new_zoom - old_zoom).abs() < f32::EPSILON {
            return;
        }
        let pointer_from_center = pointer - canvas.center();
        let layout_point = (pointer_from_center - self.pan) / old_zoom;
        self.zoom = new_zoom;
        self.pan = pointer_from_center - layout_point * new_zoom;
    }

    /// Passt einen auf der Zeichenfläche aufgespannten Bildschirmrahmen in
    /// den Viewport ein und hält dessen Mitte im Zentrum.
    fn fit_screen_selection(&mut self, canvas: egui::Rect, selection: egui::Rect) {
        let old_zoom = self.zoom;
        let layout_center =
            (selection.center() - canvas.center() - self.pan) / old_zoom;
        let scale = ((canvas.width() / selection.width())
            .min(canvas.height() / selection.height())
            * 0.92)
            .max(0.01);
        self.zoom = (old_zoom * scale).clamp(0.15, MAX_INTERACTIVE_ZOOM);
        self.pan = layout_center * -self.zoom;
    }

    /// Layout (manuelle Offsets) sofort in die `.layout.json` schreiben.
    fn persist_layout(&mut self) {
        let data_path = self
            .current_data_path
            .clone()
            .unwrap_or_else(|| self.library.join("familienbaum.minigramps.json"));
        let entries: Vec<(String, f32)> = self
            .manual_offsets
            .iter()
            .map(|(id, offset)| (id.clone(), *offset))
            .collect();
        match FileSystemStore::for_data_file(&data_path).write_layout(&entries) {
            Ok(_) => self.log(format!(
                "Layout gesichert: {} ({} Einträge)",
                data_path.display(),
                entries.len()
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

        // Schließen abfangen: Bei ungespeicherten Änderungen erst nachfragen.
        let native_close = ctx.input(|i| i.viewport().close_requested());
        if self.pending_close || native_close {
            // Ungespeichert = eine Bearbeitung läuft gerade (Profil/Editor).
            if self.inline_edit || self.show_editor {
                if native_close {
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                }
                self.pending_close = true;
            } else if self.pending_close {
                self.pending_close = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        // Undo/Redo über die Tastatur (Strg+Z / Strg+Y bzw. Strg+Umschalt+Z) —
        // außerhalb von aktiven Textfeldern.
        self.handle_undo_redo_shortcuts(ctx);

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
                            .size(13.0)
                            .strong()
                            .color(section_accent),
                    );
                    ui.separator();
                // Manuelle Offsets gelten in BEIDEN Ausrichtungen (Layout-
                // Datei) und bleiben beim Ansichtwechsel erhalten.
                let old_view = self.tree_view;
                let old_orientation = self.tree_orientation;
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
                if self.tree_view != old_view || self.tree_orientation != old_orientation {
                    self.fit_pending = true;
                }
                ui.separator();
                if icon_only_button(ui, ICON_CENTER, "toolbar-center")
                    .on_hover_text("Stammbaum zentrieren und im Fenster einpassen (Zoom-Fit)")
                    .clicked()
                {
                    println!("CENTER: zoom={:.3} pan=({:.1},{:.1}) offsets={}",
                        self.zoom, self.pan.x, self.pan.y, self.manual_offsets.len());
                    self.fit_pending = true;
                }
                if icon_only_button(ui, ICON_RESET, "toolbar-reset")
                    .on_hover_text("Alle manuellen Verschiebungen zurücksetzen (Layout-Reset)")
                    .clicked()
                {
                    println!("RESET: clearing {} offsets, zoom={:.3} pan=({:.1},{:.1})",
                        self.manual_offsets.len(), self.zoom, self.pan.x, self.pan.y);
                    if !self.manual_offsets.is_empty() {
                        self.snapshot("Layout zurücksetzen");
                    }
                    self.manual_offsets.clear();
                    self.persist_layout();
                    self.zoom = 1.0;
                    self.pan = egui::Vec2::ZERO;
                    self.fit_pending = true;
                }
                ui.separator();
                if icon_toggle_button(
                    ui,
                    ICON_POINTER,
                    "toolbar-pointer",
                    self.tree_tool == TreeTool::Cursor,
                )
                .on_hover_text("Standardwerkzeug: auswählen und Baum verschieben")
                .clicked()
                {
                    self.tree_tool = TreeTool::Cursor;
                    self.zoom_selection_start = None;
                }
                if icon_toggle_button(
                    ui,
                    ICON_ZOOM,
                    "toolbar-zoom",
                    self.tree_tool == TreeTool::Zoom,
                )
                .on_hover_text(
                    "Lupe: Linksklick hinein · Rechtsklick heraus · Linksklick-Ziehen: Bereich einpassen",
                )
                .clicked()
                {
                    self.tree_tool = TreeTool::Zoom;
                    self.card_drag = None;
                }
                ui.separator();
                    // Hinweis nur, wenn genug Platz (sonst automatisch aus).
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(8.0);
                        let hint = if self.tree_tool == TreeTool::Zoom {
                            "Lupe: Linksklick hinein · Rechtsklick heraus · Ziehen: Bereich einpassen"
                        } else {
                            "Mausrad: Zoom · Ziehen: Baum verschieben · Klick: Person · Shift+Klick: Referenz · Shift+Ziehen: Karte/Zweig/Partner-Tausch"
                        };
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
                if self.tree_tool == TreeTool::Zoom && response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                }
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
                    self.zoom = (self.zoom * (1.0 + scroll * 0.001))
                        .clamp(0.15, MAX_INTERACTIVE_ZOOM);
                }
                // Maus-Transitions-Erkennung für Drag-Logging.
                let mouse_is_down = ui.input(|i| i.pointer.primary_down());
                let drag_started = mouse_is_down && !self.drag_mouse_was_down;
                let drag_ended = !mouse_is_down && self.drag_mouse_was_down;
                self.drag_mouse_was_down = mouse_is_down;
                // Umfangreiche Layoutdiagnose bleibt auf Debug-Builds begrenzt.
                let log_layout = cfg!(debug_assertions) && (self.fit_pending || drag_ended);
                // Lang-Touch-Debouncer und aktiven Karten-Drag zurücksetzen,
                // wenn nichts gedrückt ist. Drag-Ende → Layout live sichern.
                if !mouse_is_down {
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
                    let previous_manual_offsets = self.manual_offsets.clone();
                    let card_drag_was_active = self.card_drag.is_some();
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
                        self.card_layout,
                        self.zoom,
                        self.pan,
                        drag_started,
                        drag_ended,
                        log_layout,
                    );
                    self.long_press_used = long_press_used;
                    if self.tree_tool == TreeTool::Cursor {
                        if !card_drag_was_active
                            && card_drag.is_some()
                            && manual_offsets != previous_manual_offsets
                        {
                            let drag_id = card_drag
                                .as_ref()
                                .map(|(id, _)| id.as_str())
                                .unwrap_or_default();
                            let name = self
                                .data
                                .find(drag_id)
                                .map(|person| person.display_name())
                                .unwrap_or_else(|| drag_id.to_string());
                            self.snapshot_layout_before(
                                format!("Baumposition verschieben: {name}"),
                                previous_manual_offsets,
                            );
                        }
                        self.manual_offsets = manual_offsets;
                        self.card_drag = card_drag;
                    } else {
                        action = None;
                        self.card_drag = None;
                    }
                // Zoom-Fit: den gelieferten Inhaltsbereich (Layout-Koordo-
                // dinaten) passend in die Zeichenfläche skalieren und mittig
                // setzen — einmalig nach Laden/Referenzwechsel.
                if self.fit_pending {
                    self.fit_pending = false;
                    let canvas = response.rect.size();
                    let size = content_bounds.size();
                    if size.x > 1.0 && size.y > 1.0 {
                        let fit = ((canvas.x / size.x).min(canvas.y / size.y) * 0.92)
                            .clamp(0.15, 1.4);
                        self.zoom = fit;
                        // Baum-Mitte auf Canvas-Mitte: gezeichnet wird an
                        // center + pan + pos*zoom → pan = -mitte*zoom.
                        self.pan = content_bounds.center().to_vec2() * -fit;
                    }
                }
                if self.tree_tool == TreeTool::Zoom {
                    let pointer = ui.input(|input| input.pointer.interact_pos());
                    let primary_pressed = ui.input(|input| {
                        input.pointer.button_pressed(egui::PointerButton::Primary)
                    });
                    let primary_released = ui.input(|input| {
                        input.pointer.button_released(egui::PointerButton::Primary)
                    });
                    let secondary_released = ui.input(|input| {
                        input.pointer.button_released(egui::PointerButton::Secondary)
                    });
                    if primary_pressed && pointer.is_some_and(|at| response.rect.contains(at)) {
                        self.zoom_selection_start = pointer;
                    }
                    if let (Some(start), Some(current)) = (self.zoom_selection_start, pointer) {
                        let selection = egui::Rect::from_two_pos(start, current)
                            .intersect(response.rect);
                        if ui.input(|input| {
                            input.pointer.button_down(egui::PointerButton::Primary)
                        }) {
                            painter.rect_filled(
                                selection,
                                0.0,
                                Color32::from_rgba_unmultiplied(80, 150, 220, 28),
                            );
                            painter.rect_stroke(
                                selection,
                                0.0,
                                egui::Stroke::new(1.5, Color32::from_rgb(90, 170, 235)),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if primary_released {
                            self.zoom_selection_start = None;
                            if selection.width() >= 8.0 && selection.height() >= 8.0 {
                                self.fit_screen_selection(response.rect, selection);
                            } else {
                                self.zoom_at(response.rect, current, 1.25);
                            }
                        }
                    }
                    if secondary_released
                        && pointer.is_some_and(|at| response.rect.contains(at))
                    {
                        self.zoom_selection_start = None;
                        self.zoom_at(response.rect, pointer.unwrap(), 0.8);
                    }
                } else {
                    // Canvas-Pan NACH dem Zeichnen auswerten: Ein Drag innerhalb
                    // eines Paarrahmens verschiebt den Zweig (frame_drag) und
                    // darf die Zeichenfläche nicht mitschieben.
                    if response.dragged() && !ui.input(|i| i.modifiers.shift) && !frame_drag {
                        self.pan += response.drag_delta();
                    }
                }
                match action {
                    Some(TreeAction::View(id)) => self.request_select(&id, false),
                    Some(TreeAction::Reference(id)) => self.request_select(&id, true),
                    Some(TreeAction::ToggleExpand(id)) => {
                        if !self.expanded.remove(&id) {
                            self.expanded.insert(id);
                        }
                    }
                    Some(TreeAction::SwapPartner {
                        person_id,
                        partner_id,
                        direction,
                    }) => {
                        let name = self
                            .data
                            .find(&partner_id)
                            .map(|person| person.display_name())
                            .unwrap_or_else(|| partner_id.clone());
                        self.snapshot(format!("Partnerreihenfolge ändern: {name}"));
                        self.data.swap_partner(&person_id, &partner_id, direction);
                        self.log(format!(
                            "Partner getauscht: {person_id} <-> {partner_id} ({direction})"
                        ));
                    }
                    None => {}
                }
            });

        // Dialoge (fixe Fenster, siehe dialogs.rs).
        dialogs::show_close_confirm(self, ctx);
        dialogs::show_editor(self, ctx);
        dialogs::show_open(self, ctx);
        dialogs::show_settings(self, ctx);
        dialogs::show_project(self, ctx);
        dialogs::show_export(self, ctx);
        dialogs::show_image_intent(self, ctx);
        dialogs::show_lightbox(self, ctx);
        // Wechsel-Dialog bei ungespeicherten Änderungen (vor dem nächsten Frame).
        dialogs::show_pending_select_confirm(self, ctx);
    }
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

pub(crate) fn icon_toggle_button(
    ui: &mut egui::Ui,
    bytes: &'static [u8],
    id: &'static str,
    selected: bool,
) -> egui::Response {
    let image = egui::Image::from_bytes(format!("bytes://{id}.svg"), whitened_svg(bytes))
        .fit_to_exact_size(Vec2::splat(16.0))
        .tint(ui.visuals().text_color());
    ui.add(egui::Button::image(image).selected(selected))
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
