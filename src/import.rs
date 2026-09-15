//! Laden und Parsen von Familiendaten sowie Projekt-Suche.
//!
//! Verdrahtung:
//! - `load_file` wird von `ui::MiniGramps::load_path` und `import_dialog`
//!   aufgerufen und liefert ein `model::TreeData`.
//! - `discover_projects` speist die Liste im Öffnen-Dialog (`dialogs::show_open`).
//! - `default_library` liefert den Standard-Speicherort, benutzt von
//!   `ui::MiniGramps::new` und hier in `discover_projects`.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use roxmltree::Document;

use crate::model::{
    AlternativeName, Certainty, DocumentEntry, Event, EventKind, Family, Gender, PartnerRelation,
    Person, SourceEntry, TreeData, person,
};

/// Kleine, schnell lesbare Projektbeschreibung neben der eigentlichen
/// Datendatei. Spaetere `.mfg`/`.mmg`-Pakete verwenden dieselben Felder in
/// ihrem Manifest am Paketanfang.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ProjectManifest {
    pub format: String,
    pub format_version: u32,
    pub name: String,
    pub data_file: String,
}

/// Standard-Speicherort: App-Datenordner des Betriebssystems
/// (`...\AppData\Local\minigramps\MiniGramps\data`).
pub fn default_library() -> PathBuf {
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    {
        use directories::ProjectDirs;

        return ProjectDirs::from("org", "minigramps", "MiniGramps")
            .map(|d| d.data_local_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("minigramps-data"));
    }

    #[cfg(any(target_arch = "wasm32", target_os = "android"))]
    {
        PathBuf::from("minigramps-data")
    }
}

/// Sitzungsdatei im fixen Datenordner: merkt sich das zuletzt geöffnete
/// Projekt, damit die App es beim Start wieder öffnet.
fn session_path() -> PathBuf {
    default_library().join("letzte-sitzung.json")
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SessionState {
    data_path: String,
}

/// Zuletzt geöffnetes Projekt (None = keine Sitzung oder Datei weg).
pub fn load_last_project() -> Option<PathBuf> {
    let text = fs::read_to_string(session_path()).ok()?;
    let state: SessionState = serde_json::from_str(&text).ok()?;
    let path = PathBuf::from(state.data_path);
    path.is_file().then_some(path)
}

/// Pfad des zuletzt geöffneten Projekts merken (Fehler still ignorieren —
/// die Sitzung ist ein Komfort-Feature, kein Datenbestand).
pub fn save_last_project(path: &Path) {
    let state = SessionState {
        data_path: path.display().to_string(),
    };
    if let Ok(text) = serde_json::to_string(&state) {
        let _ = fs::write(session_path(), text);
    }
}

/// Projekte im MiniGramps-Datenordner und im Gramps-Dokumentenordner
/// finden (rekursiv, Tiefe 3).
pub fn discover_projects(library: &Path) -> Vec<PathBuf> {
    let mut folders = vec![library.to_path_buf(), default_library()];
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    if let Some(documents) =
        directories::UserDirs::new().and_then(|dirs| dirs.document_dir().map(Path::to_path_buf))
    {
        folders.push(documents.join("Gramps"));
    }
    let mut projects = Vec::new();
    for folder in folders {
        collect_project_files(&folder, &mut projects, 3);
    }
    projects.sort();
    projects
}

pub fn collect_project_files(folder: &Path, projects: &mut Vec<PathBuf>, depth: u8) {
    if let Ok(entries) = fs::read_dir(folder) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name_str = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
            // Sicherungskopien (`backups/`) sind keine Projekte.
            if name_str == "backups" {
                continue;
            }
            if path.is_dir() && depth > 0 {
                collect_project_files(&path, projects, depth - 1);
            }
            let is_metadata = name_str.ends_with(".layout.json")
                || name_str.ends_with(".manifest.json")
                || name_str == "letzte-sitzung.json"
                || name_str == "settings.json";
            let supported = !is_metadata
                && path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                    matches!(
                        e.to_ascii_lowercase().as_str(),
                        "json" | "ged" | "gedcom" | "gramps" | "xml"
                    )
                });
            if supported && !projects.contains(&path) {
                projects.push(path);
            }
        }
    }
}

pub fn manifest_path(data_path: &Path) -> PathBuf {
    let stem = data_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("projekt");
    data_path.with_file_name(format!("{stem}.manifest.json"))
}

pub fn load_project_manifest(data_path: &Path) -> Option<ProjectManifest> {
    let text = fs::read_to_string(manifest_path(data_path)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save_project_manifest(data_path: &Path, data: &TreeData) -> Result<PathBuf, String> {
    let manifest = ProjectManifest {
        format: "minigramps".into(),
        format_version: data.project.format_version,
        name: data.project.name.clone(),
        data_file: data_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("familienbaum.minigramps.json")
            .to_string(),
    };
    let path = manifest_path(data_path);
    let text = serde_json::to_string_pretty(&manifest).map_err(|error| error.to_string())?;
    fs::write(&path, text).map_err(|error| error.to_string())?;
    Ok(path)
}

/// Kurzer Anzeigename fuer den Oeffnen-Dialog. MiniGramps-JSON enthaelt den
/// vom Benutzer vergebenen Projektnamen; Fremdformate verwenden ihren
/// Dateinamen, ohne den vollstaendigen Speicherpfad offenzulegen.
pub fn project_display_name(path: &Path) -> String {
    if let Some(manifest) = load_project_manifest(path)
        && !manifest.name.trim().is_empty()
    {
        return manifest.name;
    }
    // Alte MiniGramps-Dateien besitzen noch kein Manifest. Dieser Fallback
    // verschwindet, sobald Projekte nur noch als `.mfg`/`.mmg` vorliegen.
    let is_json = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"));
    if is_json
        && let Ok(text) = fs::read_to_string(path)
        && let Ok(data) = serde_json::from_str::<TreeData>(&text)
        && !data.project.name.trim().is_empty()
        && data.project.name != "Unbenanntes Projekt"
    {
        return data.project.name;
    }
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("Unbenanntes Projekt")
        .strip_suffix(".minigramps")
        .unwrap_or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("Unbenanntes Projekt")
        })
        .to_string()
}

/// Datei lesen: Gramps-Sicherungen sind GZIP-gepackt, Kodierungen können
/// UTF-8/UTF-16/CP1252 sein. Nach der Dekodierung je nach Endung parsen;
/// anschließend Legacy-Kombinamen normalisieren (`normalize_names`).
pub fn load_file(path: &Path) -> Result<TreeData, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let bytes = gunzip_if_needed(bytes)?;
    let content = decode_bytes(&bytes);
    let mut data = match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "json" => serde_json::from_str(&content).map_err(|e| e.to_string())?,
        "ged" | "gedcom" => parse_gedcom(&content)?,
        "gramps" | "xml" => parse_gramps_xml(&content)?,
        _ => return Err("Nicht unterstütztes Dateiformat".into()),
    };
    normalize_names(&mut data);
    Ok(data)
}

/// Alte Projektdateien konsolidieren: kombinierte Namen ("Vorname
/// Nachname") in die getrennten Felder `given_name`/`family_name`
/// überführen (letztes Wort = Nachname) und das Legacy-Feld leeren.
pub fn normalize_names(data: &mut TreeData) {
    for person in &mut data.people {
        if person.given_name.is_empty() && person.family_name.is_empty() && !person.name.is_empty()
        {
            let tokens: Vec<&str> = person.name.split_whitespace().collect();
            if let Some((last, first)) = tokens.split_last() {
                person.family_name = last.to_string();
                person.given_name = first.join(" ");
            } else if let Some(single) = tokens.first() {
                person.given_name = single.to_string();
            }
        }
        person.name.clear();
    }
}

fn gunzip_if_needed(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    if !bytes.starts_with(&[0x1F, 0x8B]) {
        return Ok(bytes);
    }
    use std::io::Read as _;
    let mut output = Vec::new();
    flate2::read::GzDecoder::new(&bytes[..])
        .read_to_end(&mut output)
        .map_err(|error| format!("GZIP-Dekompression fehlgeschlagen: {error}"))?;
    Ok(output)
}

/// Kodierung erkennen: BOM (UTF-8/UTF-16), sonst UTF-16-Nullbyte-Muster,
/// sonst UTF-8, sonst Windows-1252 als Fallback.
fn decode_bytes(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return decode_utf16(&bytes[2..], true);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return decode_utf16(&bytes[2..], false);
    }
    if bytes.len() >= 4 {
        let sample = &bytes[..bytes.len().min(64)];
        let even_zeros = sample.iter().step_by(2).filter(|&&byte| byte == 0).count();
        let odd_zeros = sample
            .iter()
            .skip(1)
            .step_by(2)
            .filter(|&&byte| byte == 0)
            .count();
        if odd_zeros >= 4 && even_zeros == 0 {
            return decode_utf16(bytes, true);
        }
        if even_zeros >= 4 && odd_zeros == 0 {
            return decode_utf16(bytes, false);
        }
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_owned();
    }
    bytes.iter().map(|&byte| cp1252_char(byte)).collect()
}

fn decode_utf16(bytes: &[u8], little_endian: bool) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| {
            if little_endian {
                u16::from_le_bytes([pair[0], pair[1]])
            } else {
                u16::from_be_bytes([pair[0], pair[1]])
            }
        })
        .collect();
    String::from_utf16_lossy(&units)
}

fn cp1252_char(byte: u8) -> char {
    match byte {
        0x80 => '€',
        0x82 => '‚',
        0x83 => 'ƒ',
        0x84 => '„',
        0x85 => '…',
        0x86 => '†',
        0x87 => '‡',
        0x88 => 'ˆ',
        0x89 => '‰',
        0x8A => 'Š',
        0x8B => '‹',
        0x8C => 'Œ',
        0x8E => 'Ž',
        0x91 => '\u{2018}',
        0x92 => '\u{2019}',
        0x93 => '\u{201C}',
        0x94 => '\u{201D}',
        0x95 => '•',
        0x96 => '–',
        0x97 => '—',
        0x98 => '\u{02DC}',
        0x99 => '™',
        0x9A => 'š',
        0x9B => '›',
        0x9C => 'œ',
        0x9E => 'ž',
        0x9F => 'Ÿ',
        _ => byte as char,
    }
}

/// Aktuelles GEDCOM-Ereignis (falls vorhanden) in die Personen-Ereignisse
/// übernehmen und zurücksetzen.
fn flush_event(
    p: &mut Person,
    current_event: &mut Option<EventKind>,
    date: &mut String,
    place: &mut String,
    description: &mut String,
    notes: &mut Option<String>,
    sources: &mut Vec<SourceEntry>,
) {
    if let Some(kind) = current_event.take() {
        if !date.is_empty() || !place.is_empty() || !description.is_empty() || notes.is_some() || !sources.is_empty() {
            p.events.push(crate::model::Event {
                kind,
                date: std::mem::take(date),
                place: std::mem::take(place),
                description: std::mem::take(description),
                notes: notes.take(),
                sources: std::mem::take(sources),
                certainty: Certainty::Unset,
            });
        }
        date.clear();
        place.clear();
        description.clear();
        *notes = None;
        sources.clear();
    }
}

#[derive(Clone, Copy)]
enum GedcomNoteTarget {
    Person,
    Event,
    Family,
}

fn append_note(target: &mut Option<String>, text: &str, newline: bool) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    match target {
        Some(existing) => {
            if newline {
                existing.push('\n');
            }
            existing.push_str(text);
        }
        None => *target = Some(text.to_string()),
    }
}

/// FAM-Ereignis (MARR/DIV) abschließen: Datum/Ort nur merken — die
/// Übertragung auf beide Elternteile erfolgt NACH dem vollständigen Parsen,
/// denn Personen können erst später im Text auftauchen.
fn flush_family_event(
    family_index: usize,
    current: &mut Option<EventKind>,
    date: &mut String,
    place: &mut String,
    out: &mut Vec<(usize, EventKind, String, String)>,
) {
    if let Some(kind) = current.take() {
        if !date.is_empty() || !place.is_empty() {
            out.push((family_index, kind, std::mem::take(date), std::mem::take(place)));
        }
        date.clear();
        place.clear();
    }
}

/// GEDCOM-Parser: Zeilenzustandsmaschine. Erkennt Personen (INDI), Familien
/// (FAM), NAME/SEX sowie BIRT/DEAT-Ereignisse mit DATE und PLAC
/// (Ereignis-Kontext `current_event`). FAM-level MARR/DIV werden samt
/// DATE/PLAC gesammelt und nach dem Parsen auf beide Elternteile übertragen.
fn parse_gedcom(text: &str) -> Result<TreeData, String> {
    let mut data = TreeData::default();
    let mut current_person: Option<Person> = None;
    let mut current_family: Option<usize> = None;
    let mut current_event: Option<EventKind> = None;
    let mut current_event_date = String::new();
    let mut current_event_place = String::new();
    let mut current_event_description = String::new();
    let mut current_event_notes: Option<String> = None;
    let mut current_event_sources: Vec<SourceEntry> = Vec::new();
    let mut current_name = false;
    let mut current_note_target: Option<GedcomNoteTarget> = None;
    // Namensblöcke je Person: erster `1 NAME` = Hauptname, jeder weitere =
    // Alternativname (Gramps-Prinzip). `current_alt` lenkt die Subtags um.
    let mut name_seen = false;
    let mut current_alt: Option<usize> = None;
    // Medien: `0 @X@ OBJE`-Records (Datei+Titel) plus Verweise `1 OBJE @X@`
    // bzw. eingebettete `1 OBJE`/`2 FILE`-Blöcke je Person.
    let mut current_obje: Option<String> = None;
    let mut obje_file: String = String::new();
    let mut obje_title: String = String::new();
    let mut obje_records: HashMap<String, (String, String)> = HashMap::new();
    let mut person_obje: Vec<(String, String)> = Vec::new();
    let mut inline_obje_person: Option<String> = None;
    let mut inline_counter: usize = 0;
    // FAM-Ereigniskontext (MARR/DIV mit LEVEL-2 DATE/PLAC) + gesammelte
    // Ereignisse, die nach dem Parsen auf die Eltern übertragen werden.
    let mut current_family_event: Option<EventKind> = None;
    let mut current_family_event_date = String::new();
    let mut current_family_event_place = String::new();
    let mut pending_family_events: Vec<(usize, EventKind, String, String)> = Vec::new();
    for line in text.lines() {
        let part: Vec<_> = line.split_whitespace().collect();
        if part.len() < 2 {
            continue;
        }
        if part[0] == "0" {
            // FAM-Ereignis am Ende eines FAM-Records abschließen (der neue
            // RECORD wechselt danach `current_family`).
            if let Some(family_index) = current_family {
                flush_family_event(
                    family_index,
                    &mut current_family_event,
                    &mut current_family_event_date,
                    &mut current_family_event_place,
                    &mut pending_family_events,
                );
            }
            if let Some(ref mut person) = current_person {
                flush_event(
                    person,
                    &mut current_event,
                    &mut current_event_date,
                    &mut current_event_place,
                    &mut current_event_description,
                    &mut current_event_notes,
                    &mut current_event_sources,
                );
            }
            if let Some(person) = current_person.take() {
                let mut person = person;
                person.alt_names.retain(|alt| !alt.is_empty());
                data.people.push(person);
            }
            current_family = None;
            if let Some(obje_id) = current_obje.take() {
                if !obje_file.trim().is_empty() {
                    obje_records.insert(obje_id, (obje_file.clone(), obje_title.clone()));
                }
            }
            obje_file.clear();
            obje_title.clear();
            inline_obje_person = None;
            name_seen = false;
            current_alt = None;
            current_name = false;
            if part.get(2) == Some(&"INDI") {
                current_person = Some(person(
                    part[1].trim_matches('@'),
                    "",
                    "",
                    "",
                    Gender::Unknown,
                ));
            } else if part.get(2) == Some(&"OBJE") {
                current_obje = Some(part[1].trim_matches('@').to_string());
            } else if part.get(2) == Some(&"FAM") {
                let id = part[1].trim_matches('@').to_string();
                data.families.push(Family {
                    id,
                    parent_a: None,
                    parent_b: None,
                    children: vec![],
                    notes: None,
                    sources: Vec::new(),
                    certainty: Certainty::Unset,
                });
                current_family = Some(data.families.len() - 1);
            }
        } else if let Some(p) = current_person.as_mut() {
            match part.get(1).copied() {
                Some("NAME") => {
                    flush_event(
                        p,
                        &mut current_event,
                        &mut current_event_date,
                        &mut current_event_place,
                        &mut current_event_description,
                        &mut current_event_notes,
                        &mut current_event_sources,
                    );
                    let raw = part[2..].join(" ");
                    let (given, family) = if let Some((given, family)) = raw.split_once('/') {
                        (
                            given.trim().to_string(),
                            family.trim().trim_matches('/').to_string(),
                        )
                    } else {
                        let tokens: Vec<&str> = raw.split_whitespace().collect();
                        match tokens.split_last() {
                            Some((last, first)) => (first.join(" "), last.to_string()),
                            None => (String::new(), String::new()),
                        }
                    };
                    if !name_seen {
                        // Erster Namensblock = Hauptname.
                        p.given_name = given;
                        p.family_name = family;
                        name_seen = true;
                        current_alt = None;
                    } else {
                        // Jeder weitere Block = Alternativname (Gramps-Prinzip).
                        let mut alt = AlternativeName::default();
                        alt.given_name = given;
                        alt.family_name = family;
                        p.alt_names.push(alt);
                        current_alt = Some(p.alt_names.len() - 1);
                    }
                    current_name = true;
                    current_note_target = None;
                }
                Some("SEX") => {
                    if part[0] == "1" {
                        flush_event(
                            p,
                            &mut current_event,
                            &mut current_event_date,
                            &mut current_event_place,
                            &mut current_event_description,
                            &mut current_event_notes,
                            &mut current_event_sources,
                        );
                        current_name = false;
                        current_alt = None;
                        current_note_target = None;
                    }
                    p.gender = match part.get(2) {
                        Some(&"M") => Gender::Male,
                        Some(&"F") => Gender::Female,
                        _ => Gender::Unknown,
                    };
                }
                Some("GIVN") if current_name => {
                    let value = part.get(2..).map(|text| text.join(" ")).unwrap_or_default();
                    match current_alt.and_then(|index| p.alt_names.get_mut(index)) {
                        Some(alt) => alt.given_name = value,
                        None => p.given_name = value,
                    }
                }
                Some("SURN") if current_name => {
                    let value = part.get(2..).map(|text| text.join(" ")).unwrap_or_default();
                    match current_alt.and_then(|index| p.alt_names.get_mut(index)) {
                        Some(alt) => alt.family_name = value,
                        None => p.family_name = value,
                    }
                }
                Some("NPFX") if current_name => {
                    let value = part.get(2..).map(|text| text.join(" ")).unwrap_or_default();
                    match current_alt.and_then(|index| p.alt_names.get_mut(index)) {
                        Some(alt) => alt.name_prefix = value,
                        None => p.name_prefix = value,
                    }
                }
                Some("SPFX") if current_name => {
                    let value = part.get(2..).map(|text| text.join(" ")).unwrap_or_default();
                    match current_alt.and_then(|index| p.alt_names.get_mut(index)) {
                        Some(alt) => alt.surname_prefix = value,
                        None => p.surname_prefix = value,
                    }
                }
                Some("NSFX") if current_name => {
                    let value = part.get(2..).map(|text| text.join(" ")).unwrap_or_default();
                    match current_alt.and_then(|index| p.alt_names.get_mut(index)) {
                        Some(alt) => alt.suffix = value,
                        None => p.suffix = value,
                    }
                }
                Some("NICK") if current_name || part[0] == "1" => {
                    let value = part.get(2..).map(|text| text.join(" ")).unwrap_or_default();
                    match current_alt.and_then(|index| p.alt_names.get_mut(index)) {
                        Some(alt) => alt.nick_name = value,
                        None => p.nick_name = value,
                    }
                }
                Some("TYPE") if current_name => {
                    let value = part.get(2..).map(|text| text.join(" ")).unwrap_or_default();
                    match current_alt.and_then(|index| p.alt_names.get_mut(index)) {
                        Some(alt) => alt.name_type = value,
                        None => p.name_type = value,
                    }
                }
                Some("TITL") if part[0] == "1" => {
                    flush_event(
                        p,
                        &mut current_event,
                        &mut current_event_date,
                        &mut current_event_place,
                        &mut current_event_description,
                        &mut current_event_notes,
                        &mut current_event_sources,
                    );
                    p.title = part.get(2..).map(|text| text.join(" ")).unwrap_or_default();
                    current_name = false;
                    current_alt = None;
                    current_note_target = None;
                }
                Some("NOTE") => {
                    if part[0] == "1" {
                        flush_event(
                            p,
                            &mut current_event,
                            &mut current_event_date,
                            &mut current_event_place,
                            &mut current_event_description,
                            &mut current_event_notes,
                            &mut current_event_sources,
                        );
                        let line = part.get(2..).map(|text| text.join(" ")).unwrap_or_default();
                        if !line.trim().is_empty() {
                            if !p.notes.trim().is_empty() {
                                p.notes.push('\n');
                            }
                            p.notes.push_str(line.trim());
                        }
                        current_name = false;
                        current_alt = None;
                        current_note_target = Some(GedcomNoteTarget::Person);
                    } else if current_event.is_some() {
                        let line = part.get(2..).map(|text| text.join(" ")).unwrap_or_default();
                        append_note(&mut current_event_notes, &line, true);
                        current_note_target = Some(GedcomNoteTarget::Event);
                    }
                }
                Some("SOUR") => {
                    let Some(value) = part.get(2) else {
                        continue;
                    };
                    let entry = SourceEntry {
                        title: value.trim_matches('@').to_string(),
                        detail: String::new(),
                        media: None,
                    };
                    if part[0] == "1" {
                        flush_event(
                            p,
                            &mut current_event,
                            &mut current_event_date,
                            &mut current_event_place,
                            &mut current_event_description,
                            &mut current_event_notes,
                            &mut current_event_sources,
                        );
                        if !p.sources.contains(&entry) {
                            p.sources.push(entry);
                        }
                        current_name = false;
                        current_alt = None;
                        current_note_target = None;
                    } else if current_event.is_some() && !current_event_sources.contains(&entry) {
                        current_event_sources.push(entry);
                    }
                }
                Some("CONT" | "CONC") => {
                    let line = part.get(2..).map(|text| text.join(" ")).unwrap_or_default();
                    let newline = part.get(1) == Some(&"CONT");
                    match current_note_target {
                        Some(GedcomNoteTarget::Person) => {
                            if newline && !p.notes.is_empty() {
                                p.notes.push('\n');
                            }
                            p.notes.push_str(line.trim());
                        }
                        Some(GedcomNoteTarget::Event) => {
                            append_note(&mut current_event_notes, &line, newline);
                        }
                        _ => {}
                    }
                }
                Some("TYPE") if current_event.is_some() => {
                    let value = part.get(2..).map(|text| text.join(" ")).unwrap_or_default();
                    if let Some(EventKind::Custom(label)) = &mut current_event {
                        if label == "EVEN" || label.is_empty() {
                            *label = value;
                        }
                    } else if !value.trim().is_empty() {
                        current_event_description = value;
                    }
                }
                Some("CAUS" | "AGNC") if current_event.is_some() => {
                    let value = part.get(2..).map(|text| text.join(" ")).unwrap_or_default();
                    if !value.trim().is_empty() {
                        if !current_event_description.is_empty() {
                            current_event_description.push_str("; ");
                        }
                        current_event_description.push_str(value.trim());
                    }
                }
                Some(
                    tag @ ("BIRT" | "DEAT" | "MARR" | "DIV" | "BAPM" | "CHR" | "BURI" | "CREM"
                    | "OCCU" | "RESI" | "IMMI" | "EMIG" | "CENS" | "GRAD" | "EDUC" | "RETI"
                    | "ADOP" | "CONFIRM" | "FCOM" | "ORDN" | "PROB" | "PROP" | "WILL" | "EVEN"),
                ) => {
                    flush_event(
                        p,
                        &mut current_event,
                        &mut current_event_date,
                        &mut current_event_place,
                        &mut current_event_description,
                        &mut current_event_notes,
                        &mut current_event_sources,
                    );
                    current_event = Some(EventKind::from_gedcom(tag));
                    current_event_description = part.get(2..).map(|text| text.join(" ")).unwrap_or_default();
                    current_name = false;
                    current_alt = None;
                    current_note_target = None;
                }
                Some("DATE") => {
                    if current_event.is_some() {
                        current_event_date = part[2..].join(" ");
                        if current_event == Some(EventKind::Birth) {
                            p.birth = current_event_date.clone();
                        } else if current_event == Some(EventKind::Death) {
                            p.death = current_event_date.clone();
                        }
                    }
                }
                Some("PLAC") => {
                    if current_event.is_some() {
                        let place = part[2..].join(" ");
                        current_event_place = place.clone();
                        if current_event == Some(EventKind::Birth) {
                            p.birth_place = place.clone();
                        } else if current_event == Some(EventKind::Death) {
                            p.death_place = place.clone();
                        }
                    }
                }
                Some("OBJE") if part[0] == "1" => {
                    match part.get(2).map(|value| value.trim_matches('@')) {
                        Some(target) if !target.is_empty() && !target.contains(' ') => {
                            // Verweis auf einen `0 @X@ OBJE`-Record.
                            person_obje.push((p.id.clone(), target.to_string()));
                            inline_obje_person = None;
                        }
                        _ => {
                            // Eingebetteter Block (`1 OBJE` + `2 FILE …`).
                            inline_obje_person = Some(p.id.clone());
                        }
                    }
                    current_name = false;
                    current_alt = None;
                    current_note_target = None;
                }
                Some("FILE") if part[0] == "2" && inline_obje_person.is_some() => {
                    let file = part.get(2..).map(|text| text.join(" ")).unwrap_or_default();
                    if !file.trim().is_empty() {
                        inline_counter += 1;
                        let key = format!("inline:{inline_counter}");
                        obje_records.insert(key.clone(), (file, String::new()));
                        if let Some(pid) = inline_obje_person.clone() {
                            person_obje.push((pid, key));
                        }
                    }
                }
                Some("TITL") if part[0] == "2" && inline_obje_person.is_some() => {
                    let title = part.get(2..).map(|text| text.join(" ")).unwrap_or_default();
                    if let Some((_, last)) = person_obje.last() {
                        if let Some(record) = obje_records.get_mut(last) {
                            record.1 = title;
                        }
                    }
                }
                _ => {
                    if part[0] == "1" {
                        flush_event(
                            p,
                            &mut current_event,
                            &mut current_event_date,
                            &mut current_event_place,
                            &mut current_event_description,
                            &mut current_event_notes,
                            &mut current_event_sources,
                        );
                        current_name = false;
                        current_alt = None;
                        current_note_target = None;
                        inline_obje_person = None;
                    }
                }
            }
        }
        // FAM-Level: HUSB/WIFE/CHIL (mit Wert), FAM-Ereignisse MARR/DIV mit
        // LEVEL-2 DATE/PLAC sowie NOTE/SOUR für die Familie.
        if let Some(family_index) = current_family {
            match part[0] {
                "1" => {
                    // Jedes neue Level-1-Tag beendet ein laufendes FAM-Ereignis.
                    flush_family_event(
                        family_index,
                        &mut current_family_event,
                        &mut current_family_event_date,
                        &mut current_family_event_place,
                        &mut pending_family_events,
                    );
                    let f = &mut data.families[family_index];
                    match part.get(1).copied() {
                        Some("HUSB") => {
                            if let Some(value) = part.get(2) {
                                f.parent_a = Some(value.trim_matches('@').to_string());
                            }
                            current_note_target = None;
                        }
                        Some("WIFE") => {
                            if let Some(value) = part.get(2) {
                                f.parent_b = Some(value.trim_matches('@').to_string());
                            }
                            current_note_target = None;
                        }
                        Some("CHIL") => {
                            if let Some(value) = part.get(2) {
                                f.children.push(value.trim_matches('@').to_string());
                            }
                            current_note_target = None;
                        }
                        Some(tag @ ("MARR" | "DIV")) => {
                            current_family_event = Some(EventKind::from_gedcom(tag));
                            current_note_target = None;
                        }
                        Some("NOTE") => {
                            if let Some(text) = part.get(2..) {
                                let line = text.join(" ").trim().to_string();
                                if !line.is_empty() {
                                    match &mut f.notes {
                                        Some(existing) => {
                                            existing.push_str(&format!("\n{line}"))
                                        }
                                        None => f.notes = Some(line),
                                    }
                                }
                            }
                            current_note_target = Some(GedcomNoteTarget::Family);
                        }
                        Some("SOUR") => {
                            if let Some(value) = part.get(2) {
                                let entry = SourceEntry {
                                    title: value.trim_matches('@').to_string(),
                                    detail: String::new(),
                                    media: None,
                                };
                                if !f
                                    .sources
                                    .iter()
                                    .any(|existing| existing.title == entry.title)
                                {
                                    f.sources.push(entry);
                                }
                            }
                            current_note_target = None;
                        }
                        _ => {}
                    }
                }
                "2" => {
                    if matches!(part.get(1).copied(), Some("CONT" | "CONC"))
                        && matches!(current_note_target, Some(GedcomNoteTarget::Family))
                    {
                        let line = part.get(2..).map(|text| text.join(" ")).unwrap_or_default();
                        if !line.trim().is_empty() {
                            let f = &mut data.families[family_index];
                            match &mut f.notes {
                                Some(existing) => {
                                    if part.get(1) == Some(&"CONT") {
                                        existing.push('\n');
                                    }
                                    existing.push_str(line.trim());
                                }
                                None => f.notes = Some(line.trim().to_string()),
                            }
                        }
                    }
                    if current_family_event.is_some() {
                        match part.get(1).copied() {
                            Some("DATE") => {
                                if let Some(text) = part.get(2..) {
                                    current_family_event_date = text.join(" ");
                                }
                            }
                            Some("PLAC") => {
                                if let Some(text) = part.get(2..) {
                                    current_family_event_place = text.join(" ");
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        // OBJE-Records (`0 @X@ OBJE`): Datei + Titel sammeln, Verweise wurden
        // oben je Person gesammelt (`1 OBJE @X@` bzw. eingebettet).
        if current_obje.is_some() {
            match (part[0], part.get(1).copied()) {
                ("1", Some("FILE")) => {
                    let file = part.get(2..).map(|text| text.join(" ")).unwrap_or_default();
                    if !file.trim().is_empty() && obje_file.trim().is_empty() {
                        obje_file = file;
                    }
                }
                ("1", Some("TITL")) => {
                    let title = part.get(2..).map(|text| text.join(" ")).unwrap_or_default();
                    if !title.trim().is_empty() && obje_title.trim().is_empty() {
                        obje_title = title;
                    }
                }
                _ => {}
            }
        }
    }
    if let Some(mut person) = current_person {
        flush_event(
            &mut person,
            &mut current_event,
            &mut current_event_date,
            &mut current_event_place,
            &mut current_event_description,
            &mut current_event_notes,
            &mut current_event_sources,
        );
        person.alt_names.retain(|alt| !alt.is_empty());
        data.people.push(person);
    }
    // Letzten OBJE-Record am Dateiende abschließen.
    if let Some(obje_id) = current_obje {
        if !obje_file.trim().is_empty() {
            obje_records.insert(obje_id, (obje_file.clone(), obje_title.clone()));
        }
    }
    // FAM-Ereignisse am Dateiende abschließen und auf beide Elternteile
    // übertragen (MARR/DIV als Ereignis je Elternteil + Beziehungsart).
    if let Some(family_index) = current_family {
        flush_family_event(
            family_index,
            &mut current_family_event,
            &mut current_family_event_date,
            &mut current_family_event_place,
            &mut pending_family_events,
        );
    }
    for (family_index, kind, date, place) in pending_family_events {
        if let Some(family) = data.families.get(family_index) {
            for id in [family.parent_a.as_deref(), family.parent_b.as_deref()]
                .into_iter()
                .flatten()
            {
                if let Some(person) = data.people.iter_mut().find(|person| person.id == id) {
                    let duplicate = person.events.iter().any(|event| {
                        event.kind == kind && event.date == date && event.place == place
                    });
                    if !duplicate {
                        person.events.push(Event {
                            kind: kind.clone(),
                            date: date.clone(),
                            place: place.clone(),
                            description: String::new(),
                            notes: None,
                            sources: Vec::new(),
                            certainty: Certainty::Unset,
                        });
                    }
                }
            }
            match kind {
                EventKind::Marriage => {
                    data.partner_relations
                        .insert(family.id.clone(), PartnerRelation::Married);
                }
                EventKind::Divorce => {
                    data.partner_relations
                        .insert(family.id.clone(), PartnerRelation::Divorced);
                }
                _ => {}
            }
        }
    }
    // OBJE-Medien auf Personen verteilen: erstes Bild = Profilfoto (falls
    // leer), weitere Bilder = Galerie, Nicht-Bilder = Dokumente. Die Pfade
    // bleiben zunächst relativ zum GEDCOM-Ordner; `rebase_media_files` kopiert
    // sie beim Anhängen ins Zielprojekt.
    for (person_id, obje_id) in &person_obje {
        let Some((file, title)) = obje_records.get(obje_id) else {
            continue;
        };
        if file.trim().is_empty() {
            continue;
        }
        let Some(person) = data
            .people
            .iter_mut()
            .find(|person| &person.id == person_id)
        else {
            continue;
        };
        if obje_is_image(file) {
            let photo_empty = person
                .photo
                .as_deref()
                .is_none_or(|photo| photo.trim().is_empty());
            if photo_empty {
                person.photo = Some(file.clone());
            } else if !person.gallery.iter().any(|entry| entry == file) {
                person.gallery.push(file.clone());
            }
        } else {
            let name = if title.trim().is_empty() {
                std::path::Path::new(file)
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or(file)
                    .to_string()
            } else {
                title.clone()
            };
            if !person
                .documents
                .iter()
                .any(|document| document.path == *file)
            {
                person.documents.push(DocumentEntry {
                    path: file.clone(),
                    name,
                });
            }
        }
    }
    if data.people.is_empty() {
        Err("Keine Personen in GEDCOM gefunden".into())
    } else {
        Ok(data)
    }
}

/// Bilddatei anhand der Endung erkennen (OBJE-Verteilung: Bild =
/// Foto/Galerie, Rest = Dokument).
fn obje_is_image(path: &str) -> bool {
    matches!(
        std::path::Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "tif" | "tiff" | "heic" | "heif"
            | "avif"
    )
}

/// Gramps-XML-Parser (auch GZIP-gepackte Sicherungen, siehe `load_file`).
/// Die DTD-Deklaration wird erlaubt (`allow_dtd`). Geburts-/Sterbedaten
/// stehen in `<event>`-Datensätzen und werden über `<eventref hlink>` den
/// Personen zugeordnet.
fn parse_gramps_xml(text: &str) -> Result<TreeData, String> {
    let doc = Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        },
    )
    .map_err(|e| e.to_string())?;
    let mut data = TreeData::default();
    // Gramps-Quellen (Titel aus stitle/sauthor/spubinfo) und Citations
    // (Quellverweis + Seite), den Personen über citationref zugeordnet.
    let mut source_titles: HashMap<String, String> = HashMap::new();
    for node in doc.descendants().filter(|n| n.has_tag_name("source")) {
        let handle = node.attribute("handle").unwrap_or_default().to_string();
        let text = |tag: &str| {
            node.descendants()
                .find(|n| n.has_tag_name(tag))
                .and_then(|n| n.text())
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let title = text("stitle");
        let author = text("sauthor");
        let pubinfo = text("spubinfo");
        let mut full = title;
        for part in [author, pubinfo] {
            if !part.is_empty() {
                if !full.is_empty() {
                    full.push_str(" — ");
                }
                full.push_str(&part);
            }
        }
        if !full.is_empty() {
            source_titles.insert(handle, full);
        }
    }
    let mut citation_targets: HashMap<String, (String, String)> = HashMap::new();
    for node in doc.descendants().filter(|n| n.has_tag_name("citation")) {
        let handle = node.attribute("handle").unwrap_or_default().to_string();
        let source = node
            .children()
            .find(|n| n.has_tag_name("sourceref"))
            .and_then(|n| n.attribute("hlink"))
            .unwrap_or_default()
            .to_string();
        let page = node
            .descendants()
            .find(|n| n.has_tag_name("page"))
            .and_then(|n| n.text())
            .unwrap_or("")
            .trim()
            .to_string();
        if !handle.is_empty() {
            citation_targets.insert(handle, (source, page));
        }
    }
    let mut events: HashMap<String, (String, String, String, String)> = HashMap::new();
    for node in doc.descendants().filter(|n| n.has_tag_name("event")) {
        let handle = node.attribute("handle").unwrap_or_default().to_string();
        let event_type = node.attribute("type").unwrap_or_default().to_string();
        let date = event_date(node);
        let place = node
            .descendants()
            .find(|n| n.has_tag_name("place"))
            .and_then(|n| n.text())
            .unwrap_or("")
            .to_string();
        let desc = node
            .descendants()
            .find(|n| n.has_tag_name("description"))
            .and_then(|n| n.text())
            .unwrap_or("")
            .to_string();
        events.insert(handle, (event_type, date.unwrap_or_default(), place, desc));
    }
    for node in doc.descendants().filter(|n| n.has_tag_name("person")) {
        let id = node.attribute("handle").unwrap_or_default().to_string();
        let gender = match node.attribute("gender") {
            Some("M") => Gender::Male,
            Some("F") => Gender::Female,
            _ => Gender::Unknown,
        };
        let name = node
            .descendants()
            .find(|n| n.has_tag_name("first"))
            .and_then(|n| n.text())
            .unwrap_or("Unbenannt");
        let surname = node
            .descendants()
            .find(|n| n.has_tag_name("surname"))
            .and_then(|n| n.text())
            .unwrap_or("");
        let mut birth = String::new();
        let mut death = String::new();
        let mut person_events = Vec::new();
        for reference in node.children().filter(|n| n.has_tag_name("eventref")) {
            let Some(link) = reference.attribute("hlink") else {
                continue;
            };
            if let Some((event_type, date, place, desc)) = events.get(link) {
                let kind = EventKind::from_gramps(event_type);
                if kind == EventKind::Birth {
                    birth = date.clone();
                } else if kind == EventKind::Death {
                    death = date.clone();
                }
                if !date.is_empty() || !place.is_empty() || !desc.is_empty() {
                    person_events.push(crate::model::Event {
                        kind,
                        date: date.clone(),
                        place: place.clone(),
                        description: desc.clone(),
                        notes: None,
                        sources: Vec::new(),
                        certainty: Certainty::Unset,
                    });
                }
            }
        }
        let mut person_sources = Vec::new();
        for reference in node.children().filter(|n| n.has_tag_name("citationref")) {
            let Some(link) = reference.attribute("hlink") else {
                continue;
            };
            if let Some((source, page)) = citation_targets.get(link) {
                if let Some(title) = source_titles.get(source.as_str()) {
                    let entry = crate::model::SourceEntry {
                        title: title.clone(),
                        detail: page.clone(),
                        media: None,
                    };
                    if !person_sources.contains(&entry) {
                        person_sources.push(entry);
                    }
                }
            }
        }
        data.people.push(person(&id, name, surname, &birth, gender));
        if let Some(p) = data.people.last_mut() {
            p.death = death;
            p.events = person_events;
            p.sources = person_sources;
        }
    }
    for node in doc.descendants().filter(|n| n.has_tag_name("family")) {
        let mut f = Family {
            id: node.attribute("handle").unwrap_or_default().into(),
            parent_a: None,
            parent_b: None,
            children: vec![],
            notes: None,
            sources: Vec::new(),
            certainty: Certainty::Unset,
        };
        for child in node.children() {
            match child.tag_name().name() {
                "father" => f.parent_a = child.attribute("hlink").map(str::to_string),
                "mother" => f.parent_b = child.attribute("hlink").map(str::to_string),
                "childref" => {
                    if let Some(id) = child.attribute("hlink") {
                        f.children.push(id.into())
                    }
                }
                _ => {}
            }
        }
        data.families.push(f);
    }
    if data.people.is_empty() {
        Err("Keine Personen in Gramps-XML gefunden".into())
    } else {
        Ok(data)
    }
}

/// Datum aus einem Gramps-Ereignisknoten ziehen (dateval/datestr/
/// daterange/datespan; bevorzugt `val`, sonst `start`/`stop`).
fn event_date(node: roxmltree::Node) -> Option<String> {
    for tag in ["dateval", "datestr", "daterange", "datespan"] {
        if let Some(date) = node.descendants().find(|n| n.has_tag_name(tag)) {
            if let Some(value) = date
                .attribute("val")
                .or_else(|| date.attribute("start"))
                .or_else(|| date.attribute("stop"))
            {
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf16_and_cp1252() {
        let utf16_le: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("Jürgen".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(decode_bytes(&utf16_le), "Jürgen");
        assert_eq!(decode_bytes(b"J\xFCrgen"), "Jürgen");
        assert_eq!(decode_bytes("\u{FEFF}Jürgen".as_bytes()), "Jürgen");
    }

    #[test]
    fn decodes_utf16_without_bom() {
        let utf16_le: Vec<u8> = "<?xml".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert!(decode_bytes(&utf16_le).starts_with("<?xml"));
    }

    #[test]
    fn loads_gzipped_gramps_backup() {
        let xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE database PUBLIC \"-//Gramps//DTD Gramps XML 1.7.2//EN\" \"http://gramps-project.org/xml/1.7.2/grampsxml.dtd\">\n<database><person handle=\"h1\" gender=\"M\"><name><first>Jonas</first><surname>Doe</surname></name></person></database>";
        use std::io::Write as _;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(xml.as_bytes()).unwrap();
        let gzipped = encoder.finish().unwrap();
        let decompressed = gunzip_if_needed(gzipped).unwrap();
        let data = parse_gramps_xml(&decode_bytes(&decompressed)).unwrap();
        assert_eq!(data.people[0].display_name(), "Jonas Doe");
    }

    #[test]
    fn imports_gramps_event_dates_and_gender() {
        let xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><database>\
            <event handle=\"_E1\" type=\"Birth\"><dateval val=\"1971\"/></event>\
            <event handle=\"_E2\" type=\"Death\"><dateval val=\"2020\"/></event>\
            <person handle=\"_I1\" gender=\"M\"><name><first>Jonas</first><surname>Doe</surname></name>\
            <eventref hlink=\"_E1\" role=\"Primary\"/><eventref hlink=\"_E2\" role=\"Primary\"/></person>\
            </database>";
        let data = parse_gramps_xml(xml).unwrap();
        assert_eq!(data.people[0].gender, Gender::Male);
        assert_eq!(data.people[0].birth, "1971");
        assert_eq!(data.people[0].death, "2020");
        assert_eq!(data.people[0].events.len(), 2);
        assert_eq!(
            data.people[0].events[0].kind,
            crate::model::EventKind::Birth
        );
        assert_eq!(data.people[0].events[0].date, "1971");
    }

    #[test]
    fn imports_gedcom_dates_and_gender() {
        let data = parse_gedcom(
            "0 @I1@ INDI\n1 NAME Jürgen /Muster/\n1 SEX M\n1 BIRT\n2 DATE 12 MAR 1950\n2 PLAC Hagen\n1 DEAT\n2 DATE 2001\n0 @I2@ INDI\n1 NAME Anna /Muster/\n1 SEX F\n",
        )
        .unwrap();
        let juergen = &data.people[0];
        assert_eq!(juergen.gender, Gender::Male);
        assert_eq!(juergen.birth, "12 MAR 1950");
        assert_eq!(juergen.birth_place, "Hagen");
        assert_eq!(juergen.death, "2001");
        assert_eq!(data.people[1].gender, Gender::Female);
    }

    #[test]
    fn imports_gedcom_multiple_events() {
        let data = parse_gedcom(
            "0 @I1@ INDI\n1 NAME Jürgen /Muster/\n1 SEX M\n1 BIRT\n2 DATE 12 MAR 1950\n2 PLAC Hagen\n1 OCCU\n2 DATE 1975\n1 RESI\n2 PLAC Berlin\n0 @I2@ INDI\n1 NAME Anna /Muster/\n1 SEX F\n",
        )
        .unwrap();
        let juergen = &data.people[0];
        let kinds: Vec<String> = juergen
            .events
            .iter()
            .map(|e| e.kind.label().to_string())
            .collect();
        assert!(
            kinds.contains(&"Geburt".to_string()),
            "kein Geburtsevent: {kinds:?}"
        );
        assert!(
            kinds.contains(&"Beruf".to_string()),
            "kein Berufsevent: {kinds:?}"
        );
        assert!(
            kinds.contains(&"Wohnort".to_string()),
            "kein Wohnortevent: {kinds:?}"
        );
        let occ = juergen
            .events
            .iter()
            .find(|e| e.kind.label() == "Beruf")
            .unwrap();
        assert_eq!(occ.date, "1975");
    }

    #[test]
    fn imports_gedcom_person_notes_sources_names_and_event_details() {
        let data = parse_gedcom(
            "0 @I1@ INDI\n1 NAME Johann /Muster/\n2 GIVN Johann Peter\n2 SURN Mustermann\n2 NPFX Dr.\n2 SPFX von\n2 NSFX jr.\n2 NICK Hannes\n2 TYPE Geburtsname\n1 TITL Prof.\n1 NOTE Erste Zeile\n2 CONT zweite Zeile\n1 SOUR @S1@\n1 OCCU Bäcker\n2 NOTE aus Meisterbrief\n2 SOUR @S2@\n1 EVEN\n2 TYPE Konfirmation\n2 DATE 1900\n",
        )
        .unwrap();
        let person = data.find("I1").unwrap();
        assert_eq!(person.given_name, "Johann Peter");
        assert_eq!(person.family_name, "Mustermann");
        assert_eq!(person.name_prefix, "Dr.");
        assert_eq!(person.surname_prefix, "von");
        assert_eq!(person.suffix, "jr.");
        assert_eq!(person.nick_name, "Hannes");
        assert_eq!(person.name_type, "Geburtsname");
        assert_eq!(person.title, "Prof.");
        assert_eq!(person.notes, "Erste Zeile\nzweite Zeile");
        assert_eq!(person.sources[0].title, "S1");
        let occ = person
            .events
            .iter()
            .find(|event| event.kind == EventKind::Occupation)
            .unwrap();
        assert_eq!(occ.description, "Bäcker");
        assert_eq!(occ.notes.as_deref(), Some("aus Meisterbrief"));
        assert_eq!(occ.sources[0].title, "S2");
        let custom = person
            .events
            .iter()
            .find(|event| event.kind == EventKind::Custom("Konfirmation".into()))
            .unwrap();
        assert_eq!(custom.date, "1900");
    }

    #[test]
    fn imports_gedcom_family_relationships() {
        let data = parse_gedcom("0 @I1@ INDI\n1 NAME Alex /Muster/\n1 SEX M\n0 @I2@ INDI\n1 NAME Bea /Muster/\n1 SEX F\n0 @I3@ INDI\n1 NAME Chris /Muster/\n0 @F1@ FAM\n1 HUSB @I1@\n1 WIFE @I2@\n1 CHIL @I3@\n").unwrap();
        assert_eq!(data.people.len(), 3);
        assert_eq!(data.parents_of("I3").len(), 2);
        assert_eq!(data.children_of("I1")[0].display_name(), "Chris Muster");
        assert_eq!(data.partners_of("I1")[0].display_name(), "Bea Muster");
    }

    #[test]
    fn imports_gedcom_alternative_names() {
        // Zweiter `1 NAME`-Block = Alternativname (Gramps-Prinzip), Subtags
        // lenken auf den jeweiligen Eintrag um.
        let data = parse_gedcom(
            "0 @I1@ INDI\n1 NAME Johann /Bauke/\n1 NAME Johann /Kassner/\n2 TYPE Ehename\n2 GIVN Johann Friedrich\n",
        )
        .unwrap();
        let person = data.find("I1").unwrap();
        assert_eq!(person.given_name, "Johann");
        assert_eq!(person.family_name, "Bauke");
        assert_eq!(person.alt_names.len(), 1);
        assert_eq!(person.alt_names[0].given_name, "Johann Friedrich");
        assert_eq!(person.alt_names[0].family_name, "Kassner");
        assert_eq!(person.alt_names[0].name_type, "Ehename");
    }

    #[test]
    fn imports_gedcom_obje_media_to_photo_gallery_and_documents() {
        let data = parse_gedcom(
            "0 @I1@ INDI\n1 NAME A /B/\n1 OBJE @O1@\n1 OBJE @O2@\n1 OBJE @O3@\n0 @O1@ OBJE\n1 FILE fotos/portrait.jpg\n1 TITL Portrait\n0 @O2@ OBJE\n1 FILE fotos/gruppe.png\n0 @O3@ OBJE\n1 FILE dokumente/urkunde.pdf\n1 TITL Urkunde\n",
        )
        .unwrap();
        let person = data.find("I1").unwrap();
        assert_eq!(person.photo.as_deref(), Some("fotos/portrait.jpg"));
        assert_eq!(person.gallery, vec!["fotos/gruppe.png".to_string()]);
        assert_eq!(person.documents.len(), 1);
        assert_eq!(person.documents[0].path, "dokumente/urkunde.pdf");
        assert_eq!(person.documents[0].name, "Urkunde");
    }

    #[test]
    fn imports_gedcom_inline_obje() {
        let data = parse_gedcom(
            "0 @I1@ INDI\n1 NAME A /B/\n1 OBJE\n2 FILE bilder/inline.jpg\n2 TITL Inlinebild\n",
        )
        .unwrap();
        let person = data.find("I1").unwrap();
        assert_eq!(person.photo.as_deref(), Some("bilder/inline.jpg"));
    }

    #[test]
    fn imports_gedcom_marriage_from_family() {
        // FAM-level MARR mit DATE/PLAC (Standard-GEDCOM) muss als
        // Beziehungsereignis auf BEIDEN Partnern landen + Beziehungsart.
        let data = parse_gedcom(
            "0 @I1@ INDI\n1 NAME Alex /Muster/\n1 SEX M\n0 @I2@ INDI\n1 NAME Bea /Muster/\n1 SEX F\n0 @F1@ FAM\n1 HUSB @I1@\n1 WIFE @I2@\n1 MARR\n2 DATE 10 MAY 1811\n2 PLAC Zielenzig\n",
        )
        .unwrap();
        for id in ["I1", "I2"] {
            let person = data.find(id).unwrap();
            let marr = person
                .events
                .iter()
                .find(|e| e.kind == EventKind::Marriage)
                .unwrap_or_else(|| panic!("kein Heiratsevent bei {id}"));
            assert_eq!(marr.date, "10 MAY 1811");
            assert_eq!(marr.place, "Zielenzig");
        }
        data.partners_of("I1")
            .iter()
            .find(|person| person.id == "I2")
            .expect("Partner I2 fehlt");
        assert_eq!(
            data.partner_relations.get("F1"),
            Some(&PartnerRelation::Married)
        );
    }

    #[test]
    fn imports_gedcom_real_aschenborn_marriage() {
        // Exakte Struktur der Datei Aschenborn_Kassner_32_33.ged: FAM-Record
        // kommt NACH den Personen, MARR am Ende des FAM-Records.
        let ged = "\
0 HEAD
1 SOUR Genealogische_Ahnentafel
1 CHAR UTF-8
1 GEDC
2 VERS 5.5.1
2 FORM LINEAGE-LINKED
0 @I32@ INDI
1 NAME Karl Heinrich Adolf /Aschenborn/
1 SEX M
1 BIRT
2 DATE 25 NOV 1779
2 PLAC Finsterwalde
1 DEAT
2 DATE 27 FEB 1847
2 PLAC Schweidnitz
1 FAMC @F64@
1 FAMS @F32@
0 @I33@ INDI
1 NAME Wilhelmine Ernestine Antoinette /Kaßner/
1 SEX F
1 BIRT
2 DATE 1785
2 PLAC Hermsdorf
1 DEAT
2 DATE 24 OCT 1814
2 PLAC Schweidnitz
1 FAMS @F32@
1 NOTE Geburtsname: Kaßner od. Kassner.
0 @I64@ INDI
1 NAME Georg Karl /Aschenborn/
1 SEX M
1 FAMS @F64@
0 @F32@ FAM
1 HUSB @I32@
1 WIFE @I33@
1 MARR
2 DATE 10 MAY 1811
2 PLAC Zielenzig
0 TRLR
";
        let data = parse_gedcom(ged).unwrap();
        for id in ["I32", "I33"] {
            let person = data.find(id).unwrap();
            let marr = person
                .events
                .iter()
                .find(|e| e.kind == EventKind::Marriage)
                .unwrap_or_else(|| panic!("kein Heiratsevent bei {id}"));
            assert_eq!(marr.date, "10 MAY 1811");
            assert_eq!(marr.place, "Zielenzig");
        }
        assert_eq!(
            data.partner_relations.get("F32"),
            Some(&PartnerRelation::Married)
        );
    }

    #[test]
    fn uses_manifest_name_for_project_list() {
        let folder = std::env::temp_dir().join("minigramps-manifest-test");
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        let data_path = folder.join("familienbaum.minigramps.json");
        let mut data = TreeData::demo();
        data.project.name = "Familie Bauke".into();
        fs::write(&data_path, serde_json::to_string(&data).unwrap()).unwrap();
        save_project_manifest(&data_path, &data).unwrap();

        assert_eq!(project_display_name(&data_path), "Familie Bauke");
        assert_eq!(
            load_project_manifest(&data_path).unwrap().data_file,
            "familienbaum.minigramps.json"
        );
        let _ = fs::remove_dir_all(folder);
    }
}
