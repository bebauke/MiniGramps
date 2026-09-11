//! Daten-Schnittstellen (Stores): lokal (Dateisystem) und Server (HTTP).
//!
//! Verdrahtung (geplant):
//! - `FileSystemStore` kapselt den Zugriff auf einen Projektordner bzw. eine
//!   Datendatei; `ui::MiniGramps::save`/`load_layout` nutzen ihn bereits.
//! - `ServerStore` spricht dieselben Operationen über HTTP gegen einen
//!   Gramps-Web-artigen Endpunkt (`{base}/data/...`, `{base}/media/...`).
//!   Der Transport ist abstrahiert (`HttpTransport`), damit Tests ohne Netz
//!   mit einem Mock laufen.
//! - Modelle: `TreeData`/`ProjectManifest` aus `model`/`import`, Layout-
//!   Serialisierung hier (Version 2 = hierarchische Versätze).

#![allow(dead_code)]

use std::{collections::HashMap, fs, path::PathBuf};

use crate::import::{ProjectManifest, collect_project_files, load_project_manifest, manifest_path};
use crate::model::TreeData;

/// Abstraktion der Datenhaltung: Festplatte oder Server.
pub trait DataStore {
    /// Projektdaten lesen.
    fn read_data(&self) -> Result<TreeData, String>;
    /// Projektdaten schreiben (atomar genug für den aktuellen Zweck).
    fn write_data(&self, data: &TreeData) -> Result<(), String>;
    /// Manuelle Layout-Versätze + zuletzt aktive Referenzperson lesen.
    fn read_layout(&self) -> (HashMap<String, f32>, Option<String>);
    /// Manuelle Layout-Versätze + Referenzperson schreiben.
    fn write_layout(
        &self,
        entries: &[(String, f32)],
        reference: Option<&str>,
    ) -> Result<(), String>;
    /// Projektmanifest lesen (Name, Format, Datendatei).
    fn read_manifest(&self) -> Option<ProjectManifest>;
    /// Projektmanifest schreiben.
    fn write_manifest(&self, manifest: &ProjectManifest) -> Result<(), String>;
    /// Medien-Bytes über den relativen Pfad (`media/<hash>.<ext>`) lesen.
    fn read_media(&self, relative: &str) -> Result<Vec<u8>, String>;
    /// Medien-Bytes schreiben.
    fn write_media(&self, relative: &str, bytes: &[u8]) -> Result<(), String>;
    /// Verfügbare Projekte auflisten (Anzeigenamen-Pflicht im Dialog).
    fn list_projects(&self) -> Vec<String>;
}

// --- Layout-Serialisierung ---------------------------------------------------

/// Layout-Datei (Version 2 = hierarchische Versätze; Version 1 flach wird
/// beim Laden ignoriert, siehe `ui`-Historie).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct LayoutFile {
    pub version: u32,
    /// Zuletzt aktive Referenzperson des Projekts (optional, abwärtskompatibel).
    #[serde(default)]
    pub reference: Option<String>,
    pub entries: Vec<LayoutEntry>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct LayoutEntry {
    pub id: String,
    pub offset: f32,
}

/// Aktuelle Layout-Dateiversion.
pub const LAYOUT_VERSION: u32 = 2;

fn parse_layout(text: &str) -> (HashMap<String, f32>, Option<String>) {
    serde_json::from_str::<LayoutFile>(text)
        .map(|file| {
            if file.version < LAYOUT_VERSION {
                return (HashMap::new(), None);
            }
            let entries = file
                .entries
                .into_iter()
                .map(|entry| (entry.id, entry.offset))
                .collect();
            (entries, file.reference)
        })
        .unwrap_or_default()
}

// --- Lokaler Store (Festplatte) ----------------------------------------------

/// Store direkt auf dem Dateisystem, verankert an einer Datendatei.
#[derive(Clone)]
pub struct FileSystemStore {
    root: PathBuf,
    data_file: PathBuf,
}

impl FileSystemStore {
    /// Store für eine konkrete Datendatei (root = deren Ordner).
    pub fn for_data_file(data_file: &std::path::Path) -> Self {
        Self {
            root: data_file
                .parent()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_default(),
            data_file: data_file.to_path_buf(),
        }
    }

    /// Store für einen Ordner (z. B. die Medien-Basis); Datendatei mit
    /// Standardnamen.
    pub fn for_root(root: std::path::PathBuf) -> Self {
        Self {
            data_file: root.join("familienbaum.minigramps.json"),
            root,
        }
    }

    fn layout_path(&self) -> PathBuf {
        let stem = self
            .data_file
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("projekt");
        self.data_file.with_file_name(format!("{stem}.layout.json"))
    }
}

impl DataStore for FileSystemStore {
    fn read_data(&self) -> Result<TreeData, String> {
        let text = fs::read_to_string(&self.data_file).map_err(|error| error.to_string())?;
        serde_json::from_str(&text).map_err(|error| error.to_string())
    }

    fn write_data(&self, data: &TreeData) -> Result<(), String> {
        let text = serde_json::to_string_pretty(data).map_err(|error| error.to_string())?;
        fs::write(&self.data_file, text).map_err(|error| error.to_string())
    }

    fn read_layout(&self) -> (HashMap<String, f32>, Option<String>) {
        fs::read_to_string(self.layout_path())
            .map(|text| parse_layout(&text))
            .unwrap_or_default()
    }

    fn write_layout(
        &self,
        entries: &[(String, f32)],
        reference: Option<&str>,
    ) -> Result<(), String> {
        let file = LayoutFile {
            version: LAYOUT_VERSION,
            reference: reference.map(str::to_string),
            entries: entries
                .iter()
                .map(|(id, offset)| LayoutEntry {
                    id: id.clone(),
                    offset: *offset,
                })
                .collect(),
        };
        let text = serde_json::to_string_pretty(&file).map_err(|error| error.to_string())?;
        fs::write(self.layout_path(), text).map_err(|error| error.to_string())
    }

    fn read_manifest(&self) -> Option<ProjectManifest> {
        load_project_manifest(&self.data_file)
    }

    fn write_manifest(&self, manifest: &ProjectManifest) -> Result<(), String> {
        let path = manifest_path(&self.data_file);
        let text = serde_json::to_string_pretty(manifest).map_err(|error| error.to_string())?;
        fs::write(path, text).map_err(|error| error.to_string())
    }

    fn read_media(&self, relative: &str) -> Result<Vec<u8>, String> {
        fs::read(self.root.join(relative)).map_err(|error| error.to_string())
    }

    fn write_media(&self, relative: &str, bytes: &[u8]) -> Result<(), String> {
        let target = self.root.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::write(target, bytes).map_err(|error| error.to_string())
    }

    fn list_projects(&self) -> Vec<String> {
        let mut files = Vec::new();
        collect_project_files(&self.root, &mut files, 2);
        files
            .iter()
            .filter_map(|path| crate::import::project_display_name(path).into())
            .collect()
    }
}

// --- Server-Store (HTTP) ------------------------------------------------------

/// HTTP-Transport (austauschbar für Tests ohne Netz).
pub trait HttpTransport {
    fn get(&self, url: &str, token: Option<&str>) -> Result<Vec<u8>, String>;
    fn put(&self, url: &str, body: &[u8], token: Option<&str>) -> Result<(), String>;
}

/// Blocking-Transport auf Basis von `ureq`.
pub struct UreqTransport;

impl HttpTransport for UreqTransport {
    fn get(&self, url: &str, token: Option<&str>) -> Result<Vec<u8>, String> {
        use std::io::Read as _;
        let mut request = ureq::get(url);
        if let Some(token) = token {
            request = request.set("Authorization", &format!("Bearer {token}"));
        }
        let reader = request.call().map_err(|error| error.to_string())?;
        let mut bytes = Vec::new();
        reader
            .into_reader()
            .read_to_end(&mut bytes)
            .map_err(|error: std::io::Error| error.to_string())?;
        Ok(bytes)
    }

    fn put(&self, url: &str, body: &[u8], token: Option<&str>) -> Result<(), String> {
        let mut request = ureq::put(url);
        if let Some(token) = token {
            request = request.set("Authorization", &format!("Bearer {token}"));
        }
        request
            .send_bytes(body)
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

/// Verbindungskonfiguration für den Server-Store.
#[derive(Clone)]
pub struct ServerConfig {
    /// Basis-URL, z. B. `https://familie.example/api/minigramps/v1`.
    pub base_url: String,
    /// Zugangstoken (NICHT im Projekt gespeichert — Schlüsselspeicher).
    pub token: Option<String>,
}

/// Store auf einem Server: dieselben Operationen wie lokal, aber über HTTP.
pub struct ServerStore<T: HttpTransport> {
    transport: T,
    config: ServerConfig,
}

impl<T: HttpTransport> ServerStore<T> {
    pub fn new(transport: T, config: ServerConfig) -> Self {
        Self { transport, config }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/{}", self.config.base_url.trim_end_matches('/'), path)
    }

    fn token(&self) -> Option<&str> {
        self.config.token.as_deref()
    }
}

impl<T: HttpTransport> DataStore for ServerStore<T> {
    fn read_data(&self) -> Result<TreeData, String> {
        let bytes = self
            .transport
            .get(&self.url("data/tree.json"), self.token())?;
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())
    }

    fn write_data(&self, data: &TreeData) -> Result<(), String> {
        let bytes = serde_json::to_vec(data).map_err(|error| error.to_string())?;
        self.transport
            .put(&self.url("data/tree.json"), &bytes, self.token())
    }

    fn read_layout(&self) -> (HashMap<String, f32>, Option<String>) {
        let bytes = match self
            .transport
            .get(&self.url("layout/tree.json"), self.token())
        {
            Ok(bytes) => bytes,
            Err(_) => return (HashMap::new(), None),
        };
        std::str::from_utf8(&bytes)
            .map(parse_layout)
            .unwrap_or_default()
    }

    fn write_layout(
        &self,
        entries: &[(String, f32)],
        reference: Option<&str>,
    ) -> Result<(), String> {
        let file = LayoutFile {
            version: LAYOUT_VERSION,
            reference: reference.map(str::to_string),
            entries: entries
                .iter()
                .map(|(id, offset)| LayoutEntry {
                    id: id.clone(),
                    offset: *offset,
                })
                .collect(),
        };
        let bytes = serde_json::to_vec(&file).map_err(|error| error.to_string())?;
        self.transport
            .put(&self.url("layout/tree.json"), &bytes, self.token())
    }

    fn read_manifest(&self) -> Option<ProjectManifest> {
        let bytes = self
            .transport
            .get(&self.url("manifest.json"), self.token())
            .ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    fn write_manifest(&self, manifest: &ProjectManifest) -> Result<(), String> {
        let bytes = serde_json::to_vec(manifest).map_err(|error| error.to_string())?;
        self.transport
            .put(&self.url("manifest.json"), &bytes, self.token())
    }

    fn read_media(&self, relative: &str) -> Result<Vec<u8>, String> {
        self.transport.get(&self.url(relative), self.token())
    }

    fn write_media(&self, relative: &str, bytes: &[u8]) -> Result<(), String> {
        self.transport.put(&self.url(relative), bytes, self.token())
    }

    fn list_projects(&self) -> Vec<String> {
        let bytes = match self.transport.get(&self.url("projects"), self.token()) {
            Ok(bytes) => bytes,
            Err(_) => return Vec::new(),
        };
        serde_json::from_slice(&bytes).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::person;
    use std::collections::BTreeMap;

    #[test]
    fn fs_store_round_trip() {
        let root = std::env::temp_dir().join("minigramps-fsstore-test");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let data_file = root.join("familienbaum.minigramps.json");
        let store = FileSystemStore::for_data_file(&data_file);

        let mut data = TreeData::default();
        data.people.push(person(
            "p1",
            "Ada",
            "Lovelace",
            "1815",
            crate::model::Gender::Female,
        ));
        store.write_data(&data).unwrap();
        assert_eq!(store.read_data().unwrap().people.len(), 1);

        store.write_layout(&[("p1".into(), -42.5)], Some("p1")).unwrap();
        let (layout, reference) = store.read_layout();
        assert_eq!(layout.get("p1"), Some(&-42.5));
        assert_eq!(reference.as_deref(), Some("p1"));

        store
            .write_manifest(&ProjectManifest {
                format: "minigramps".into(),
                format_version: 1,
                name: "Test".into(),
                data_file: "familienbaum.minigramps.json".into(),
            })
            .unwrap();
        assert_eq!(store.read_manifest().unwrap().name, "Test");

        store.write_media("media/abc.png", b"pngdata").unwrap();
        assert_eq!(store.read_media("media/abc.png").unwrap(), b"pngdata");

        let projects = store.list_projects();
        assert!(projects.iter().any(|name| name == "Test"));
        let _ = fs::remove_dir_all(&root);
    }

    /// In-Memory-Transport: URL → Antwortkörper (GET/PUT).
    struct MockTransport {
        routes: std::cell::RefCell<BTreeMap<String, Vec<u8>>>,
    }

    impl MockTransport {
        fn new() -> Self {
            Self {
                routes: std::cell::RefCell::new(BTreeMap::new()),
            }
        }
    }

    impl HttpTransport for MockTransport {
        fn get(&self, url: &str, _token: Option<&str>) -> Result<Vec<u8>, String> {
            self.routes
                .borrow()
                .get(url)
                .cloned()
                .ok_or_else(|| format!("404: {url}"))
        }

        fn put(&self, url: &str, body: &[u8], _token: Option<&str>) -> Result<(), String> {
            self.routes
                .borrow_mut()
                .insert(url.to_string(), body.to_vec());
            Ok(())
        }
    }

    #[test]
    fn server_store_round_trip_without_network() {
        let transport = MockTransport::new();
        let store = ServerStore::new(
            transport,
            ServerConfig {
                base_url: "https://example.test/api/v1".into(),
                token: Some("geheim".into()),
            },
        );

        let mut data = TreeData::default();
        data.people.push(person(
            "p1",
            "Ada",
            "Lovelace",
            "1815",
            crate::model::Gender::Female,
        ));
        store.write_data(&data).unwrap();
        assert_eq!(store.read_data().unwrap().people[0].family_name, "Lovelace");

        store.write_layout(&[("p1".into(), 12.0)], None).unwrap();
        assert_eq!(store.read_layout().0.get("p1"), Some(&12.0));

        store.write_media("media/x.jpg", b"jpeg").unwrap();
        assert_eq!(store.read_media("media/x.jpg").unwrap(), b"jpeg");
    }
}
