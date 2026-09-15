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
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
use rfd::FileDialog;

use crate::import::{default_library, load_file, load_project_manifest, save_project_manifest};
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
use crate::import::{load_last_project, save_last_project};
use crate::model::{ChildRelation, Gender, MergeCandidate, Person, TreeData, person};
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
pub(crate) const ICON_UNLINK: &[u8] = include_bytes!("../../assets/icons/unlink.svg");
pub(crate) const ICON_EDIT: &[u8] = include_bytes!("../../assets/icons/edit-3.svg");
pub(crate) const ICON_REFERENCE: &[u8] = include_bytes!("../../assets/icons/star.svg");
pub(crate) const ICON_CHEVRON_LEFT: &[u8] = include_bytes!("../../assets/icons/chevron-left.svg");
pub(crate) const ICON_CHEVRON_RIGHT: &[u8] = include_bytes!("../../assets/icons/chevron-right.svg");
pub(crate) const ICON_TRASH: &[u8] = include_bytes!("../../assets/icons/trash-2.svg");
pub(crate) const ICON_EXTERNAL_LINK: &[u8] = include_bytes!("../../assets/icons/external-link.svg");
pub(crate) const ICON_CENTER: &[u8] = include_bytes!("../../assets/icons/crosshair.svg");
pub(crate) const ICON_RESET: &[u8] = include_bytes!("../../assets/icons/refresh-cw.svg");
pub(crate) const ICON_UNDO: &[u8] = include_bytes!("../../assets/icons/rotate-ccw.svg");
pub(crate) const ICON_MERGE: &[u8] = include_bytes!("../../assets/icons/git-merge.svg");
pub(crate) const ICON_REDO: &[u8] = include_bytes!("../../assets/icons/rotate-cw.svg");
pub(crate) const ICON_POINTER: &[u8] = include_bytes!("../../assets/icons/mouse-pointer.svg");
pub(crate) const ICON_ZOOM: &[u8] = include_bytes!("../../assets/icons/zoom-in.svg");
pub(crate) const ICON_DEBUG: &[u8] = include_bytes!("../../assets/icons/tool.svg");
pub(crate) const LOGO: &[u8] = include_bytes!("../../assets/icon.svg");
const MAX_INTERACTIVE_ZOOM: f32 = 8.0;

/// Ein Review-Eintrag: Treffer mit Auswahl (zusammenführen?) plus
/// Feldwahl für Geburt/Tod (Seite Neu/Vorhanden, sonst Standard).
pub struct MergeReviewEntry {
    pub candidate: MergeCandidate,
    pub selected: bool,
    pub take_new_birth: bool,
    pub take_new_death: bool,
    /// Name von Neu übernehmen (statt Bestand).
    pub take_new_name: bool,
    /// Explizit manuell hinzugefügt (bleibt bei Neuaufbau der Automatik
    /// erhalten, wird nur neu bewertet).
    pub manual: bool,
    /// Eltern-Mitmergen: gewählter Elternteil je Seite (Neu/Vorhanden).
    pub parent_pick_new: Option<String>,
    pub parent_pick_old: Option<String>,
}

/// Offene Duplikat-Prüfung nach angehängtem Import: Einträge mit Auswahl
/// plus endgültig abgelehnte Paare (kein Match) plus frische IDs und
/// manuelle Trefferwahl (Suchbegriff + je eine Auswahl je Seite).
pub struct MergeReview {
    pub candidates: Vec<MergeReviewEntry>,
    pub rejected: Vec<(String, String)>,
    /// Frisch angehängte IDs (rechte Seite „Neu" der manuellen Suche).
    pub fresh_ids: HashSet<String>,
    /// Datenstand vor dem Anhängen (nur Import): Abbrechen stellt ihn exakt
    /// wieder her (angehängte Personen + bereits zusammengeführte Änderungen
    /// verwerfen). Beim Projekt-Scan `None` (nichts angehängt).
    pub pre_import: Option<TreeData>,
    /// Suchbegriff der manuellen Trefferwahl (beide Seiten).
    pub manual_query: String,
    /// Manuell gewählt: Bestand (links) und Neu (rechts).
    pub manual_keep: Option<String>,
    pub manual_drop: Option<String>,
}

/// Schritt im geführten Import-Abgleich: Fixpunkt wählen, Baum-Overlay
/// prüfen, Auto-Ergänzung prüfen, Unterschiede paarweise durchgehen, fertig.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WizardStep {
    Anchor,
    Overlay,
    Supplement,
    Review,
    Done,
}

/// Eine Zuordnung im Wizard (aus dem 1:1-Mapping): `exact` = bereits
/// automatisch eingebettet, sonst im Review zu prüfen. `distance` =
/// Graphdistanz zum Fixpunkt im Anhang (Review-Reihenfolge).
pub struct WizardMapping {
    pub keep_id: String,
    pub drop_id: String,
    pub exact: bool,
    pub distance: usize,
    pub name_score: f32,
    pub family_score: f32,
    pub kin_score: f32,
}

/// Zustand des geführten Import-Abgleichs (Modal, siehe
/// `dialogs::show_import_wizard`).
pub struct ImportWizard {
    pub step: WizardStep,
    pub fresh_ids: HashSet<String>,
    pub fresh_total: usize,
    pub pre_import: TreeData,
    pub anchor_options: Vec<MergeCandidate>,
    pub anchor_query: String,
    pub anchor_keep: Option<String>,
    pub anchor_drop: Option<String>,
    pub mappings: Vec<WizardMapping>,
    /// Bereits eingebettete Drops (drop→keep): für Familien-Paarung auch nach
    /// dem Auto-Schritt noch auflösbar.
    pub merged: HashMap<String, String>,
    /// Übersprungene Diffs im Review (nicht übernommen = behalten):
    /// Skalare (drop,key), Listen (drop,art,schlüssel), Beziehungen als
    /// Rel-Schlüssel (drop-familie:art). Angewandte Diffs verschwinden von
    /// selbst.
    pub skipped_scalars: Vec<(String, String)>,
    pub skipped_lists: Vec<(String, String, String)>,
    pub skipped_rels: Vec<String>,
    pub review_index: usize,
    pub protocol: Vec<String>,
    pub auto_count: usize,
}

/// Angefragter Personenwechsel während offener ungespeicherter Bearbeitung.
/// Mit `image` wird nach dem Wechsel direkt der Editor mit dem Bild als
/// neuem Profilbild geöffnet (Bild-Drop auf eine Baumkarte).
#[derive(Clone, Debug)]
pub struct PendingSelect {
    pub target: String,
    pub set_reference: bool,
    pub image: Option<PathBuf>,
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

#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CardLayout {
    Compact,
    Portrait,
}

impl CardLayout {
    pub(crate) fn height(self) -> f32 {
        match self {
            Self::Compact => 78.0,
            // Platz für Geburts- UND Sterbezeile untereinander.
            Self::Portrait => 178.0,
        }
    }
}

pub struct MiniGramps {
    pub data: TreeData,
    /// Angezeigte Person (rechte Seitenleiste). Nur Ansicht, kein Baum-Effekt.
    pub selected: Option<String>,
    /// Mehrfachauswahl per Strg+Klick im Baum (Auswahlreihenfolge, Letzte =
    /// `selected`): Letzte grün gefüllt (links angezeigt), Rest dick grün
    /// umrandet. Einfacher Klick löst auf Einfachauswahl auf.
    pub multi_select: Vec<String>,
    /// Referenzperson = Wurzel des Stammbaums (`tree::draw_tree`).
    pub reference: Option<String>,
    /// Verlauf besuchter Referenzpersonen (Zurück/Vor-Pfeile in der
    /// Titelleiste). Neue Referenzen hängen hinten an; Zurück/Vor wandern
    /// entlang des Verlaufs.
    pub reference_history: Vec<String>,
    /// Aktuelle Position in `reference_history`.
    pub reference_history_index: usize,
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
    /// Latch gegen Mehrfach-Tausch beim Partner-Umsortieren: Wird nach einem
    /// Tausch gesetzt und erst beim Loslassen zurückgesetzt, damit desselbe
    /// Ziehen nicht pro Frame erneut tauscht (Flicker).
    pub partner_swap_latch: bool,
    /// Maus-Status vom Vorgängerframe zur Erkennung von Drag-Start/Ende.
    pub drag_mouse_was_down: bool,
    /// Standard-Generationenzahl (Einstellungen; 0 = alle).
    pub max_generations: usize,
    /// Personenbudget der automatischen Vorfahrenansicht und dessen
    /// persistierte Start-/Schrittwerte.
    pub tree_person_limit: usize,
    pub tree_initial_person_limit: usize,
    pub tree_load_step: usize,
    /// Treffer-Schwelle für den Duplikat-Abgleich beim Anhängen (0–100 %).
    pub match_threshold: f32,
    /// Warnstufe Sicherheit (aus den Optionen): Infos bis zu dieser Stufe
    /// werden in Baum und Seitenleiste farblich hervorgehoben. None = Aus.
    pub warn_certainty: Option<crate::model::Certainty>,
    /// Zoom-Faktor fürs Umschalten auf die Ganzfoto-Ansicht bei Personen MIT
    /// Foto (darunter Ganzfoto, darüber Avatar+Text).
    pub photo_full_zoom: f32,
    /// Dasselbe für Personen OHNE Foto (Initialen-Großansicht).
    pub initials_full_zoom: f32,
    /// Häufige Vornamen ab so vielen Gleichnamigen je Nachnamengruppe ohne
    /// Exakt-Boost (einstellbar im Prüfdialog, Default 4).
    pub common_given_threshold: usize,
    /// Offene Duplikat-Prüfung: Kandidaten mit Auswahl plus abgelehnte Paare.
    pub merge_review: Option<MergeReview>,
    /// Review-Dialog für Duplikate einblenden.
    pub show_merge_review: bool,
    /// Geführter Import-Abgleich (Fixpunkt-Wizard): Fixpunkt → Overlay →
    /// Auto-Ergänzung → Unterschieds-Review → Abschluss. `Some` = Modal offen.
    pub import_wizard: Option<ImportWizard>,
    /// Einzel-Merge-Dialog (rechte Leiste, Zusammenführen) einblenden.
    pub show_person_merge: bool,
    /// Suchtext im Einzel-Merge-Dialog (filtert Trefferliste zusätzlich).
    pub person_merge_query: String,
    /// Top-Treffer (ohne Selbst) für die aktuelle Person, nach Score sortiert.
    pub person_merge_hits: Vec<MergeCandidate>,
    /// Detail-Paar (keep, drop) im Einzel-Merge-Dialog.
    pub person_merge_detail: Option<(String, String)>,
    /// Feldwahl im Einzel-Merge-Detail (Geburt/Tod von Neu übernehmen).
    pub person_merge_take_birth: bool,
    pub person_merge_take_death: bool,
    /// Feldwahl im Einzel-Merge-Detail (Name von Neu übernehmen).
    pub person_merge_take_name: bool,
    /// Eltern-Mitmergen im Einzel-Merge-Detail: gewählter Elternteil je Seite.
    pub person_merge_parent_new: Option<String>,
    pub person_merge_parent_old: Option<String>,
    /// Schnellerfassungs-Dialog einblenden (aus dem Projektmenü).
    pub show_quick: bool,
    /// Richtung der Schnellerfassung (Abwärts: Partner + Kinder zur
    /// Referenz, Aufwärts: Eltern direkt zur Referenz).
    pub quick_dir: crate::model::QuickDir,
    /// Referenzperson der Schnellerfassung (aus dem Baum gewählt): Abwärts
    /// Partner-/Kinderanker, Aufwärts das Kind.
    pub quick_ref_id: Option<String>,
    /// Kopfzeile des aktuellen Blocks (nur Abwärts: Partner; Aufwärts
    /// blendet den Kopf aus und nutzt nur `quick_rows`).
    pub quick_head: crate::model::QuickPerson,
    /// Weitere Zeilen des aktuellen Blocks: Abwärts Kinder, Aufwärts Eltern
    /// (Elternteil 1 männlich, Elternteil 2 weiblich vorausgewählt).
    pub quick_rows: Vec<crate::model::QuickPerson>,
    /// FIFO-Warteschlange eingetragener Personen in Auswahl-/Zeilenreihenfolge:
    /// (Person, Partner?) — Kombis (abwärts: Kopf-Partner mit Referenz als
    /// Partner-Kontext) und Singles. Jede Person und jede Kombi liegt nur
    /// einmal vor; Kombi vorhanden ⇒ kein Single derselben Person mehr.
    /// Übernehmen setzt die Referenz der Reihe nach darauf.
    pub quick_queue: Vec<(String, Option<String>)>,
    /// Abwärts-Seite: Index des geladenen Partners (0..=Anzahl; letzte Seite
    /// ohne Partner). Mehrere Partner werden nacheinander durchgeblättert.
    pub quick_partner_idx: usize,
    /// Bereits als Referenz abgearbeitete Personen: landen nicht erneut auf
    /// der Queue (kein Pendeln zwischen Partnern).
    pub quick_visited: HashSet<String>,
    /// Detailansicht einer Inline-Suchtreffer-Person: (Ziel-Zeile, ID).
    /// Der +-Button bindet die Person an die Zeile.
    pub quick_detail: Option<(usize, String)>,
    /// Cache Vornamen-Token → Statistik-Geschlecht für den initialen
    /// Geschlechts-Default (wird bei Dialogstart und Speichern geleert).
    pub quick_gender_cache: std::collections::HashMap<String, Option<crate::model::Gender>>,
    /// Feld, das nach dem Hinzufügen einer Zeile den Fokus bekommen soll.
    pub quick_focus: Option<(usize, usize)>,
    /// Kartenabstand im automatischen Layout (Einstellungen; Standard 48 =
    /// doppelter ursprünglicher Abstand, damit der Vorfahrenbaum luftiger ist).
    pub layout_gap: f32,
    /// Extra-Abstand zwischen Nachbarkarten ohne Partner-Verbindung im
    /// Vorfahrenbaum (Einstellungen; Standard 30, 0 = kein Extra).
    pub non_partner_gap: f32,
    /// Fixe Kartenbreiten je Kartenlayout (Einstellungen; Kompakt 215,
    /// großes Foto 160). Namen werden bei Bedarf mit „…" gekürzt.
    pub compact_card_width: f32,
    pub portrait_card_width: f32,
    /// Symbole vor Geburts-/Todesdatum auf den Baumkarten (Einstellungen).
    pub birth_symbol: String,
    pub death_symbol: String,
    /// Datenordner: Speicherort (`save`) und Medien-Basisordner (`media`).
    pub library: PathBuf,
    pub status: String,
    pub zoom: f32,
    /// Zoom-Fit beim nächsten Frame ausführen (nach Laden/Referenzwechsel).
    pub fit_pending: bool,
    pub pan: Vec2,
    pub show_open: bool,
    pub show_settings: bool,
    /// Projekteigenschaften hinter dem Logo in der Titelleiste.
    pub show_project: bool,
    /// Projektname beim Beginn der Texteingabe, damit die komplette Änderung
    /// als ein Undo-Schritt gespeichert wird.
    pub project_name_before_edit: Option<String>,
    /// Auswahl der kuenftigen Exportformate, geoeffnet aus dem Projektfenster.
    pub show_export: bool,
    /// Laufender MFG-Export im Hintergrundthread (Ladebildschirm, siehe
    /// `dialogs::show_export_progress`): Dateiname + Ergebniskanal.
    pub export_progress: Option<ExportProgress>,
    /// Sortierung der Personenliste: nach Anzahl (true) oder Alphabet.
    pub group_by_count: bool,
    /// Suchtext der linken Personenliste (Sitzungszustand).
    pub people_filter: String,
    /// Ob zuletzt ein Suchfilter aktiv war (erkennt das Leeren der Suche).
    pub people_filter_was_active: bool,
    /// ID-Generation der Nachnamensgruppen: Erhöhen klappt alle wieder ein.
    pub people_group_generation: u32,
    /// Vorbereitete linke Personenliste; wird nur nach Daten- oder
    /// Sortieränderungen neu gruppiert und sortiert.
    pub people_groups: Vec<(String, Vec<(String, Gender, String, Option<i32>)>)>,
    pub people_groups_dirty: bool,
    /// Auswahl, für die das rechte Profil zuletzt an den Anfang gescrollt
    /// wurde (Scroll-Offset soll Personenwechsel nicht überleben).
    pub profile_shown_for: Option<String>,
    /// Bildschirm-Rechtecke der im letzten Baum-Frame gezeichneten
    /// Personenkarten (Personen-ID, Rechteck) für Datei-Drops auf Karten.
    pub tree_card_rects: Vec<(String, egui::Rect)>,
    /// Textur-Cache (`media::photo_texture`), Schlüssel = `Person::id`
    /// bzw. Galerie-Pseudo-IDs `gallery-<id>-<index>`.
    pub photo_cache: HashMap<String, TextureHandle>,
    pub tree_view: TreeView,
    /// Im Öffnen-Dialog ausgewähltes Projekt (wird mit "Laden" geöffnet).
    pub selected_project: Option<PathBuf>,
    /// Zwischengespeicherte Projektliste samt Anzeigenamen für den
    /// Öffnen-Dialog (kein Datei-IO pro Frame bei großen Projekten).
    pub project_list_cache: Vec<PathBuf>,
    pub project_list_names: Vec<String>,
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
    /// Verknüpfungs-Detailmodal (Vorschlag anklicken): (Bezugsperson, Kandidat, Art).
    pub link_detail: Option<(String, String, RelationKind)>,
    /// Nachname des Beziehungspickers.
    pub relation_family_name: String,
    /// Explizit gewähltes Geschlecht für neu anzulegende Beziehungspersonen
    /// (None = automatisch: Partner → Gegengeschlecht, sonst Unbekannt).
    pub new_person_gender: Option<Gender>,
    /// Wofür die explizite Wahl gilt (Art + Bezugsperson); bei Wechsel zurücksetzen.
    pub new_person_gender_for: Option<(RelationKind, String)>,
    /// Per Drag-and-drop eingefügte Datei mit unklarer Verwendung.
    pub pending_image: Option<PathBuf>,
    /// Gelesene Bild-Metadaten je Projektbild (Infozeile Bildbetrachter).
    pub photo_meta_cache: HashMap<String, crate::media::PhotoMeta>,
    /// Offene Foto-Auswahl (None = geschlossen, sonst vorhandene
    /// Projektbilder zur Wiederverwendung).
    pub photo_chooser: Option<Vec<String>>,
    /// True, wenn die Foto-Auswahl aus der Galerie-Ablage geöffnet wurde:
    /// Gewähltes landet dann in der Galerie, nicht als Profilbild.
    pub photo_chooser_gallery: bool,
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
    /// Geburtsdatum/-ort für ein neu anzulegendes Kind (Relationsbearbeitung).
    pub pending_child_birth: String,
    pub pending_child_birth_place: String,
    /// Offene Beziehungskategorie (`+`-Schalter, nur im Bearbeitungsmodus).
    pub relation_picker: Option<RelationKind>,
    /// Aufgeklappte Beziehungszeile im Beziehungseditor: Kategorie + ID der
    /// Verwandten, deren Beziehungsart bearbeitet wird.
    pub relation_editor: Option<(RelationKind, String)>,
    /// EINGEKLAPpte Kategorien der rechten Leiste (Sitzungszustand).
    pub collapsed_sections: HashSet<String>,
    /// Ausgeblendete Kategorien (Rechtsklick auf Kategorietitel → Häkchen).
    pub hidden_sections: HashSet<String>,
    /// Eingeblendete erweiterte Namensfelder (Titel, Spitzname, Rufname,
    /// Präfixe, Suffix) — Rechtsklick auf die Namenfelder. Befüllte Felder
    /// werden beim Betreten des Editmodus automatisch eingeblendet.
    pub name_fields_shown: HashSet<String>,
    /// Schließen angefordert, aber ungespeicherte Änderungen prüfen.
    pub pending_close: bool,
    /// Angefragte Personen-Auswahl bei laufender unsicherer Bearbeitung
    /// (Wechsel-Dialog in `dialogs::show_pending_select_confirm`).
    pub pending_select: Option<PendingSelect>,
    /// Tastatur-Auswahl im Wechsel-Dialog (0 = Speichern und wechseln
    /// [Default, hervorgehoben], 1 = Verwerfen, 2 = Abbrechen): Links/Rechts
    /// wechseln (Buttons liegen horizontal), Enter bestätigt.
    pub pending_select_choice: usize,
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
    /// HWND des Fensters (Windows) für die abgerundete Fensterform.
    window_hwnd: Option<isize>,
    /// Zuletzt angewandte Fensterform (Breite, Höhe, maximiert) – verhindert
    /// unnötige `SetWindowRgn`-Aufrufe pro Frame.
    window_region: Option<(i32, i32, bool)>,
    /// Zuletzt persistierte globale Einstellungen (Änderungserkennung).
    settings_applied: crate::settings::AppSettings,
    started: std::time::Instant,
}

/// Laufender MFG-Export im Hintergrundthread: angezeigter Dateiname plus
/// Kanal, über den der Thread `Ok`/`Err` meldet (`try_recv`-Polling pro
/// Frame, kein Blockieren der UI).
pub struct ExportProgress {
    pub file_name: String,
    pub rx: std::sync::mpsc::Receiver<Result<(), String>>,
}

impl MiniGramps {
    pub fn new() -> Self {
        let library = default_library();
        let _ = fs::create_dir_all(&library);
        let (lightbox_tx, lightbox_rx) = std::sync::mpsc::channel();
        let mut app = Self {
            data: TreeData::demo(),
            selected: Some("p5".into()),
            multi_select: vec!["p5".to_string()],
            reference: Some("p5".into()),
            reference_history: vec!["p5".to_string()],
            reference_history_index: 0,
            expanded: HashSet::new(),
            manual_offsets: HashMap::new(),
            long_press_used: false,
            card_drag: None,
            partner_swap_latch: false,
            drag_mouse_was_down: false,
            max_generations: 5,
            tree_person_limit: 60,
            tree_initial_person_limit: 60,
            tree_load_step: 60,
            match_threshold: 80.0,
            warn_certainty: None,
            photo_full_zoom: 0.8,
            initials_full_zoom: 0.8,
            common_given_threshold: 4,
            merge_review: None,
            show_merge_review: false,
            import_wizard: None,
            show_person_merge: false,
            person_merge_query: String::new(),
            person_merge_hits: Vec::new(),
            person_merge_detail: None,
            person_merge_take_birth: false,
            person_merge_take_death: false,
            person_merge_take_name: false,
            person_merge_parent_new: None,
            person_merge_parent_old: None,
            show_quick: false,
            quick_dir: crate::model::QuickDir::Down,
            quick_ref_id: None,
            quick_head: crate::model::QuickPerson::default(),
            quick_rows: Vec::new(),
            quick_queue: Vec::new(),
            quick_partner_idx: 0,
            quick_visited: HashSet::new(),
            quick_detail: None,
            quick_gender_cache: std::collections::HashMap::new(),
            quick_focus: None,
            layout_gap: 48.0,
            non_partner_gap: tree::UNMARRIED_GAP_EXTRA,
            compact_card_width: 215.0,
            portrait_card_width: 160.0,
            birth_symbol: "ᛉ".into(),
            death_symbol: "ᛦ".into(),
            library,
            status: "Beispielbaum geladen".into(),
            zoom: 1.0,
            fit_pending: false,
            pan: Vec2::ZERO,
            show_open: false,
            show_settings: false,
            show_project: false,
            project_name_before_edit: None,
            show_export: false,
            export_progress: None,
            group_by_count: true,
            people_filter: String::new(),
            people_filter_was_active: false,
            people_group_generation: 0,
            people_groups: Vec::new(),
            people_groups_dirty: true,
            profile_shown_for: None,
            tree_card_rects: Vec::new(),
            photo_cache: HashMap::new(),
            tree_view: TreeView::Descendants,
            selected_project: None,
            project_list_cache: Vec::new(),
            project_list_names: Vec::new(),
            dark_mode: true,
            card_layout: CardLayout::Compact,
            tree_orientation: TreeOrientation::Vertical,
            editing: None,
            draft: person("", "", "", "", Gender::Unknown),
            inline_edit: false,
            relation_query: String::new(),
            link_detail: None,
            relation_family_name: String::new(),
            new_person_gender: None,
            new_person_gender_for: None,
            pending_image: None,
            photo_meta_cache: HashMap::new(),
            photo_chooser: None,
            photo_chooser_gallery: false,
            lightbox_image: None,
            lightbox_tx,
            lightbox_rx,
            lightbox_loading: HashSet::new(),
            pending_child_for: None,
            pending_child_partner: None,
            pending_child_relation: ChildRelation::Birth,
            pending_child_birth: String::new(),
            pending_child_birth_place: String::new(),
            relation_picker: None,
            relation_editor: None,
            collapsed_sections: HashSet::new(),
            hidden_sections: HashSet::new(),
            name_fields_shown: HashSet::new(),
            pending_close: false,
            pending_select: None,
            pending_select_choice: 0,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            tree_tool: TreeTool::Cursor,
            zoom_selection_start: None,
            server_url: String::new(),
            server_token: String::new(),
            server_online: false,
            server_base: None,
            current_data_path: None,
            window_hwnd: None,
            window_region: None,
            settings_applied: crate::settings::AppSettings::default(),
            started: std::time::Instant::now(),
        };
        // Globale Einstellungen laden und anwenden (vor dem Projekt-Laden).
        let settings = crate::settings::load();
        app.dark_mode = settings.dark_mode;
        app.max_generations = settings.max_generations;
        app.tree_initial_person_limit = settings.tree_initial_person_limit.max(1);
        app.tree_load_step = settings.tree_load_step.max(1);
        app.match_threshold = settings.match_threshold.clamp(50.0, 100.0);
        app.warn_certainty = settings.warn_certainty;
        app.photo_full_zoom = settings.photo_full_zoom.clamp(0.2, 2.0);
        app.initials_full_zoom = settings.initials_full_zoom.clamp(0.2, 2.0);
        app.common_given_threshold = settings.common_given_threshold.clamp(2, 10);
        app.tree_person_limit = app.tree_initial_person_limit;
        app.group_by_count = settings.group_by_count;
        app.layout_gap = settings.layout_gap.clamp(5.0, 150.0);
        app.non_partner_gap = settings.non_partner_gap.clamp(0.0, 150.0);
        app.compact_card_width = settings.compact_card_width.clamp(120.0, 400.0);
        app.portrait_card_width = settings.portrait_card_width.clamp(120.0, 400.0);
        app.birth_symbol = settings.birth_symbol.clone();
        app.death_symbol = settings.death_symbol.clone();
        app.card_layout = settings.card_layout;
        app.tree_orientation = settings.tree_orientation;
        app.settings_applied = settings;
        app.log(format!(
            "Start. Datenordner (Speicherort): {}",
            app.library.display()
        ));
        // Zuletzt geöffnetes Projekt automatisch wiederherstellen; schlägt
        // das fehlen (Datei weg, Format unbekannt), bleibt der Beispielbaum.
        #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
        if let Some(path) = load_last_project() {
            app.log(format!(
                "Letzte Sitzung wiederherstellen: {}",
                path.display()
            ));
            app.load_path(&path);
        }
        app
    }

    /// Log: Terminal (`log::info!`) + Debug-Leiste (`panels::show_debug`).
    pub fn log(&mut self, message: impl Into<String>) {
        let line = format!(
            "[{:>9.3}s] {}",
            self.started.elapsed().as_secs_f32(),
            message.into()
        );
        log::info!("{line}");
    }

    /// Debug: alle zwischengespeicherten Vorschaubilder verwerfen (werden
    /// bei Bedarf neu aus den Originalen erzeugt).
    pub fn debug_clear_thumbs(&mut self) {
        let count = crate::media::delete_all_thumbs(&self.library);
        self.photo_cache.clear();
        self.status = format!("{count} Vorschaubilder gelöscht — werden neu erzeugt");
        self.log(format!("Debug: {count} Thumbnails gelöscht"));
    }

    /// Debug: runde Profilbilder aus den Originalen neu erzeugen (neue
    /// Zuschnitt-Logik für alle Bilder übernehmen).
    pub fn debug_rebuild_avatars(&mut self) {
        let count = crate::media::delete_avatar_thumbs(&self.library);
        self.photo_cache.clear();
        self.status = format!("{count} Profilbilder werden neu erzeugt");
        self.log(format!("Debug: {count} Avatar-Thumbs gelöscht"));
    }

    /// Debug: doppelte Beziehungen vereinen (dieselbe Beziehung genau einmal).
    /// Zuerst trocken zählen, dann Snapshot + Zusammenführen + Speichern.
    pub fn debug_dedupe_relationships(&mut self) {
        let mut probe = self.data.clone();
        let removed = probe.dedupe_relationships();
        if removed == 0 {
            self.status = "Keine doppelten Beziehungen gefunden".into();
            return;
        }
        self.snapshot("Doppelte Beziehungen entfernen");
        let removed = self.data.dedupe_relationships();
        self.people_groups_dirty = true;
        self.save();
        self.status = format!("Doppelte Beziehungen entfernt: {removed}");
        self.log(format!("Debug: {removed} doppelte Beziehungen entfernt"));
    }

    /// Projekt in den Datenordner schreiben (`<library>/familienbaum…json`).
    /// Zusätzlich wird das Baum-Layout in einer SEPARATEN Datei
    /// (`<projekt>.layout.json`) neben den Daten gespeichert: manuelle
    /// Verschiebungen, über IDs zugeordnet, relativ zum Eltern-Anker —
    /// angewendet in horizontaler wie vertikaler Ausrichtung.
    pub fn save(&mut self) {
        #[cfg(any(target_arch = "wasm32", target_os = "android"))]
        {
            self.status = "Speichern ist auf diesem Ziel noch nicht implementiert".into();
            self.log(self.status.clone());
            return;
        }

        #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
        {
        let entries: Vec<(String, f32)> = self
            .manual_offsets
            .iter()
            .map(|(id, offset)| (id.clone(), *offset))
            .collect();
        let path = match self.current_data_path.clone().filter(|path| {
            path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        }) {
            Some(path) => path,
            None => {
                match crate::projects::create(
                    &default_library().join("projects"), &self.data, &self.library,
                    &entries, self.reference.as_deref(),
                ) {
                    Ok(path) => self.load_path(&path),
                    Err(error) => self.status = format!("Projekt anlegen fehlgeschlagen: {error}"),
                }
                return;
            }
        };
        let store = FileSystemStore::for_data_file(&path);
        match store
            .write_data(&self.data)
            .and_then(|_| store.write_layout(&entries, self.reference.as_deref()))
        {
            Ok(_) => {
                save_last_project(&path);
                self.status = format!("Gespeichert: {}", path.display());
                self.log(format!("Gespeichert: {}", path.display()));
            }
            Err(e) => {
                self.status = format!("Speichern fehlgeschlagen: {e}");
                self.log(format!("Speichern fehlgeschlagen: {e}"));
                return;
            }
        }
        match save_project_manifest(&path, &self.data) {
            Ok(manifest) => self.log(format!("Manifest gespeichert: {}", manifest.display())),
            Err(error) => self.log(format!("Manifest speichern fehlgeschlagen: {error}")),
        }
        // Bereinigung nur innerhalb des eigenen Projektordners.
        if let Some(root) = path.parent() {
            crate::media::cleanup_unused_media(root, &self.data);
        }
        self.refresh_project_list();
        }
    }

    /// Manueller Dateidialog ("Datei manuell laden..." im Öffnen-Fenster).
    pub fn import_dialog(&mut self) {
        #[cfg(any(target_arch = "wasm32", target_os = "android"))]
        {
            self.status = "Import ist auf diesem Ziel noch nicht implementiert".into();
            self.log(self.status.clone());
            return;
        }

        #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
        {
        if let Some(path) = FileDialog::new()
            .add_filter(
                "Familien-Daten",
                &["json", "ged", "gedcom", "gramps", "xml"],
            )
            .pick_file()
        {
            self.log(format!("Manueller Ladeversuch: {}", path.display()));
            self.import_project(&path);
        }
        }
    }

    /// Import erstellt immer eine neue Projektkopie, niemals ein bestehendes Ziel.
    pub fn import_project(&mut self, source: &Path) {
        let result = (|| {
            let mut data = load_file(source)?;
            data.project.name = crate::import::project_display_name(source);
            let (offsets, reference) = FileSystemStore::for_data_file(source).read_layout();
            let offsets: Vec<_> = offsets.into_iter().collect();
            crate::projects::create(
                &default_library().join("projects"), &data,
                source.parent().unwrap_or(Path::new(".")),
                &offsets, reference.as_deref(),
            )
        })();
        match result {
            Ok(path) => self.load_path(&path),
            Err(error) => self.status = format!("Import fehlgeschlagen: {error}"),
        }
    }

    /// Aktuelles Projekt als Komplettpaket (.mfg) exportieren: Daten,
    /// Layout, Manifest und Medien in einer Datei (Zieldialog). Der ZIP-
    /// Aufbau läuft im Hintergrundthread — die UI zeigt solange einen
    /// Ladebildschirm statt einzufrieren (siehe `show_export_progress`).
    pub fn export_mfg_dialog(&mut self, ctx: &egui::Context) {
        #[cfg(any(target_arch = "wasm32", target_os = "android"))]
        {
            let _ = ctx;
            self.status = "Export ist auf diesem Ziel noch nicht implementiert".into();
            return;
        }

        #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
        {
            let Some(data_path) = self.current_data_path.clone() else {
                self.status = "Kein geöffnetes Projekt zum Exportieren".into();
                return;
            };
            // Der Export liest die Dateien von der Platte: offene
            // Bearbeitung erst übernehmen + sichern, sonst wäre das Paket
            // veraltet (Muster wie beim Speichern-Button).
            if self.inline_edit {
                self.commit_draft();
                self.inline_edit = false;
                self.save();
            }
            let default_name = data_path
                .parent()
                .and_then(|dir| dir.file_name())
                .and_then(|name| name.to_str())
                .map(|name| format!("{name}.mfg"))
                .unwrap_or_else(|| "projekt.mfg".to_string());
            if let Some(dest) = FileDialog::new()
                .add_filter("MiniGramps Full", &["mfg"])
                .set_file_name(&default_name)
                .save_file()
            {
                let file_name = dest
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("projekt.mfg")
                    .to_string();
                let (tx, rx) = std::sync::mpsc::channel();
                let repaint = ctx.clone();
                std::thread::spawn(move || {
                    let result = crate::projects::export_mfg(&data_path, &dest);
                    let _ = tx.send(result);
                    repaint.request_repaint();
                });
                self.export_progress = Some(ExportProgress { file_name, rx });
                self.show_export = false;
            }
        }
    }

    pub fn new_project(&mut self) {
        match crate::projects::create(
            &default_library().join("projects"), &TreeData::default(),
            &self.library, &[], None,
        ) {
            Ok(path) => self.load_path(&path),
            Err(error) => self.status = format!("Projekt anlegen fehlgeschlagen: {error}"),
        }
    }

    /// Datei ans AKTUELLE Projekt anhängen (statt ersetzen): IDs werden frisch
    /// vergeben, danach Abgleich (Duplikat-Verdacht ab Schwelle) ins Review.
    pub fn import_append_dialog(&mut self) {
        #[cfg(any(target_arch = "wasm32", target_os = "android"))]
        {
            self.status = "Import ist auf diesem Ziel noch nicht implementiert".into();
            return;
        }

        #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
        {
            if let Some(path) = FileDialog::new()
                .add_filter(
                    "Familien-Daten",
                    &["json", "ged", "gedcom", "gramps", "xml", "mfg"],
                )
                .pick_file()
            {
                self.import_and_match(&path);
            }
        }
    }

    /// Datei laden, anhängen und Duplikate erkennen (Review-Dialog).
    /// Vorher wird der Projektordner gesichert (Wiederherstellung im
    /// Projektfenster bei fehlerhaftem Import).
    pub fn import_and_match(&mut self, source: &Path) {
        // Komplettpakete zuerst in ein temporäres Verzeichnis entpacken.
        let temp_root = std::env::temp_dir().join(format!("minigramps-mfg-{}", std::process::id()));
        let (json_source, media_base, cleanup_temp) = if source
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("mfg"))
        {
            let _ = std::fs::remove_dir_all(&temp_root);
            match crate::projects::import_mfg(&temp_root, source) {
                Ok(json) => {
                    let base = json.parent().unwrap_or(Path::new(".")).to_path_buf();
                    (json, base, true)
                }
                Err(error) => {
                    self.status = format!("Import fehlgeschlagen: {error}");
                    return;
                }
            }
        } else {
            (
                source.to_path_buf(),
                source.parent().unwrap_or(Path::new(".")).to_path_buf(),
                false,
            )
        };
        let imported = match load_file(&json_source) {
            Ok(data) => data,
            Err(error) => {
                if cleanup_temp {
                    let _ = std::fs::remove_dir_all(&temp_root);
                }
                self.status = format!("Import fehlgeschlagen: {error}");
                return;
            }
        };
        if let Some(project_dir) = self
            .current_data_path
            .clone()
            .and_then(|path| path.parent().map(Path::to_path_buf))
        {
            match crate::projects::backup_project(&default_library(), &project_dir) {
                Ok(backup) => self.log(format!("Sicherung angelegt: {}", backup.display())),
                Err(error) => self.log(format!("Sicherung fehlgeschlagen: {error}")),
            }
        }
        let name = crate::import::project_display_name(source);
        self.snapshot(format!("Import anhängen: {name}"));
        // Pre-Import-Stand für Abbrechen sichern (exaktes Zurückrollen).
        let pre_import = self.data.clone();
        let fresh: HashSet<String> = self.data.append_import(imported).into_iter().collect();
        // Medien der frischen Personen ins eigene Projekt kopieren (Daten
        // verweisen sonst auf den Quellordner bzw. das Temp-Verzeichnis).
        if let Some(project_dir) = self
            .current_data_path
            .clone()
            .and_then(|path| path.parent().map(Path::to_path_buf))
        {
            let fresh_ids: Vec<String> = fresh.iter().cloned().collect();
            if let Err(error) = crate::projects::rebase_media_files(
                &mut self.data,
                &fresh_ids,
                &media_base,
                &project_dir,
            ) {
                self.log(format!("Medienkopie unvollständig: {error}"));
            }
        }
        if cleanup_temp {
            let _ = std::fs::remove_dir_all(&temp_root);
        }
        // Geführter Abgleich (Fixpunkt-Wizard): Fixpunkt wählen → Overlay →
        // Auto-Ergänzung → Unterschiede paarweise prüfen → Abschluss.
        self.people_groups_dirty = true;
        self.photo_cache.clear();
        self.fit_pending = true;
        self.log(format!(
            "Import angehängt: {} ({} neu)",
            source.display(),
            fresh.len()
        ));
        self.open_import_wizard(pre_import, fresh);
    }

    /// true, sobald irgendein modales Dialogfenster offen ist. Offene
    /// Modals fangen Klicks ab: Der dahinterliegende Baum darf dann nicht
    /// gleichzeitig reagieren (Click-through) — weder Karten-Klicks noch
    /// Drag/Pan/Zoom noch Datei-Drops. Der Baum liest rohen Pointer-State,
    /// den egui-Layer allein nicht sperren; daher fragen alle Baum-
    /// Eingabestellen dieses Prädikat ab (siehe tree.rs `input_blocked`).
    pub fn any_modal_open(&self) -> bool {
        self.show_quick
            || self.show_merge_review
            || self.import_wizard.is_some()
            || self.show_person_merge
            || self.link_detail.is_some()
            || self.show_project
            || self.show_export
            || self.export_progress.is_some()
            || self.show_settings
            || self.show_open
            || self.pending_close
            || self.pending_image.is_some()
            || self.photo_chooser.is_some()
            || self.pending_select.is_some()
            || self.lightbox_image.is_some()
    }

    /// Schnellerfassung öffnen (über das Projektmenü). Die Referenz ist die
    /// aktuell gewählte Person im Baum; die Mehrfachauswahl liegt schon auf
    /// dem Stack (pro Person Kombi mit erstem Partner + Partner-Single,
    /// ohne Partner als Single).
    pub fn open_quick_entry(&mut self) {
        self.quick_dir = crate::model::QuickDir::Down;
        // Referenz = erste Auswahl, Rest in Auswahlreihenfolge auf den Stack —
        // so stimmt die Abarbeitung mit der Auswahl überein (nicht invers).
        let ordered: Vec<String> = self
            .multi_select
            .iter()
            .filter(|id| self.data.find(id).is_some())
            .cloned()
            .collect();
        self.quick_ref_id = ordered.first().cloned().or_else(|| {
            self.selected
                .clone()
                .or_else(|| self.reference.clone())
                .filter(|id| self.data.find(id).is_some())
        });
        self.quick_queue = Vec::new();
        self.quick_partner_idx = 0;
        self.quick_visited.clear();
        if let Some(ref_id) = self.quick_ref_id.clone() {
            self.quick_visited.insert(ref_id);
        }
        // Rest der Auswahl in Reihenfolge auf den Stack: pro Person Kombi
        // (Person, erster Partner) + Partner als Single; ohne Partner als
        // Single. Duplikate (z. B. beidseitig gewählt) einmalig.
        let mut queue: Vec<(String, Option<String>)> = Vec::new();
        let mut push_unique = |entry: (String, Option<String>)| {
            if !queue.contains(&entry) {
                queue.push(entry);
            }
        };
        for pid in ordered.iter().skip(1) {
            if let Some(first) = self
                .data
                .partners_of(pid)
                .first()
                .map(|person| person.id.clone())
            {
                push_unique((pid.clone(), Some(first.clone())));
                if Some(first.as_str()) != self.quick_ref_id.as_deref() {
                    push_unique((first, None));
                }
            } else {
                push_unique((pid.clone(), None));
            }
        }
        self.quick_queue = queue;
        // Bestehende Kinder/Eltern der Referenz stehen gebunden in den Zeilen.
        dialogs::quick_load_reference(self);
        self.show_quick = true;
    }

    /// Aktuelle Schnellerfassungs-Eingabe sofort speichern (wie Import-
    /// Verknüpfung): bestehende gebundene Personen wiederverwenden, neue
    /// anlegen. Gibt (verarbeitet, neu angelegte IDs) zurück. Mit `queue`
    /// landen die Betroffenen auf der FIFO-Queue: abwärts der Kopf als Kombi
    /// (Referenz, Partner) plus Kopf als Single, Zeilen als Singles
    /// (aufwärts nur Zeilen-Singles). Nur bei nicht-leerer Eingabe folgen
    /// Snapshot + Speichern (ein Undo-Schritt je Aufruf).
    pub fn persist_quick_form(&mut self, queue: bool) -> (bool, Vec<String>) {
        // Laufenden Block übernehmen, sofern nicht komplett leer. Aufwärts
        // gibt es keinen Kopf (die Referenz ist das Kind).
        let head_empty = self.quick_head.is_empty() && self.quick_head.bind.is_none();
        let rows_empty = self
            .quick_rows
            .iter()
            .all(|row| row.is_empty() && row.bind.is_none());
        if head_empty && rows_empty {
            return (false, Vec::new());
        }
        let head = if self.quick_dir == crate::model::QuickDir::Up
            || (self.quick_head.is_empty() && self.quick_head.bind.is_none())
        {
            None
        } else {
            Some(self.quick_head.clone())
        };
        let rows = self.quick_rows.clone();
        // Slots in Zeilenreihenfolge (Kopf zuerst): gebundene IDs sind sofort
        // bekannt, neue werden nach dem Commit zugeordnet.
        enum Slot {
            Bound(String),
            New,
            Skip,
        }
        let slot_of = |quick: &crate::model::QuickPerson, data: &crate::model::TreeData| {
            if let Some(id) = quick.bind.as_deref() {
                if data.find(id).is_some() {
                    Slot::Bound(id.to_string())
                } else {
                    Slot::Skip
                }
            } else if quick.is_empty() {
                Slot::Skip
            } else {
                Slot::New
            }
        };
        let mut slots: Vec<(bool, Slot)> = Vec::new();
        if let Some(ref head_person) = head {
            slots.push((true, slot_of(head_person, &self.data)));
        }
        for row in &rows {
            slots.push((false, slot_of(row, &self.data)));
        }
        let is_down = self.quick_dir == crate::model::QuickDir::Down;
        let ref_id = self.quick_ref_id.clone();
        // Gebundene Zeilen: editierte Vorname/Geburt/Tod fürs Zurückschreiben
        // merken (Nachname/Geschlecht bleiben Anker und unangetastet).
        let mut write_back: Vec<(String, String, String, String)> = Vec::new();
        if let Some(ref head_person) = head {
            if let Some(id) = head_person.bind.as_deref() {
                if self.data.find(id).is_some() {
                    write_back.push((
                        id.to_string(),
                        head_person.given.trim().to_string(),
                        head_person.birth.trim().to_string(),
                        head_person.death.trim().to_string(),
                    ));
                }
            }
        }
        for row in &rows {
            if let Some(id) = row.bind.as_deref() {
                if self.data.find(id).is_some() {
                    write_back.push((
                        id.to_string(),
                        row.given.trim().to_string(),
                        row.birth.trim().to_string(),
                        row.death.trim().to_string(),
                    ));
                }
            }
        }
        let created = self
            .data
            .commit_quick_blocks(self.quick_ref_id.as_deref(), vec![(self.quick_dir, head, rows)]);
        for (id, given, birth, death) in write_back {
            if let Some(person) = self.data.people.iter_mut().find(|person| person.id == id) {
                person.given_name = given;
                person.birth = birth;
                person.death = death;
            }
        }
        // Slots auflösen: Kopf-ID + Zeilen-IDs (Modell legt neue
        // Block-Personen in Kopf→Zeilen-Reihenfolge an).
        let mut created_iter = created.iter();
        let mut head_id: Option<String> = None;
        let mut row_ids: Vec<String> = Vec::new();
        for (is_head, slot) in slots {
            let resolved = match slot {
                Slot::Bound(id) => Some(id),
                Slot::New => created_iter.next().cloned(),
                Slot::Skip => None,
            };
            match (is_head, resolved) {
                (true, Some(id)) => head_id = Some(id),
                (false, Some(id)) => row_ids.push(id),
                _ => {}
            }
        }
        if queue {
            // Kopf als Kombi (Referenz, Partner): Richtung exakt einmalig.
            // Kopf zusätzlich als Single (eigene Runde), Zeilen als Singles.
            // Ohne Referenz nur Singles (kein Kombi-Kontext).
            if is_down {
                match (ref_id, head_id) {
                    (Some(ref_id), Some(head)) => {
                        self.queue_combo(ref_id, head.clone(), &created);
                        self.queue_single(head);
                    }
                    (_, Some(head)) => self.queue_single(head),
                    _ => {}
                }
            }
            for id in row_ids {
                self.queue_single(id);
            }
        }
        self.quick_rows.clear();
        self.quick_detail = None;
        self.quick_head = crate::model::QuickPerson::default();
        self.quick_gender_cache.clear();
        self.people_groups_dirty = true;
        self.photo_cache.clear();
        self.snapshot("Schnellerfassung");
        self.save();
        (true, created)
    }

    /// Single (Person, None) auf den Stack: nur wenn weder besucht noch in
    /// einer Form (Single oder Kombi) vorhanden.
    fn queue_single(&mut self, id: String) {
        if self.quick_visited.iter().any(|visited| visited == &id) {
            return;
        }
        if self.quick_queue.iter().any(|(pid, _)| pid == &id) {
            return;
        }
        self.quick_queue.push((id, None));
    }

    /// Kombi (Referenz, Partner) auf den Stack: Richtung exakt einmalig; eine
    /// vorhandene Single derselben Referenz steigt zur Kombi auf. Bereits
    /// abgearbeitete Referenzen mit bekanntem Partner werden übersprungen
    /// (kein Pendeln); neuer Partner reaktiviert.
    fn queue_combo(&mut self, person: String, partner: String, created: &[String]) {
        if self
            .quick_queue
            .iter()
            .any(|(pid, ppartner)| pid == &person && ppartner.as_deref() == Some(partner.as_str()))
        {
            return;
        }
        let partner_is_new = created.iter().any(|id| id == &partner);
        if !partner_is_new && self.quick_visited.iter().any(|visited| visited == &person) {
            return;
        }
        self.quick_queue.retain(|(pid, _)| pid != &person);
        self.quick_queue.push((person, Some(partner)));
    }

    /// Schnellerfassung abschließen: Eingabe speichern und Fenster schließen.
    /// Bei leerer Eingabe (bereits per Übernehmen gespeichert) nur schließen.
    pub fn commit_quick_entry(&mut self) {
        let (processed, created) = self.persist_quick_form(false);
        if processed {
            self.status = format!("Schnellerfassung: {} neue Personen", created.len());
            self.log(format!("Schnellerfassung: {} neue Personen", created.len()));
        }
        self.quick_queue.clear();
        self.show_quick = false;
    }

    /// Ausgewählte Review-Treffer zusammenführen (Daten + Layoutversatz).
    pub fn apply_merge_review(&mut self) {
        let selected: Vec<(String, String, bool, bool, bool)> = self
            .merge_review
            .as_ref()
            .map(|review| {
                review
                    .candidates
                    .iter()
                    .filter(|entry| entry.selected)
                    .map(|entry| {
                        (
                            entry.candidate.keep_id.clone(),
                            entry.candidate.drop_id.clone(),
                            entry.take_new_birth,
                            entry.take_new_death,
                            entry.take_new_name,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        if selected.is_empty() {
            return;
        }
        for (keep, drop, take_birth, take_death, take_name) in &selected {
            self.merge_one(keep, drop, *take_birth, *take_death, *take_name);
        }
        if let Some(review) = self.merge_review.as_mut() {
            let done: HashSet<String> = selected
                .iter()
                .map(|(_, drop, _, _, _)| drop.clone())
                .collect();
            review
                .candidates
                .retain(|entry| !done.contains(&entry.candidate.drop_id));
        }
        self.people_groups_dirty = true;
        self.fix_selection_after_merge();
        self.reference_history.clear();
        self.reset_reference_navigation();
        self.status = format!("{} Treffer zusammengeführt", selected.len());
        self.log(format!("Merge: {} Treffer", selected.len()));
    }

    /// Einen Treffer zusammenführen (Daten + Layoutversatz + Fotocache).
    fn merge_one(&mut self, keep: &str, drop: &str, take_birth: bool, take_death: bool, take_name: bool) {
        self.data.apply_merge_choice(keep, drop, take_birth, take_death, take_name);
        if !self.manual_offsets.contains_key(keep) {
            if let Some(offset) = self.manual_offsets.remove(drop) {
                self.manual_offsets.insert(keep.to_string(), offset);
            }
        } else {
            self.manual_offsets.remove(drop);
        }
        crate::media::clear_person_photo_cache(&mut self.photo_cache, keep);
    }

    /// Geführten Import-Abgleich öffnen (nach dem Anhängen): Fixpunkt-Schritt
    /// mit den besten Kandidaten (erster vorausgewählt).
    pub fn open_import_wizard(&mut self, pre_import: TreeData, fresh: HashSet<String>) {
        let threshold = (self.match_threshold / 100.0).clamp(0.0, 1.0);
        let mut options = self
            .data
            .find_merge_candidates(&fresh, threshold, self.common_given_threshold);
        options.truncate(30);
        let (anchor_keep, anchor_drop) = options
            .first()
            .map(|candidate| {
                (
                    Some(candidate.keep_id.clone()),
                    Some(candidate.drop_id.clone()),
                )
            })
            .unwrap_or((None, None));
        let fresh_total = fresh.len();
        self.import_wizard = Some(ImportWizard {
            step: WizardStep::Anchor,
            fresh_ids: fresh,
            fresh_total,
            pre_import,
            anchor_options: options,
            anchor_query: String::new(),
            anchor_keep,
            anchor_drop,
            mappings: Vec::new(),
            merged: HashMap::new(),
            skipped_scalars: Vec::new(),
            skipped_lists: Vec::new(),
            skipped_rels: Vec::new(),
            review_index: 0,
            protocol: Vec::new(),
            auto_count: 0,
        });
        self.status = "Abgleich starten: Fixpunkt wählen".to_string();
    }

    /// Fixpunkt bestätigen → 1:1-Mapping aufbauen + Distanzen ab Fixpunkt,
    /// weiter zum Overlay-Schritt.
    pub fn wizard_confirm_anchor(&mut self) {
        let threshold = (self.match_threshold / 100.0).clamp(0.0, 1.0);
        let (anchor_keep, anchor_drop, fresh) = match self.import_wizard.as_ref() {
            Some(wizard)
                if wizard.anchor_keep.is_some() && wizard.anchor_drop.is_some() =>
            {
                (
                    wizard.anchor_keep.clone().unwrap(),
                    wizard.anchor_drop.clone().unwrap(),
                    wizard.fresh_ids.clone(),
                )
            }
            _ => return,
        };
        let mapping = self
            .data
            .build_import_mapping(&fresh, threshold, self.common_given_threshold);
        let distances = self.data.import_distances(&anchor_drop, &fresh);
        // Fixpunkt-Paar sicher ins Mapping (auch unter der Schwelle als
        // manuelle Setzung möglich): vorne anstellen.
        let mut mappings: Vec<WizardMapping> = mapping
            .into_iter()
            .map(|entry| {
                let distance = distances.get(&entry.drop_id).copied().unwrap_or(usize::MAX);
                WizardMapping {
                    keep_id: entry.keep_id,
                    drop_id: entry.drop_id,
                    exact: entry.exact,
                    distance,
                    name_score: entry.name_score,
                    family_score: entry.family_score,
                    kin_score: entry.kin_score,
                }
            })
            .collect();
        if !mappings
            .iter()
            .any(|entry| entry.keep_id == anchor_keep && entry.drop_id == anchor_drop)
            && self.data.find(&anchor_keep).is_some()
            && self.data.find(&anchor_drop).is_some()
        {
            mappings.push(WizardMapping {
                keep_id: anchor_keep.clone(),
                drop_id: anchor_drop.clone(),
                exact: false,
                distance: 0,
                name_score: 1.0,
                family_score: 1.0,
                kin_score: 1.0,
            });
        }
        mappings.sort_by_key(|entry| (entry.distance, !entry.exact));
        if let Some(wizard) = self.import_wizard.as_mut() {
            wizard.mappings = mappings;
            wizard.review_index = 0;
            wizard.step = WizardStep::Overlay;
        }
        self.status = "Overlay aufgebaut — bitte prüfen".to_string();
    }

    /// Auto-Ergänzung: exakte Paare einbetten (nur Lücken füllen) +
    /// Protokoll; eingebettete Drops aus der Frisch-Menge streichen.
    pub fn wizard_run_supplement(&mut self) {
        let exact: Vec<(String, String)> = match self.import_wizard.as_ref() {
            Some(wizard) => wizard
                .mappings
                .iter()
                .filter(|entry| entry.exact)
                .map(|entry| (entry.keep_id.clone(), entry.drop_id.clone()))
                .collect(),
            None => return,
        };
        let mut count = 0usize;
        for (keep, drop) in exact {
            if self.data.find(&keep).is_none() || self.data.find(&drop).is_none() {
                continue;
            }
            let mut messages = self.data.supplement_exact_pair(&keep, &drop);
            if !self.manual_offsets.contains_key(&keep) {
                if let Some(offset) = self.manual_offsets.remove(&drop) {
                    self.manual_offsets.insert(keep.clone(), offset);
                }
            } else {
                self.manual_offsets.remove(&drop);
            }
            crate::media::clear_person_photo_cache(&mut self.photo_cache, &keep);
            if messages.is_empty() {
                if let Some(name) = self.data.find(&keep).map(|p| p.display_name()) {
                    messages.push(format!("{name}: bereits vollständig — nichts zu ergänzen"));
                }
            }
            if let Some(wizard) = self.import_wizard.as_mut() {
                wizard.protocol.extend(messages);
                wizard.fresh_ids.remove(&drop);
                wizard.merged.insert(drop, keep.clone());
            }
            count += 1;
        }
        if let Some(wizard) = self.import_wizard.as_mut() {
            wizard.auto_count += count;
            wizard.mappings.retain(|entry| !entry.exact);
            // Review-Reihenfolge: Fixpunkt-nah zuerst.
            wizard.mappings.sort_by_key(|entry| entry.distance);
            wizard.review_index = 0;
            wizard.step = WizardStep::Supplement;
        }
        self.people_groups_dirty = true;
        self.fix_selection_after_merge();
        self.status = format!("{count} Paare automatisch eingebettet");
    }

    /// Ohne Fixpunkt fortfahren (keine Treffer): leeres Mapping, alles Neue
    /// bleibt angehängt — direkt zum Overlay-Schritt.
    pub fn wizard_skip_anchor(&mut self) {
        if let Some(wizard) = self.import_wizard.as_mut() {
            wizard.mappings = Vec::new();
            wizard.review_index = 0;
            wizard.step = WizardStep::Overlay;
        }
        self.status = "Kein Fixpunkt — alles als neu".to_string();
    }

    /// Drop→Keep für Familien-Paarung: offene Mappings + bereits eingebettete.
    pub fn wizard_drop_to_keep(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        if let Some(wizard) = self.import_wizard.as_ref() {
            for entry in &wizard.mappings {
                map.insert(entry.drop_id.clone(), entry.keep_id.clone());
            }
            for (drop, keep) in &wizard.merged {
                map.insert(drop.clone(), keep.clone());
            }
        }
        map
    }

    /// Review-Paare in Durchlauf-Reihenfolge (nur unsichere, Fixpunkt-nah).
    pub fn wizard_review_order(&self) -> Vec<(String, String)> {
        self.import_wizard
            .as_ref()
            .map(|wizard| {
                let mut pairs: Vec<(String, String, usize)> = wizard
                    .mappings
                    .iter()
                    .filter(|entry| !entry.exact)
                    .map(|entry| {
                        (
                            entry.keep_id.clone(),
                            entry.drop_id.clone(),
                            entry.distance,
                        )
                    })
                    .collect();
                pairs.sort_by_key(|(_, _, distance)| *distance);
                pairs
                    .into_iter()
                    .map(|(keep, drop, _)| (keep, drop))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Wizard abschließen: Zähler-Protokoll in Status/Log, Dialog zu,
    /// speichern. Eingebettete + bestätigte Änderungen bleiben bestehen.
    pub fn wizard_finish(&mut self) {
        let (auto_count, review_total, fresh_left, fresh_total, protocol) =
            match self.import_wizard.as_ref() {
                Some(wizard) => (
                    wizard.auto_count,
                    wizard.mappings.len(),
                    wizard.fresh_ids.len(),
                    wizard.fresh_total,
                    wizard.protocol.clone(),
                ),
                None => return,
            };
        let new_count = fresh_left;
        self.import_wizard = None;
        self.people_groups_dirty = true;
        self.photo_cache.clear();
        self.fit_pending = true;
        self.fix_selection_after_merge();
        self.reference_history.clear();
        self.reset_reference_navigation();
        self.save();
        self.status = format!(
            "Abgleich fertig: {auto_count} automatisch, {review_total} geprüft, {new_count} neu von {fresh_total}"
        );
        self.log(format!(
            "Import-Abgleich: auto={auto_count} review={review_total} neu={new_count}"
        ));
        for line in protocol {
            self.log(format!("Abgleich: {line}"));
        }
    }

    /// Wizard abbrechen: Stand vor dem Anhängen exakt wiederherstellen
    /// (angehängte Personen + Einbettungen verwerfen), dann Snapshot +
    /// Speichern (Strg+Z holt den Anhang zurück), Dialog schließen.
    pub fn wizard_discard(&mut self) {
        let pre_import = match self.import_wizard.as_ref() {
            Some(wizard) => wizard.pre_import.clone(),
            None => return,
        };
        self.data = pre_import;
        self.manual_offsets
            .retain(|id, _| self.data.find(id).is_some());
        self.import_wizard = None;
        self.merge_review = None;
        self.show_merge_review = false;
        self.people_groups_dirty = true;
        self.photo_cache.clear();
        self.fit_pending = true;
        if self
            .reference
            .as_deref()
            .is_some_and(|id| self.data.find(id).is_none())
        {
            self.reference = self.data.people.first().map(|person| person.id.clone());
            self.selected = self.reference.clone();
        }
        if self
            .selected
            .as_deref()
            .is_some_and(|id| self.data.find(id).is_none())
        {
            self.selected = self.data.people.first().map(|person| person.id.clone());
        }
        self.snapshot("Anhang verworfen");
        self.save();
        self.status = "Anhang verworfen".to_string();
        self.log("Anhang verworfen (Abgleich abgebrochen)".to_string());
    }

    /// Auswahl nach dem Zusammenführen an die Daten angleichen (gelöschte
    /// Duplikate auf die erste Person umbiegen).
    fn fix_selection_after_merge(&mut self) {
        if self
            .reference
            .as_deref()
            .is_some_and(|id| self.data.find(id).is_none())
        {
            self.reference = self.data.people.first().map(|person| person.id.clone());
            self.selected = self.reference.clone();
        }
        if self
            .selected
            .as_deref()
            .is_some_and(|id| self.data.find(id).is_none())
        {
            self.selected = self.data.people.first().map(|person| person.id.clone());
        }
    }

    /// Treffer endgültig ablehnen (kein Match — bleibt getrennt, kein
    /// erneuter Vorschlag in dieser Prüfung).
    pub fn reject_merge_candidate(&mut self, drop_id: &str) {
        if let Some(review) = self.merge_review.as_mut() {
            if let Some(position) = review
                .candidates
                .iter()
                .position(|entry| entry.candidate.drop_id == drop_id)
            {
                let entry = review.candidates.remove(position);
                review
                    .rejected
                    .push((entry.candidate.keep_id, entry.candidate.drop_id));
            }
        }
    }

    /// Anhang abbrechen (nur Import-Review): Datenstand vor dem Anhängen exakt
    /// wiederherstellen — angehängte Personen und bereits zusammengeführte
    /// Änderungen werden verworfen. Danach Snapshot + Speichern (Strg+Z holt
    /// den Anhang zurück), Dialog schließen.
    pub fn discard_appended_import(&mut self) {
        let pre_import = match self
            .merge_review
            .as_ref()
            .and_then(|review| review.pre_import.clone())
        {
            Some(data) => data,
            None => return,
        };
        self.data = pre_import;
        // Verwaiste Layoutversätze frischer IDs räumen.
        self.manual_offsets
            .retain(|id, _| self.data.find(id).is_some());
        self.merge_review = None;
        self.show_merge_review = false;
        self.people_groups_dirty = true;
        self.photo_cache.clear();
        self.fit_pending = true;
        // Auswahl an die wiederhergestellten Daten angleichen.
        if self
            .reference
            .as_deref()
            .is_some_and(|id| self.data.find(id).is_none())
        {
            self.reference = self.data.people.first().map(|person| person.id.clone());
            self.selected = self.reference.clone();
        }
        if self
            .selected
            .as_deref()
            .is_some_and(|id| self.data.find(id).is_none())
        {
            self.selected = self.data.people.first().map(|person| person.id.clone());
        }
        self.snapshot("Anhang verworfen");
        self.save();
        self.status = "Anhang verworfen".to_string();
        self.log("Anhang verworfen (Review abgebrochen)".to_string());
    }

    /// Trefferliste nach Schwellenänderung neu aufbauen (direkt im Modal):
    /// Automatik neu berechnen (Abgelehnte bleiben draußen, verschwundene
    /// Auto-Einträge samt Auswahl entfallen), Auswahl und Feldwahl
    /// übernommener Paare sowie explizit manuelle Einträge (neu bewertet)
    /// bleiben erhalten. Keine Datenänderung (kein Snapshot nötig).
    pub fn recompute_merge_review(&mut self) {
        let (fresh_ids, rejected, kept) = match self.merge_review.as_ref() {
            Some(review) => (
                review.fresh_ids.clone(),
                review.rejected.clone(),
                review
                    .candidates
                    .iter()
                    .map(|entry| {
                        (
                            (
                                entry.candidate.keep_id.clone(),
                                entry.candidate.drop_id.clone(),
                            ),
                            (
                                entry.selected,
                                entry.take_new_birth,
                                entry.take_new_death,
                                entry.take_new_name,
                                entry.manual,
                                entry.parent_pick_new.clone(),
                                entry.parent_pick_old.clone(),
                            ),
                        )
                    })
                    .collect::<HashMap<
                        (String, String),
                        (bool, bool, bool, bool, bool, Option<String>, Option<String>),
                    >>(),
            ),
            None => return,
        };
        let threshold = (self.match_threshold / 100.0).clamp(0.0, 1.0);
        let common_min = self.common_given_threshold.clamp(2, 10);
        let mut auto = if fresh_ids.is_empty() {
            self.data.find_project_duplicates(threshold, common_min)
        } else {
            self.data.find_merge_candidates(&fresh_ids, threshold, common_min)
        };
        auto.retain(|candidate| {
            !rejected.contains(&(candidate.keep_id.clone(), candidate.drop_id.clone()))
        });
        let auto_pairs: HashSet<(String, String)> = auto
            .iter()
            .map(|candidate| (candidate.keep_id.clone(), candidate.drop_id.clone()))
            .collect();
        let mut entries: Vec<MergeReviewEntry> = auto
            .into_iter()
            .map(|candidate| {
                let key = (candidate.keep_id.clone(), candidate.drop_id.clone());
                let mut entry = self.make_review_entry(candidate);
                if let Some((selected, birth, death, name, _, pick_new, pick_old)) =
                    kept.get(&key)
                {
                    entry.selected = *selected;
                    entry.take_new_birth = *birth;
                    entry.take_new_death = *death;
                    entry.take_new_name = *name;
                    entry.parent_pick_new = pick_new.clone();
                    entry.parent_pick_old = pick_old.clone();
                }
                entry
            })
            .collect();
        // Explizit manuelle Einträge (nicht in der Automatik, nicht
        // abgelehnt) mit frischen Scores erhalten.
        let mut kept_manual: Vec<((String, String), (bool, bool, bool))> = kept
            .into_iter()
            .filter(|(key, (_, _, _, _, manual, _, _))| {
                *manual && !auto_pairs.contains(key) && !rejected.contains(key)
            })
            .map(|(key, (selected, birth, death, _, _, _, _))| (key, (selected, birth, death)))
            .collect();
        kept_manual.sort_by(|left, right| left.0.cmp(&right.0));
        for ((keep, drop), (selected, birth, death)) in kept_manual {
            if let Some(candidate) =
                self.data.manual_match_candidate(&keep, &drop, &fresh_ids, common_min)
            {
                let mut entry = self.make_review_entry(candidate);
                entry.selected = selected;
                entry.take_new_birth = birth;
                entry.take_new_death = death;
                entry.manual = true;
                entries.push(entry);
            }
        }
        let count = entries.len();
        if let Some(review) = self.merge_review.as_mut() {
            review.candidates = entries;
        }
        self.status = format!("Liste neu aufgebaut: {count} Treffer");
    }

    /// Manuell gewähltes Paar (Bestand links, Neu rechts) als Treffer
    /// übernehmen — mehrfach möglich. Doppelte Paare und ungültige Auswahl
    /// meldet die Statuszeile; der Treffer landet unselektiert in der Liste.
    pub fn add_manual_match(&mut self) {
        let (keep, drop) = match self.merge_review.as_ref().and_then(|review| {
            review.manual_keep.clone().zip(review.manual_drop.clone())
        }) {
            Some(pair) => pair,
            None => {
                self.status = "Bitte links eine Person aus dem Bestand und rechts eine aus dem Anhang wählen.".into();
                return;
            }
        };
        let fresh = self
            .merge_review
            .as_ref()
            .map(|review| review.fresh_ids.clone())
            .unwrap_or_default();
        let Some(candidate) = self.data.manual_match_candidate(
            &keep,
            &drop,
            &fresh,
            self.common_given_threshold,
        ) else {
            self.status = "Ungültige Auswahl (links Bestand, rechts frisch Angehängtes).".into();
            return;
        };
        if self.merge_review.as_ref().is_some_and(|review| {
            review
                .candidates
                .iter()
                .any(|entry| entry.candidate.keep_id == keep && entry.candidate.drop_id == drop)
        }) {
            self.status = "Dieser Treffer steht bereits in der Liste.".into();
            return;
        }
        let entry = self.make_review_entry(candidate);
        if let Some(review) = self.merge_review.as_mut() {
            review.candidates.push(MergeReviewEntry { manual: true, ..entry });
            review.manual_keep = None;
            review.manual_drop = None;
            self.status = format!(
                "Manueller Treffer hinzugefügt (jetzt {}).",
                review.candidates.len()
            );
        }
        self.people_groups_dirty = true;
    }

    /// Elternpaar aus zwei Merge-Dialogen als Treffer übernehmen (Button in
    /// allen Merge-Ansichten): Paar in die Review-Liste legen (Review ggf.
    /// anlegen). Gibt true bei Aufnahme zurück.
    pub fn add_parent_pair_as_match(&mut self, keep_parent_id: &str, drop_parent_id: &str) -> bool {
        let review_was_open = self.show_merge_review;
        if self.merge_review.is_none() {
            self.merge_review = Some(MergeReview {
                candidates: Vec::new(),
                rejected: Vec::new(),
                fresh_ids: HashSet::new(),
                pre_import: None,
                manual_query: String::new(),
                manual_keep: None,
                manual_drop: None,
            });
        }
        let threshold = (self.match_threshold / 100.0).clamp(0.0, 1.0);
        let common_min = self.common_given_threshold.clamp(2, 10);
        let Some(candidate) =
            self.data
                .pair_match_candidate(keep_parent_id, drop_parent_id, threshold, common_min)
        else {
            self.status = "Elternpaar ungültig (identisch, unbekannt oder Eltern/Kind).".into();
            return false;
        };
        if self.merge_review.as_ref().is_some_and(|review| {
            review.candidates.iter().any(|entry| {
                entry.candidate.keep_id == keep_parent_id
                    && entry.candidate.drop_id == drop_parent_id
            })
        }) {
            self.status = "Dieses Elternpaar steht bereits in der Liste.".into();
            return false;
        }
        let entry = self.make_review_entry(candidate);
        if let Some(review) = self.merge_review.as_mut() {
            review.candidates.push(MergeReviewEntry { manual: true, ..entry });
        }
        self.people_groups_dirty = true;
        // Aus dem Einzel-Dialog: dorthin wechseln (Hauptpaar dort neu wählen).
        if !review_was_open {
            self.show_person_merge = false;
            self.person_merge_detail = None;
            self.show_merge_review = true;
            self.status = "Elternpaar übernommen — weiter im Review.".into();
        } else {
            self.status = "Elternpaar als Treffer hinzugefügt.".into();
        }
        true
    }

    /// Review-Eintrag mit Standard-Feldwahl (nicht-leere Bestandsseite
    /// bevorzugt, Treffer unselektiert) — für Automatik, Manuell und Scan.
    fn make_review_entry(&self, candidate: MergeCandidate) -> MergeReviewEntry {
        let keep = self.data.find(&candidate.keep_id);
        MergeReviewEntry {
            take_new_birth: keep.map(|person| person.birth.trim().is_empty()).unwrap_or(true),
            take_new_death: keep.map(|person| person.death.trim().is_empty()).unwrap_or(true),
            take_new_name: false,
            candidate,
            selected: false,
            manual: false,
            parent_pick_new: None,
            parent_pick_old: None,
        }
    }

    /// Duplikate im aktuellen Projekt suchen (gleiche Vergleichsfunktion wie
    /// beim Import, aber MIT Verwandten-IDs) und im Review-Dialog zeigen.
    pub fn find_project_duplicates(&mut self) {
        let threshold = (self.match_threshold / 100.0).clamp(0.0, 1.0);
        let candidates =
            self.data.find_project_duplicates(threshold, self.common_given_threshold);
        self.snapshot("Duplikate suchen");
        let entries = candidates
            .into_iter()
            .map(|candidate| self.make_review_entry(candidate))
            .collect();
        self.merge_review = Some(MergeReview {
            candidates: entries,
            rejected: Vec::new(),
            fresh_ids: HashSet::new(),
            pre_import: None,
            manual_query: String::new(),
            manual_keep: None,
            manual_drop: None,
        });
        self.show_merge_review = true;
        self.people_groups_dirty = true;
        let count = self.merge_review.as_ref().map(|r| r.candidates.len()).unwrap_or(0);
        if count == 0 {
            self.status = "Keine Duplikate im Projekt gefunden.".into();
        } else {
            self.status = format!("{count} mögliche Duplikate im Projekt — bitte prüfen");
        }
    }

    /// Einzel-Merge-Dialog öffnen (rechte Leiste, Zusammenführen): Top-5
    /// Treffer zur gewählten Person (ohne Selbst) mit Scores berechnen.
    pub fn open_person_merge(&mut self) {
        let Some(target) = self.selected.clone().filter(|id| self.data.find(id).is_some())
        else {
            self.status = "Keine Person ausgewählt".into();
            return;
        };
        let threshold = (self.match_threshold / 100.0).clamp(0.0, 1.0);
        let common_min = self.common_given_threshold.clamp(2, 10);
        let mut hits: Vec<(u32, MergeCandidate)> = self
            .data
            .people
            .iter()
            .filter(|person| person.id != target)
            .filter_map(|person| {
                let candidate =
                    self.data
                        .pair_match_candidate(&target, &person.id, threshold, common_min)?;
                let total = ((candidate.name_score + candidate.family_score + candidate.kin_score)
                    / 3.0
                    * 100.0)
                    .round()
                    .clamp(0.0, 100.0) as u32;
                Some((total, candidate))
            })
            .collect();
        hits.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then_with(|| left.1.drop_id.cmp(&right.1.drop_id))
        });
        hits.truncate(5);
        self.person_merge_hits = hits.into_iter().map(|(_, candidate)| candidate).collect();
        self.person_merge_query = String::new();
        self.person_merge_detail = None;
        self.person_merge_take_birth = false;
        self.person_merge_take_death = false;
        self.person_merge_take_name = false;
        self.person_merge_parent_new = None;
        self.person_merge_parent_old = None;
        self.show_person_merge = true;
    }

    /// Einzel-Merge anwenden (Detail-Paar zusammenführen wie im Review).
    pub fn apply_person_merge(&mut self, take_birth: bool, take_death: bool, take_name: bool) {
        let (keep, drop) = match self.person_merge_detail.clone() {
            Some(pair) => pair,
            None => return,
        };
        if self.data.find(&keep).is_none() || self.data.find(&drop).is_none() {
            return;
        }
        self.merge_one(&keep, &drop, take_birth, take_death, take_name);
        self.people_groups_dirty = true;
        self.fix_selection_after_merge();
        self.show_person_merge = false;
        self.person_merge_detail = None;
        self.status = "Personen zusammengeführt".into();
        self.log(format!("Einzel-Merge: {keep} + {drop}"));
    }

    /// Ordner des aktuell geöffneten Projekts (für Sicherung/Wiederherstellung).
    fn current_project_dir(&self) -> Option<PathBuf> {
        self.current_data_path
            .clone()
            .and_then(|path| path.parent().map(Path::to_path_buf))
    }

    /// Neueste Sicherung des aktuellen Projekts (für die Anzeige).
    pub fn last_backup_label(&self) -> Option<String> {
        let dir = self.current_project_dir()?;
        let name = dir.file_name()?.to_str()?;
        crate::projects::list_backups(&default_library(), name)
            .last()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .map(str::to_string)
    }

    /// Aktuelles Projekt aus der neuesten Sicherung wiederherstellen
    /// (nach fehlerhaftem Import) und neu laden.
    pub fn restore_last_backup(&mut self) {
        let Some(dir) = self.current_project_dir() else {
            self.status = "Kein Projektordner zum Wiederherstellen".into();
            return;
        };
        let name = dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string();
        let Some(backup) = crate::projects::list_backups(&default_library(), &name).pop() else {
            self.status = "Keine Sicherung vorhanden".into();
            return;
        };
        match crate::projects::restore_backup(&dir, &backup) {
            Ok(()) => {
                self.log(format!("Wiederhergestellt: {}", backup.display()));
                let path = self.current_data_path.clone();
                if let Some(path) = path {
                    self.load_path(&path);
                }
                self.status = "Sicherung wiederhergestellt".into();
            }
            Err(error) => self.status = format!("Wiederherstellen fehlgeschlagen: {error}"),
        }
    }

    /// Projektliste für den Öffnen-Dialog neu einlesen (einmalig statt pro Frame).
    pub fn refresh_project_list(&mut self) {
        let paths = crate::import::discover_projects(&self.library);
        self.project_list_names = paths
            .iter()
            .map(|path| crate::import::project_display_name(path))
            .collect();
        self.project_list_cache = paths;
    }

    /// Datei laden (`crate::import::load_file`) und Ansichtszustand zurücksetzen.
    /// Das Baum-Layout wird aus der separaten `.layout.json` geladen (falls
    /// vorhanden) und in beiden Ausrichtungen angewendet.
    pub fn load_path(&mut self, path: &Path) {
        if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("mfg")) {
            match crate::projects::import_mfg(&default_library().join("projects"), path) {
                Ok(json) => {
                    self.load_path(&json);
                    self.refresh_project_list();
                }
                Err(error) => self.status = format!("Import fehlgeschlagen: {error}"),
            }
            return;
        }
        if !path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("json")) {
            self.import_project(path);
            return;
        }
        self.log(format!("Lade: {}", path.display()));
        match load_file(path) {
            Ok(data) => {
                self.library = path.parent().unwrap_or(Path::new(".")).to_path_buf();
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
                let (offsets, saved_reference) =
                    FileSystemStore::for_data_file(path).read_layout();
                self.manual_offsets = offsets;
                if let Some(ref_id) = saved_reference.filter(|id| self.data.find(id).is_some()) {
                    self.reference = Some(ref_id.clone());
                    self.selected = Some(ref_id);
                }
                self.reset_reference_navigation();
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
                #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
                save_last_project(path);
                self.refresh_project_list();
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
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
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
                let (offsets, saved_reference) = store.read_layout();
                self.manual_offsets = offsets;
                // Cache schreiben, damit offline weitergearbeitet werden kann.
                let cache = FileSystemStore::for_root(cache_root);
                let _ = cache.write_data(&data);
                let _ = cache.write_layout(
                    &self
                        .manual_offsets
                        .iter()
                        .map(|(id, offset)| (id.clone(), *offset))
                        .collect::<Vec<_>>(),
                    saved_reference.as_deref(),
                );
                self.server_online = true;
                self.server_base = Some(base_url.to_string());
                let reference_id = saved_reference
                    .filter(|id| data.people.iter().any(|p| &p.id == id))
                    .or_else(|| data.people.first().map(|p| p.id.clone()));
                self.selected = reference_id.clone();
                self.reference = reference_id;
                self.reset_reference_navigation();
                self.expanded.clear();
                self.data = data;
                self.people_groups_dirty = true;
                self.undo_stack.clear();
                self.redo_stack.clear();
                self.pending_select = None;
                self.inline_edit = false;
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
                        let (offsets, saved_reference) = cache.read_layout();
                        self.manual_offsets = offsets;
                        self.server_online = false;
                        self.server_base = Some(base_url.to_string());
                        let reference_id = saved_reference
                            .filter(|id| data.people.iter().any(|p| &p.id == id))
                            .or_else(|| data.people.first().map(|p| p.id.clone()));
                        self.selected = reference_id.clone();
                        self.reference = reference_id;
                        self.reset_reference_navigation();
                        self.expanded.clear();
                        self.data = data;
                        self.people_groups_dirty = true;
                        self.undo_stack.clear();
                        self.redo_stack.clear();
                        self.pending_select = None;
                        self.inline_edit = false;
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

    /// Mobile/Web bekommen spaeter einen async Transport statt `ureq` und lokalem Cache.
    #[cfg(any(target_arch = "wasm32", target_os = "android"))]
    pub fn load_from_server(&mut self, _base_url: &str, _token: &str) {
        self.status = "Server-Laden ist auf diesem Ziel noch nicht implementiert".into();
        self.log(self.status.clone());
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
        self.tree_person_limit = self.tree_initial_person_limit;
        // Verlauf pflegen: neue Referenz hinten anhängen, Vorwärtszweig
        // abschneiden (Browser-Muster), auf 100 Einträge begrenzen.
        let on_current = self
            .reference_history
            .get(self.reference_history_index)
            .map(String::as_str)
            == Some(id);
        if !on_current {
            self.reference_history.truncate(self.reference_history_index + 1);
            self.reference_history.push(id.to_string());
            if self.reference_history.len() > 100 {
                let overflow = self.reference_history.len() - 100;
                self.reference_history.drain(0..overflow);
            }
            self.reference_history_index = self.reference_history.len().saturating_sub(1);
        }
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

    pub fn can_navigate_back(&self) -> bool {
        self.reference_history_index > 0
    }

    pub fn can_navigate_forward(&self) -> bool {
        self.reference_history_index + 1 < self.reference_history.len()
    }

    /// Zur vorigen Referenzperson wechseln (frisst den Verlauf nicht auf).
    pub fn navigate_back(&mut self) {
        if self.reference_history_index > 0 {
            self.reference_history_index -= 1;
            self.apply_navigation();
        }
    }

    /// Zur nächsten Referenzperson wechseln.
    pub fn navigate_forward(&mut self) {
        if self.reference_history_index + 1 < self.reference_history.len() {
            self.reference_history_index += 1;
            self.apply_navigation();
        }
    }

    /// Geschichte direkt anwenden, ohne einen neuen Eintrag anzulegen.
    fn apply_navigation(&mut self) {
        if let Some(id) = self
            .reference_history
            .get(self.reference_history_index)
            .cloned()
        {
            self.reference = Some(id.clone());
            self.selected = Some(id.clone());
            self.tree_person_limit = self.tree_initial_person_limit;
            self.pan = Vec2::ZERO;
            self.fit_pending = true;
            let name = self
                .data
                .find(&id)
                .map(|p| p.display_name())
                .unwrap_or_else(|| id.clone());
            self.status = format!("Referenzperson: {name}");
        }
    }

    /// Verlauf der besuchten Referenzpersonen auf den aktuellen Stand
    /// zurücksetzen (nach Laden eines Projekts/Server).
    fn reset_reference_navigation(&mut self) {
        self.reference_history = self.reference.clone().into_iter().collect();
        self.reference_history_index = self.reference_history.len().saturating_sub(1);
        self.tree_person_limit = self.tree_initial_person_limit;
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
        self.draft.strip_names();
        let name = self.draft.display_name();
        let is_new = self
            .data
            .people
            .iter()
            .all(|person| person.id != self.draft.id);
        let action = if is_new {
            format!("Person anlegen: {name}")
        } else {
            format!("Profil bearbeiten: {name}")
        };
        self.snapshot(action);
        if let Some(person) = self
            .data
            .people
            .iter_mut()
            .find(|person| person.id == self.draft.id)
        {
            *person = self.draft.clone();
        } else {
            self.data.people.push(self.draft.clone());
        }
        let _ = crate::media::write_round_avatar_now(&self.library, &self.draft);
        crate::media::clear_person_photo_cache(&mut self.photo_cache, &self.draft.id);
        self.selected = Some(self.draft.id.clone());
        self.editing = Some(self.draft.id.clone());
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
                image: None,
            });
            self.pending_select_choice = 0;
            return;
        }
        self.apply_select(id, set_reference);
    }

    fn apply_select(&mut self, id: &str, set_reference: bool) {
        self.selected = Some(id.into());
        // Einfachauswahl löst Mehrfachauswahl auf (Invariante: selected = letzte).
        self.multi_select = vec![id.into()];
        self.relation_picker = None;
        if set_reference {
            self.set_reference(id);
        }
    }

    /// Strg+Klick-Toggle der Mehrfachauswahl: drin → raus (Anzeige fällt auf
    /// die davor hinzugefügte zurück), draußen → hinten an (wird angezeigt).
    fn toggle_multi_select(&mut self, id: &str) {
        if self.data.find(id).is_none() {
            return;
        }
        if let Some(position) = self.multi_select.iter().position(|member| member == id) {
            self.multi_select.remove(position);
        } else {
            self.multi_select.push(id.into());
        }
        self.selected = self.multi_select.last().cloned();
        self.relation_picker = None;
    }

    /// Bild nur in die Galerie des Entwurfs legen (Profilbild unverändert).
    pub fn add_gallery_photo(&mut self, relative: String) {
        if !self.draft.gallery.iter().any(|entry| entry == &relative) {
            self.draft.gallery.push(relative);
        }
    }

    /// Bild direkt in die Galerie der gespeicherten Person legen — ohne
    /// Bearbeitungsmodus, mit Undo-Snapshot.
    pub fn add_gallery_photo_to_person(&mut self, id: &str, relative: String) {
        let Some(name) = self.data.find(id).map(|person| person.display_name()) else {
            return;
        };
        if self
            .data
            .find(id)
            .is_some_and(|person| person.gallery.iter().any(|entry| entry == &relative))
        {
            self.status = "Bild ist bereits in der Galerie".into();
            return;
        }
        self.snapshot(format!("Galeriebild hinzufügen: {name}"));
        if let Some(person) = self.data.people.iter_mut().find(|person| person.id == id) {
            person.gallery.push(relative);
        }
        self.status = "Bild in Galerie gelegt".into();
    }

    /// Übernommenes Bild als Profilbild in den Entwurf setzen (plus
    /// Galerie-Eintrag) und Foto-Cache der Person verwerfen.
    pub fn set_draft_photo(&mut self, relative: String) {
        self.draft.photo = Some(relative.clone());
        if !self.draft.gallery.iter().any(|entry| entry == &relative) {
            self.draft.gallery.push(relative);
        }
        let id = self.draft.id.clone();
        crate::media::clear_person_photo_cache(&mut self.photo_cache, &id);
    }

    /// Vom Wechsel-Dialog bestätigten Zielwechsel ausführen. War ein
    /// Bild-Drop der Auslöser, wird danach der Editor mit dem Bild in der
    /// Galerie geöffnet (Profilbild bleibt unverändert). Mit `edit_after`
    /// (Speichern-und-wechseln) öffnet das Ziel gleich wieder im
    /// Bearbeitenmodus.
    pub(crate) fn apply_pending_select(&mut self, ctx: &egui::Context, edit_after: bool) {
        if let Some(pending) = self.pending_select.take() {
            self.apply_select(&pending.target, pending.set_reference);
            if let Some(image) = pending.image {
                self.open_draft_with_image(ctx, &pending.target, &image);
            } else if edit_after {
                if let Some(person) = self.data.find(&pending.target).cloned() {
                    self.draft = person;
                    self.draft.ensure_standard_events();
                    self.seed_shown_name_fields();
                    self.inline_edit = true;
                }
            }
        }
    }

    /// Quellmedium öffnen (aus dem Quellen-Kontextmenü, separat von der
    /// Galerie): Bilder in der Lightbox, sonst im System-Betrachter (nur
    /// Desktop).
    pub fn open_source_media(&mut self, relative: &str) {
        let path = self.library.join(relative);
        if !path.is_file() {
            self.status = "Mediendatei fehlt".into();
            return;
        }
        if crate::media::is_image_file(&path) {
            self.lightbox_image = Some(relative.to_string());
            return;
        }
        #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
        {
            #[cfg(target_os = "windows")]
            let result = std::process::Command::new("cmd")
                .args(["/C", "start", "", &path.to_string_lossy().to_string()])
                .spawn();
            #[cfg(target_os = "macos")]
            let result = std::process::Command::new("open").arg(&path).spawn();
            #[cfg(all(
                not(target_os = "windows"),
                not(target_os = "macos"),
                not(target_arch = "wasm32"),
                not(target_os = "android")
            ))]
            let result = std::process::Command::new("xdg-open").arg(&path).spawn();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(relative);
            match result {
                Ok(_) => self.status = format!("Geöffnet: {name}"),
                Err(_) => self.status = "Konnte Datei nicht öffnen".into(),
            }
        }
        #[cfg(any(target_arch = "wasm32", target_os = "android"))]
        {
            self.status = "Öffnen auf diesem Ziel nicht unterstützt".into();
        }
    }

    /// Editor der Person öffnen und ein Bild in die Galerie legen (das
    /// Profilbild wird dabei NICHT angetastet). Bei ungespeicherten
    /// Änderungen an einer anderen Person erscheint zuerst der
    /// Wechsel-Dialog (Speichern/Verwerfen/Abbrechen).
    pub fn open_editor_with_image(&mut self, ctx: &egui::Context, id: &str, path: &Path) {
        if !crate::media::is_image_file(path) {
            self.status = "Nur Bilddateien (png, jpg, jpeg, webp) können abgelegt werden".into();
            return;
        }
        if self.inline_edit
            && self.draft_has_changes()
            && self.selected.as_deref() != Some(id)
        {
            self.pending_select = Some(PendingSelect {
                target: id.into(),
                set_reference: false,
                image: Some(path.to_path_buf()),
            });
            self.pending_select_choice = 0;
            return;
        }
        self.apply_select(id, false);
        self.open_draft_with_image(ctx, id, path);
    }

    /// Entwurf der Person laden, Editor öffnen und Bild in die Galerie legen.
    fn open_draft_with_image(&mut self, ctx: &egui::Context, id: &str, path: &Path) {
        let Some(person) = self.data.find(id).cloned() else {
            return;
        };
        self.draft = person;
        self.draft.ensure_standard_events();
        self.seed_shown_name_fields();
        self.inline_edit = true;
        if let Some(relative) = crate::media::import_media_file_async(ctx, &self.library, path) {
            if !self.draft.gallery.iter().any(|entry| entry == &relative) {
                self.draft.gallery.push(relative);
            }
            let id = self.draft.id.clone();
            crate::media::clear_person_photo_cache(&mut self.photo_cache, &id);
            self.status = "Bild in Galerie gelegt — Speichern nicht vergessen".into();
        } else {
            self.status = "Bild konnte nicht übernommen werden".into();
        }
    }

    /// Stift/Diskette als Methode: Strg+E schaltet zwischen Bearbeiten und
    /// Speichern um (Entwurf laden bzw. übernehmen + Datei schreiben).
    pub fn toggle_inline_edit(&mut self) {
        if self.inline_edit {
            self.commit_draft();
            self.status = "Profil gespeichert".into();
            // Auch auf die Festplatte schreiben — sonst sind
            // Foto/Änderungen nach Neustart weg.
            self.save();
            self.inline_edit = false;
            self.relation_picker = None;
            self.relation_query.clear();
        } else if let Some(id) = self.selected.clone() {
            if let Some(person) = self.data.find(&id).cloned() {
                self.draft = person;
                self.draft.ensure_standard_events();
                self.seed_shown_name_fields();
                self.inline_edit = true;
            }
        }
    }

    /// Befüllte erweiterte Namensfelder beim Betreten des Editmodus einblenden
    /// (einmalig; danach toggelt nur das Rechtsklick-Menü — Ausblenden bei
    /// vollem Feld bleibt möglich, Daten bleiben erhalten).
    pub(crate) fn seed_shown_name_fields(&mut self) {
        for (key, field) in [
            ("title", self.draft.title.as_str()),
            ("nick_name", self.draft.nick_name.as_str()),
            ("call_name", self.draft.call_name.as_str()),
            ("name_prefix", self.draft.name_prefix.as_str()),
            ("surname_prefix", self.draft.surname_prefix.as_str()),
            ("suffix", self.draft.suffix.as_str()),
        ] {
            if !field.trim().is_empty() {
                self.name_fields_shown.insert(key.to_string());
            }
        }
    }

    /// Zeichenfläche (Strg+Z / Strg+Y) zurück- und vorlaufen lassen,
    /// Strg+E schaltet Bearbeiten/Speichern um — auch aus Textfeldern heraus.
    pub fn handle_undo_redo_shortcuts(&mut self, ctx: &egui::Context) {
        // Strg+E zuerst: Speichern muss auch bei aktivem Textfeld gehen.
        let toggle_edit = ctx.input(|i| {
            i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::E)
        });
        if toggle_edit {
            self.toggle_inline_edit();
            return;
        }
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
        match FileSystemStore::for_data_file(&data_path)
            .write_layout(&entries, self.reference.as_deref())
        {
            Ok(_) => self.log(format!(
                "Layout gesichert: {} ({} Einträge)",
                data_path.display(),
                entries.len()
            )),
            Err(e) => self.log(format!("Layout sichern fehlgeschlagen: {e}")),
        }
    }

    /// Rundet das rahmenlose Fenster (Windows) mit 5 Punkten Radius; im
    /// maximierten Zustand wird die Form zurückgesetzt. Wird nur bei
    /// Größen-/Zustandsänderung tatsächlich angewendet.
    fn apply_window_corners(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        #[cfg(windows)]
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if self.window_hwnd.is_none() {
                match frame.window_handle().map(|h| h.as_raw()) {
                    Ok(RawWindowHandle::Win32(h)) => self.window_hwnd = Some(h.hwnd.get()),
                    _ => return,
                }
            }
            let Some(hwnd) = self.window_hwnd else {
                return;
            };
            let hwnd = hwnd as *mut std::ffi::c_void;
            let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            let mut rect = win_shape::Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            // SAFETY: `hwnd` stammt aus dem echten Fenster-Handle dieses Frames.
            unsafe { win_shape::GetClientRect(hwnd, &mut rect) };
            let key = (rect.right, rect.bottom, maximized);
            if self.window_region == Some(key) {
                return;
            }
            self.window_region = Some(key);
            let scale = ctx.pixels_per_point().max(0.1);
            // SAFETY: Handle + Region stammen aus den Win32-APIs; die Region
            // geht bei Erfolg in den Besitz des Fensters über.
            unsafe {
                if maximized || rect.right <= 0 || rect.bottom <= 0 {
                    win_shape::SetWindowRgn(hwnd, std::ptr::null_mut(), 1);
                } else {
                    let r = (WINDOW_CORNER_RADIUS * scale).round() as i32;
                    let region = win_shape::CreateRoundRectRgn(
                        0,
                        0,
                        rect.right + 1,
                        rect.bottom + 1,
                        r * 2,
                        r * 2,
                    );
                    if !region.is_null() {
                        win_shape::SetWindowRgn(hwnd, region, 1);
                    }
                }
            }
        }
        #[cfg(not(windows))]
        {
            let _ = (ctx, frame);
        }
    }
}

impl eframe::App for MiniGramps {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
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

        // Rahmenloses Fenster: 5px-Rundung anwenden (nur bei Änderung).
        self.apply_window_corners(ctx, frame);

        // Schließen abfangen: Bei ungespeicherten Änderungen erst nachfragen.
        let native_close = ctx.input(|i| i.viewport().close_requested());
        if self.pending_close || native_close {
            // Ungespeichert = eine Bearbeitung läuft gerade (Profil/Editor).
            if self.inline_edit {
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
                    // Schmale Leiste: Titel „STAMMBAUM" ausblenden und die
                    // Ansichts-Schalter abkürzen (Tooltips = volle Bedeutung).
                    let wide = ui.available_width() >= 640.0;
                    if wide {
                        ui.label(
                            egui::RichText::new("STAMMBAUM")
                                .size(13.0)
                                .strong()
                                .color(section_accent),
                        );
                        ui.separator();
                    }
                // Manuelle Offsets gelten in BEIDEN Ausrichtungen (Layout-
                // Datei) und bleiben beim Ansichtwechsel erhalten.
                let old_view = self.tree_view;
                let old_orientation = self.tree_orientation;
                let (label_desc, label_anc, label_fan) = if wide {
                    ("Nachfahrenbaum", "Vorfahrenbaum", "Ahnenfächer")
                } else {
                    ("Nachf.", "Vorf.", "Fächer")
                };
                let (label_v, label_h) = if wide { ("Vertikal", "Horizontal") } else { ("V", "H") };
                ui.selectable_value(
                    &mut self.tree_view,
                    TreeView::Descendants,
                    egui::RichText::new(label_desc).size(11.0),
                )
                .on_hover_text("Nachfahrenbaum");
                ui.selectable_value(
                    &mut self.tree_view,
                    TreeView::Ancestors,
                    egui::RichText::new(label_anc).size(11.0),
                )
                .on_hover_text("Vorfahrenbaum");
                ui.selectable_value(
                    &mut self.tree_view,
                    TreeView::Fan,
                    egui::RichText::new(label_fan).size(11.0),
                )
                .on_hover_text("Ahnenfächer");
                ui.separator();
                ui.selectable_value(
                    &mut self.tree_orientation,
                    TreeOrientation::Vertical,
                    egui::RichText::new(label_v).size(11.0),
                )
                .on_hover_text("Vertikal");
                ui.selectable_value(
                    &mut self.tree_orientation,
                    TreeOrientation::Horizontal,
                    egui::RichText::new(label_h).size(11.0),
                )
                .on_hover_text("Horizontal");
                if self.tree_view != old_view || self.tree_orientation != old_orientation {
                    self.fit_pending = true;
                }
                ui.separator();
                if icon_only_button(ui, ICON_CENTER, "toolbar-center")
                    .on_hover_text("Stammbaum zentrieren und im Fenster einpassen (Zoom-Fit)")
                    .clicked()
                {
                    log::debug!("CENTER: zoom={:.3} pan=({:.1},{:.1}) offsets={}",
                        self.zoom, self.pan.x, self.pan.y, self.manual_offsets.len());
                    self.fit_pending = true;
                }
                if icon_only_button(ui, ICON_RESET, "toolbar-reset")
                    .on_hover_text("Alle manuellen Verschiebungen zurücksetzen (Layout-Reset)")
                    .clicked()
                {
                    log::debug!("RESET: clearing {} offsets, zoom={:.3} pan=({:.1},{:.1})",
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
                // Offenes Modal: Baum-Eingaben (Klick/Drag/Zoom/Drop) sind
                // gesperrt, siehe `any_modal_open`.
                let modal_block = self.any_modal_open();
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
                if response.hovered() && scroll != 0.0 && !modal_block {
                    // Zoom um den Mauspunkt: derselbe Layoutpunkt bleibt unter
                    // dem Cursor stehen (`zoom_at` passt den Pan an).
                    if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
                        self.zoom_at(response.rect, pointer, 1.0 + scroll * 0.001);
                    } else {
                        self.zoom = (self.zoom * (1.0 + scroll * 0.001))
                            .clamp(0.15, MAX_INTERACTIVE_ZOOM);
                    }
                }
                // Maus-Transitions-Erkennung für Drag-Logging.
                let mouse_is_down = ui.input(|i| i.pointer.primary_down());
                let drag_started = mouse_is_down && !self.drag_mouse_was_down;
                let drag_ended = !mouse_is_down && self.drag_mouse_was_down;
                self.drag_mouse_was_down = mouse_is_down;
// Umfangreiche Layoutdiagnose: Level `debug` (Standard aus), Auslöser sind
// Fit/Drag-Ende oder F9. Sichtbar mit `RUST_LOG=minigramps=debug`.
let log_layout_request = ui.input(|i| i.key_pressed(egui::Key::F9));
let log_layout = self.fit_pending || drag_ended || log_layout_request;
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
                let card_drag_was_active = self.card_drag.is_some();
                let may_start_card_drag = self.tree_tool == TreeTool::Cursor
                    && !card_drag_was_active
                    && mouse_is_down
                    && !modal_block
                    && ui.input(|i| i.modifiers.shift);
                let previous_manual_offsets =
                    may_start_card_drag.then(|| self.manual_offsets.clone());
                let mut manual_offsets = if self.tree_tool == TreeTool::Cursor {
                    std::mem::take(&mut self.manual_offsets)
                } else {
                    self.manual_offsets.clone()
                };
                let mut card_drag = self.card_drag.take();
                let mut swap_latch = self.partner_swap_latch;
                let mut frame_drag = false;
                let mut more_people_available = false;
                let mut loaded_more_people = false;
                let content_bounds = tree::draw_tree(
                    &painter,
                    response.rect,
                    &self.data,
                    reference.as_deref(),
                    viewed.as_deref(),
                    &mut action,
                    &self.expanded,
                    &mut self.long_press_used,
                    &mut swap_latch,
                    &mut card_drag,
                    &mut frame_drag,
                    &mut self.tree_card_rects,
                    self.max_generations,
                    self.tree_person_limit,
                    &mut more_people_available,
                    self.layout_gap,
                    self.non_partner_gap,
                    self.compact_card_width,
                    self.portrait_card_width,
                    &mut manual_offsets,
                    &self.library,
                    &mut self.photo_cache,
                    self.tree_view,
                    self.tree_orientation,
                    self.card_layout,
                    &self.birth_symbol,
                    &self.death_symbol,
                    self.zoom,
                    self.pan,
                    drag_started,
                    drag_ended,
                    log_layout,
                    modal_block,
                    &self.multi_select,
                    self.photo_full_zoom,
                    self.initials_full_zoom,
                    self.warn_certainty,
                );
                if more_people_available
                    && self.max_generations == 0
                    && self.tree_view == TreeView::Ancestors
                {
                    let button_size = egui::Vec2::new(190.0, 30.0);
                    let button_center = egui::pos2(
                        response.rect.center().x,
                        response.rect.bottom() - button_size.y / 2.0 - 10.0,
                    );
                    let button_rect = egui::Rect::from_center_size(button_center, button_size);
                    if ui
                        .put(
                            button_rect,
                            egui::Button::new(format!(
                                "Weitere {} laden",
                                self.tree_load_step
                            )),
                        )
                        .clicked()
                        && !modal_block
                    {
                        self.tree_person_limit = self
                            .tree_person_limit
                            .saturating_add(self.tree_load_step.max(1));
                        self.fit_pending = true;
                        self.status = format!(
                            "Vorfahrenlimit auf {} Personen erhöht",
                            self.tree_person_limit
                        );
                        loaded_more_people = true;
                        ctx.request_repaint();
                    }
                }
                if self.tree_tool == TreeTool::Cursor {
                    if !card_drag_was_active && card_drag.is_some() {
                        if let Some(previous_manual_offsets) = previous_manual_offsets
                            .filter(|previous| previous != &manual_offsets)
                        {
                            let (drag_id, drag_count) = card_drag
                                .as_ref()
                                .map(|(id, members)| (id.as_str(), members.len()))
                                .unwrap_or_default();
                            let label = if drag_id.starts_with("container:") {
                                format!("Geschwistergruppe verschieben ({drag_count} Personen)")
                            } else {
                                let name = self
                                    .data
                                    .find(drag_id)
                                    .map(|person| person.display_name())
                                    .unwrap_or_else(|| drag_id.to_string());
                                format!("Baumposition verschieben: {name}")
                            };
                            self.snapshot_layout_before(label, previous_manual_offsets);
                        }
                    }
                    self.manual_offsets = manual_offsets;
                    self.card_drag = card_drag;
                    self.partner_swap_latch = swap_latch;
                } else {
                    action = None;
                    self.card_drag = None;
                }
                // Zoom-Fit: den gelieferten Inhaltsbereich (Layout-Koordo-
                // dinaten) passend in die Zeichenfläche skalieren und mittig
                // setzen — einmalig nach Laden/Referenzwechsel.
                if self.fit_pending && !loaded_more_people {
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
                if self.tree_tool == TreeTool::Zoom && !modal_block {
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
                } else if !modal_block {
                    // Canvas-Pan NACH dem Zeichnen auswerten: Ein Drag innerhalb
                    // eines Paarrahmens verschiebt den Zweig (frame_drag) und
                    // darf die Zeichenfläche nicht mitschieben.
                    if response.dragged() && !ui.input(|i| i.modifiers.shift) && !frame_drag {
                        self.pan += response.drag_delta();
                    }
                }
                // Offene Dialogfenster fangen Klicks ab: Dahinterliegende
                // Baumkarten dürfen nicht gleichzeitig reagieren
                // (Click-through, z. B. Person hinter Fotowähler-Button).
                // Gilt für ALLE Modals, siehe `any_modal_open`.
                let action = if self.any_modal_open() { None } else { action };
                match action {
                    Some(TreeAction::View(id)) => self.request_select(&id, false),
                    Some(TreeAction::Reference(id)) => self.request_select(&id, true),
                    Some(TreeAction::ToggleMulti(id)) => self.toggle_multi_select(&id),
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
                    Some(TreeAction::PanTo(pos)) => {
                        self.pan = -pos * self.zoom;
                    }
                    None => {}
                }
                // Bilddatei auf eine Personenkarte gezogen: Editor dieser
                // Person öffnen und Bild in die Galerie legen. Bei offenem
                // Modal gesperrt (kein Editor hinter dem Dialog).
                let tree_drop = if modal_block {
                    None
                } else {
                    ui.input(|input| {
                        let hover = input.pointer.hover_pos()?;
                        if !response.rect.contains(hover) {
                            return None;
                        }
                        input.raw.dropped_files.iter().find_map(|file| {
                            let path = file.path.clone()?;
                            let hit = self
                                .tree_card_rects
                                .iter()
                                .rev()
                                .find(|(_, rect)| rect.contains(hover))
                                .map(|(id, _)| id.clone())?;
                            Some((hit, path))
                        })
                    })
                };
                if let Some((id, path)) = tree_drop {
                    let ctx = ui.ctx().clone();
                    self.open_editor_with_image(&ctx, &id, &path);
                }
            });

        // Dialoge (fixe Fenster, siehe dialogs.rs).
        dialogs::show_close_confirm(self, ctx);
        dialogs::show_open(self, ctx);
        dialogs::show_settings(self, ctx);
        dialogs::show_project(self, ctx);
        dialogs::show_export(self, ctx);
        dialogs::show_export_progress(self, ctx);
        dialogs::show_image_intent(self, ctx);
        dialogs::show_photo_chooser(self, ctx);
        dialogs::show_merge_review(self, ctx);
        dialogs::show_import_wizard(self, ctx);
        dialogs::show_person_merge(self, ctx);
        picker::show_link_detail(self, ctx);
        dialogs::show_quick(self, ctx);
        dialogs::show_quick_detail(self, ctx);
        dialogs::show_lightbox(self, ctx);
        // Wechsel-Dialog bei ungespeicherten Änderungen (vor dem nächsten Frame).
        dialogs::show_pending_select_confirm(self, ctx);

        // Zuletzt: eigene Rand-Resizerkennung (überschreibt ggf. gesetzte
        // Cursor der Widgets an den Fensterrändern).
        handle_window_resize(ctx);

        // Globale Einstellungen bei Änderung sofort persistieren.
        let mut window_x = self.settings_applied.window_x;
        let mut window_y = self.settings_applied.window_y;
        let mut window_width = self.settings_applied.window_width;
        let mut window_height = self.settings_applied.window_height;
        let mut window_maximized = self.settings_applied.window_maximized;

        let info = ctx.input(|i| i.viewport().clone());
        if let Some(inner) = info.inner_rect {
            let is_maximized = info.maximized.unwrap_or(false);
            window_maximized = Some(is_maximized);
            if !is_maximized && !info.minimized.unwrap_or(false) {
                window_x = Some(inner.min.x);
                window_y = Some(inner.min.y);
                window_width = Some(inner.width());
                window_height = Some(inner.height());
            }
        }

        let current = crate::settings::AppSettings {
            dark_mode: self.dark_mode,
            max_generations: self.max_generations,
            tree_initial_person_limit: self.tree_initial_person_limit,
            tree_load_step: self.tree_load_step,
            match_threshold: self.match_threshold,
            warn_certainty: self.warn_certainty,
            photo_full_zoom: self.photo_full_zoom,
            initials_full_zoom: self.initials_full_zoom,
            common_given_threshold: self.common_given_threshold,
            group_by_count: self.group_by_count,
            layout_gap: self.layout_gap,
            non_partner_gap: self.non_partner_gap,
            card_layout: self.card_layout,
            compact_card_width: self.compact_card_width,
            portrait_card_width: self.portrait_card_width,
            birth_symbol: self.birth_symbol.clone(),
            death_symbol: self.death_symbol.clone(),
            tree_orientation: self.tree_orientation,
            window_x,
            window_y,
            window_width,
            window_height,
            window_maximized,
        };
        if current != self.settings_applied {
            if current.tree_initial_person_limit
                != self.settings_applied.tree_initial_person_limit
                || (current.max_generations == 0 && self.settings_applied.max_generations != 0)
            {
                self.tree_person_limit = current.tree_initial_person_limit.max(1);
                self.fit_pending = true;
            }
            crate::settings::save(&current);
            self.settings_applied = current;
        }
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

/// Kompakter Icon-Button für die zweizeilige Befehlsgruppe in der
/// Titelleiste (Zeile 1: Undo/Redo, Zeile 2: Zurück/Vor). Über `icon_size`
/// und `min_size` fein steuerbar, damit die Gruppengrenzen auf die
/// großen Buttons (Speichern/Öffnen) ausgerichtet bleiben.
pub(crate) fn icon_row_button(
    ui: &mut egui::Ui,
    bytes: &'static [u8],
    id: &'static str,
    icon_size: f32,
    min_size: egui::Vec2,
) -> egui::Response {
    let image = egui::Image::from_bytes(format!("bytes://{id}.svg"), whitened_svg(bytes))
        .fit_to_exact_size(Vec2::splat(icon_size))
        .tint(ui.visuals().text_color());
    ui.add(egui::Button::image(image).min_size(min_size))
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

/// Win32-Aufrufe zum Abrunden des rahmenlosen Fensters (exakter Radius).
#[cfg(windows)]
mod win_shape {
    use std::ffi::c_void;

    #[repr(C)]
    pub struct Rect {
        pub left: i32,
        pub top: i32,
        pub right: i32,
        pub bottom: i32,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        pub fn GetClientRect(hwnd: *mut c_void, rect: *mut Rect) -> i32;
        pub fn SetWindowRgn(hwnd: *mut c_void, hwnd_rgn: *mut c_void, redraw: i32) -> i32;
    }

    #[link(name = "gdi32")]
    unsafe extern "system" {
        pub fn CreateRoundRectRgn(
            x1: i32,
            y1: i32,
            x2: i32,
            y2: i32,
            ellipse_w: i32,
            ellipse_h: i32,
        ) -> *mut c_void;
    }
}

/// Radius der Fensterrundung in logischen Punkten.
const WINDOW_CORNER_RADIUS: f32 = 5.0;

/// Rahmenloses Fenster: eigene Rand-Erkennung, da `with_decorations(false)`
/// die OS-Resizeränder entfernt. Bei Mausdruck am Rand wird der native
/// Resize-Vorgang gestartet (funktioniert, weil winit `WS_SIZEBOX` behält).
fn handle_window_resize(ctx: &egui::Context) {
    const M: f32 = 6.0;
    if ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
        return;
    }
    let rect = ctx.content_rect();
    let Some(p) = ctx.input(|i| i.pointer.hover_pos()) else {
        return;
    };
    let left = p.x <= rect.left() + M;
    let right = p.x >= rect.right() - M;
    let top = p.y <= rect.top() + M;
    let bottom = p.y >= rect.bottom() - M;
    let (dir, cursor) = match (left, right, top, bottom) {
        (true, _, true, _) => (
            egui::ResizeDirection::NorthWest,
            egui::CursorIcon::ResizeNorthWest,
        ),
        (_, true, true, _) => (
            egui::ResizeDirection::NorthEast,
            egui::CursorIcon::ResizeNorthEast,
        ),
        (true, _, _, true) => (
            egui::ResizeDirection::SouthWest,
            egui::CursorIcon::ResizeSouthWest,
        ),
        (_, true, _, true) => (
            egui::ResizeDirection::SouthEast,
            egui::CursorIcon::ResizeSouthEast,
        ),
        (true, _, _, _) => (egui::ResizeDirection::West, egui::CursorIcon::ResizeWest),
        (_, true, _, _) => (egui::ResizeDirection::East, egui::CursorIcon::ResizeEast),
        (_, _, true, _) => (egui::ResizeDirection::North, egui::CursorIcon::ResizeNorth),
        (_, _, _, true) => (egui::ResizeDirection::South, egui::CursorIcon::ResizeSouth),
        _ => return,
    };
    ctx.set_cursor_icon(cursor);
    if ctx.input(|i| i.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
    }
}

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
fn is_position_on_any_monitor(x: f32, y: f32) -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::ffi::c_void;
        #[link(name = "user32")]
        unsafe extern "system" {
            fn MonitorFromPoint(pt: POINT, dwFlags: u32) -> *mut c_void;
        }
        #[repr(C)]
        struct POINT {
            x: i32,
            y: i32,
        }
        let pt = POINT { x: x as i32, y: y as i32 };
        let monitor = unsafe { MonitorFromPoint(pt, 0) }; // MONITOR_DEFAULTTONULL = 0
        !monitor.is_null()
    }
    #[cfg(not(target_os = "windows"))]
    {
        true
    }
}

/// Einstiegspunkt: Fenster, App-Icon (Logo gerendert via resvg), Schriften.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
pub fn run() -> eframe::Result<()> {
    // Logging über Level steuern: Standard zeigt die App-Meldungen (info),
    // Layoutdiagnose nur mit `RUST_LOG=minigramps=debug` (oder `trace`).
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("minigramps=info,warn"),
    )
    .init();
    
    let settings = crate::settings::load();
    let mut viewport = egui::ViewportBuilder::default()
        .with_min_inner_size([900., 560.])
        .with_decorations(false);

    let mut use_default_size = true;
    if let (Some(x), Some(y)) = (settings.window_x, settings.window_y) {
        if is_position_on_any_monitor(x, y) {
            viewport = viewport.with_position([x, y]);
            if let (Some(w), Some(h)) = (settings.window_width, settings.window_height) {
                viewport = viewport.with_inner_size([w as f32, h as f32]);
                use_default_size = false;
            }
        }
    }
    if use_default_size {
        viewport = viewport.with_inner_size([1280., 760.]);
    }
    if settings.window_maximized.unwrap_or(false) {
        viewport = viewport.with_maximized(true);
    }

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

#[cfg(target_os = "android")]
pub fn run_android() -> eframe::Result<()> {
    Ok(())
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub async fn run_web() -> Result<(), wasm_bindgen::JsValue> {
    console_error_panic_hook::set_once();
    let web_options = eframe::WebOptions::default();
    eframe::WebRunner::new()
        .start(
            "minigramps_canvas",
            web_options,
            Box::new(|cc| {
                configure_fonts(&cc.egui_ctx);
                Ok(Box::new(MiniGramps::new()))
            }),
        )
        .await
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))
}

/// App-Icon aus dem eingebetteten Logo (`assets/icon.svg`) rasterisieren.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
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
/// Baskerville kennt keine Runen (Elhaz-Symbole) — unter Windows hängt daher
/// die System-Symbol-Schrift als Fallback dahinter (nur für fehlende Glyphen).
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
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android"), target_os = "windows"))]
    {
        if let Ok(bytes) = std::fs::read("C:\\Windows\\Fonts\\seguisym.ttf") {
            fonts.font_data.insert(
                "system-symbols".into(),
                Arc::new(egui::FontData::from_owned(bytes)),
            );
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .push("system-symbols".into());
        }
    }
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
