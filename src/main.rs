#![allow(clippy::collapsible_if, clippy::too_many_arguments)]

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use directories::ProjectDirs;
use eframe::{
    egui,
    egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, TextureHandle, Vec2},
};
use rfd::FileDialog;
use roxmltree::Document;
use serde::{Deserialize, Serialize};

const ICON_OPEN: &[u8] = include_bytes!("../assets/icons/folder.svg");
const ICON_SAVE: &[u8] = include_bytes!("../assets/icons/save.svg");
const ICON_SETTINGS: &[u8] = include_bytes!("../assets/icons/settings.svg");
const ICON_ADD_PERSON: &[u8] = include_bytes!("../assets/icons/user-plus.svg");
const ICON_PARTNER: &[u8] = include_bytes!("../assets/icons/heart.svg");
const ICON_PARENT: &[u8] = include_bytes!("../assets/icons/arrow-up.svg");
const ICON_SIBLING: &[u8] = include_bytes!("../assets/icons/git-branch.svg");
const ICON_CHILD: &[u8] = include_bytes!("../assets/icons/arrow-down.svg");
const ICON_EDIT: &[u8] = include_bytes!("../assets/icons/edit-3.svg");
const LOGO: &[u8] = include_bytes!("../assets/icon.svg");

#[derive(Clone, Serialize, Deserialize)]
struct Person {
    id: String,
    name: String,
    birth: String,
    #[serde(default)]
    birth_place: String,
    death: String,
    #[serde(default)]
    death_place: String,
    gender: Gender,
    #[serde(default)]
    photo: Option<String>,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    gallery: Vec<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
enum Gender {
    Male,
    Female,
    #[default]
    Unknown,
}

#[derive(Clone, Serialize, Deserialize)]
struct Family {
    id: String,
    parent_a: Option<String>,
    parent_b: Option<String>,
    children: Vec<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
enum ChildRelation {
    #[default]
    Birth,
    Adopted,
    Step,
    Foster,
}

impl ChildRelation {
    fn label(self) -> &'static str {
        match self {
            ChildRelation::Birth => "Leiblich",
            ChildRelation::Adopted => "Adoptiert",
            ChildRelation::Step => "Stiefkind",
            ChildRelation::Foster => "Pflegekind",
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
struct TreeData {
    people: Vec<Person>,
    families: Vec<Family>,
    #[serde(default)]
    child_relations: HashMap<String, ChildRelation>,
}

impl TreeData {
    fn demo() -> Self {
        Self {
            people: vec![
                person("p1", "Helena Bergmann", "1948", Gender::Female),
                person("p2", "Martin Bergmann", "1945", Gender::Male),
                person("p3", "Clara Bergmann", "1974", Gender::Female),
                person("p4", "Jonas Adler", "1971", Gender::Male),
                person("p5", "Mila Adler", "2001", Gender::Female),
                person("p6", "Noah Adler", "2005", Gender::Male),
            ],
            families: vec![
                Family {
                    id: "f1".into(),
                    parent_a: Some("p1".into()),
                    parent_b: Some("p2".into()),
                    children: vec!["p3".into()],
                },
                Family {
                    id: "f2".into(),
                    parent_a: Some("p3".into()),
                    parent_b: Some("p4".into()),
                    children: vec!["p5".into(), "p6".into()],
                },
            ],
            child_relations: HashMap::new(),
        }
    }
    fn find(&self, id: &str) -> Option<&Person> {
        self.people.iter().find(|p| p.id == id)
    }
    fn children_of(&self, id: &str) -> Vec<&Person> {
        self.families
            .iter()
            .filter(|f| f.parent_a.as_deref() == Some(id) || f.parent_b.as_deref() == Some(id))
            .flat_map(|f| f.children.iter())
            .filter_map(|id| self.find(id))
            .collect()
    }
    fn parents_of(&self, id: &str) -> Vec<&Person> {
        self.families
            .iter()
            .filter(|f| f.children.iter().any(|c| c == id))
            .flat_map(|f| [&f.parent_a, &f.parent_b])
            .filter_map(|id| id.as_deref())
            .filter_map(|id| self.find(id))
            .collect()
    }
    fn siblings_of(&self, id: &str) -> Vec<&Person> {
        self.families
            .iter()
            .filter(|f| f.children.iter().any(|child| child == id))
            .flat_map(|f| f.children.iter())
            .filter(|sibling| sibling.as_str() != id)
            .filter_map(|sibling| self.find(sibling))
            .collect()
    }
    fn partners_of(&self, id: &str) -> Vec<&Person> {
        self.families
            .iter()
            .filter_map(|f| {
                if f.parent_a.as_deref() == Some(id) {
                    f.parent_b.as_deref()
                } else if f.parent_b.as_deref() == Some(id) {
                    f.parent_a.as_deref()
                } else {
                    None
                }
            })
            .filter_map(|partner| self.find(partner))
            .collect()
    }
    fn link_partner(&mut self, person_id: &str, partner_id: &str) {
        if person_id == partner_id
            || self
                .partners_of(person_id)
                .iter()
                .any(|person| person.id == partner_id)
        {
            return;
        }
        if let Some(family) = self.families.iter_mut().find(|family| {
            (family.parent_a.as_deref() == Some(person_id) && family.parent_b.is_none())
                || (family.parent_b.as_deref() == Some(person_id) && family.parent_a.is_none())
        }) {
            if family.parent_a.as_deref() == Some(person_id) {
                family.parent_b = Some(partner_id.into());
            } else {
                family.parent_a = Some(partner_id.into());
            }
        } else {
            self.families.push(Family {
                id: format!("f{}", self.families.len() + 1),
                parent_a: Some(person_id.into()),
                parent_b: Some(partner_id.into()),
                children: Vec::new(),
            });
        }
    }
    fn link_child(&mut self, parent_id: &str, child_id: &str) {
        if parent_id == child_id {
            return;
        }
        if let Some(family) = self.families.iter_mut().find(|family| {
            family.parent_a.as_deref() == Some(parent_id)
                || family.parent_b.as_deref() == Some(parent_id)
        }) {
            if !family.children.iter().any(|child| child == child_id) {
                family.children.push(child_id.into());
            }
        } else {
            self.families.push(Family {
                id: format!("f{}", self.families.len() + 1),
                parent_a: Some(parent_id.into()),
                parent_b: None,
                children: vec![child_id.into()],
            });
        }
    }
    fn link_child_to(
        &mut self,
        parent_a: Option<&str>,
        parent_b: Option<&str>,
        child_id: &str,
        relation: ChildRelation,
    ) {
        if Some(child_id) == parent_a || Some(child_id) == parent_b {
            return;
        }
        let existing = self
            .families
            .iter()
            .position(|family| {
                family.parent_a.as_deref() == parent_a && family.parent_b.as_deref() == parent_b
            })
            .map(|index| self.families[index].id.clone());
        let family_id = existing.unwrap_or_else(|| {
            let id = format!("f{}", self.families.len() + 1);
            self.families.push(Family {
                id: id.clone(),
                parent_a: parent_a.map(str::to_string),
                parent_b: parent_b.map(str::to_string),
                children: Vec::new(),
            });
            id
        });
        if let Some(family) = self
            .families
            .iter_mut()
            .find(|family| family.id == family_id)
        {
            if !family.children.iter().any(|child| child == child_id) {
                family.children.push(child_id.to_string());
            }
        }
        self.child_relations
            .insert(format!("{family_id}/{child_id}"), relation);
    }
    fn relation_of_child(&self, parent_id: &str, child_id: &str) -> ChildRelation {
        self.families
            .iter()
            .filter(|family| {
                (family.parent_a.as_deref() == Some(parent_id)
                    || family.parent_b.as_deref() == Some(parent_id))
                    && family.children.iter().any(|child| child == child_id)
            })
            .find_map(|family| {
                self.child_relations
                    .get(&format!("{}/{child_id}", family.id))
            })
            .copied()
            .unwrap_or(ChildRelation::Birth)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TreeView {
    Descendants,
    Ancestors,
    Fan,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TreeOrientation {
    Vertical,
    Horizontal,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RelationKind {
    Partner,
    Parent,
    Sibling,
    Child,
}

enum TreeAction {
    View(String),
    Reference(String),
    ToggleExpand(String),
}

fn person(id: &str, name: &str, birth: &str, gender: Gender) -> Person {
    Person {
        id: id.into(),
        name: name.into(),
        birth: birth.into(),
        birth_place: String::new(),
        death: String::new(),
        death_place: String::new(),
        gender,
        photo: None,
        notes: String::new(),
        source: String::new(),
        gallery: Vec::new(),
    }
}

struct MiniGramps {
    data: TreeData,
    selected: Option<String>,
    reference: Option<String>,
    expanded: HashSet<String>,
    long_press_used: bool,
    max_generations: usize,
    library: PathBuf,
    status: String,
    zoom: f32,
    pan: Vec2,
    show_editor: bool,
    show_open: bool,
    show_settings: bool,
    group_by_count: bool,
    photo_cache: HashMap<String, TextureHandle>,
    tree_view: TreeView,
    project_locations: Vec<PathBuf>,
    dark_mode: bool,
    tree_orientation: TreeOrientation,
    editing: Option<String>,
    draft: Person,
    inline_edit: bool,
    relation_query: String,
    pending_image: Option<PathBuf>,
    pending_child_for: Option<String>,
    pending_child_partner: Option<String>,
    pending_child_relation: ChildRelation,
    relation_picker: Option<RelationKind>,
    log_lines: Vec<String>,
    started: std::time::Instant,
}

impl MiniGramps {
    fn new() -> Self {
        let library = default_library();
        let _ = fs::create_dir_all(&library);
        let mut app = Self {
            data: TreeData::demo(),
            selected: Some("p5".into()),
            reference: Some("p5".into()),
            expanded: HashSet::new(),
            long_press_used: false,
            max_generations: 5,
            library,
            status: "Beispielbaum geladen".into(),
            zoom: 1.0,
            pan: Vec2::ZERO,
            show_editor: false,
            show_open: false,
            show_settings: false,
            group_by_count: true,
            photo_cache: HashMap::new(),
            tree_view: TreeView::Descendants,
            project_locations: Vec::new(),
            dark_mode: true,
            tree_orientation: TreeOrientation::Vertical,
            editing: None,
            draft: person("", "", "", Gender::Unknown),
            inline_edit: false,
            relation_query: String::new(),
            pending_image: None,
            pending_child_for: None,
            pending_child_partner: None,
            pending_child_relation: ChildRelation::Birth,
            relation_picker: None,
            log_lines: Vec::new(),
            started: std::time::Instant::now(),
        };
        app.log(format!(
            "Start. Datenordner (Speicherort): {}",
            app.library.display()
        ));
        app
    }

    fn log(&mut self, message: impl Into<String>) {
        let line = format!(
            "[{:>9.3}s] {}",
            self.started.elapsed().as_secs_f32(),
            message.into()
        );
        println!("{line}");
        self.log_lines.push(line);
    }

    fn save(&mut self) {
        let path = self.library.join("familienbaum.minigramps.json");
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
    }

    fn import_dialog(&mut self) {
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

    fn load_path(&mut self, path: &Path) {
        self.log(format!("Lade: {}", path.display()));
        match load_file(path) {
            Ok(data) => {
                self.selected = data.people.first().map(|p| p.id.clone());
                self.log(format!(
                    "Geladen: {} Personen, {} Familien aus {}",
                    data.people.len(),
                    data.families.len(),
                    path.display()
                ));
                self.data = data;
                self.photo_cache.clear();
                self.show_open = false;
                self.status = format!("Geöffnet: {}", path.display());
            }
            Err(e) => {
                self.status = format!("Laden fehlgeschlagen: {e}");
                self.log(format!("Laden fehlgeschlagen {}: {e}", path.display()));
            }
        }
    }

    fn change_library(&mut self) {
        if let Some(path) = FileDialog::new().set_directory(&self.library).pick_folder() {
            self.library = path;
            let display = self.library.display().to_string();
            self.status = format!("Datenordner: {display}");
            self.log(format!("Datenordner (Speicherort) geändert: {display}"));
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
        }
        ctx.set_style(style);
        let header_color = if self.dark_mode {
            Color32::from_rgb(24, 56, 68)
        } else {
            Color32::from_rgb(211, 235, 229)
        };
        let panel_color = if self.dark_mode {
            Color32::from_rgb(23, 33, 43)
        } else {
            Color32::from_rgb(242, 247, 246)
        };
        let canvas_color = if self.dark_mode {
            Color32::from_rgb(15, 22, 30)
        } else {
            Color32::from_rgb(250, 252, 251)
        };
        let brand_accent = if self.dark_mode {
            Color32::from_rgb(161, 224, 204)
        } else {
            Color32::from_rgb(0, 86, 58)
        };
        let accent_dim = if self.dark_mode {
            Color32::from_rgb(139, 171, 177)
        } else {
            Color32::from_rgb(43, 107, 88)
        };
        let section_accent = if self.dark_mode {
            Color32::from_rgb(135, 191, 183)
        } else {
            Color32::from_rgb(0, 105, 70)
        };

        egui::TopBottomPanel::top("header")
            .frame(egui::Frame::new().fill(header_color).inner_margin(10))
            .show(ctx, |ui| {
                ui.columns(3, |columns| {
                    columns[0].with_layout(
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            if icon_button_big(ui, ICON_OPEN, "open", "Projekt öffnen").clicked() {
                                let found =
                                    discover_projects(&self.library, &self.project_locations).len();
                                self.log(format!(
                                    "Projektsuche: {found} Treffer in {} Ordnern",
                                    2 + self.project_locations.len()
                                ));
                                self.show_open = true;
                            }
                            if icon_button_big(ui, ICON_SAVE, "save", "Projekt speichern").clicked()
                            {
                                self.save();
                            }
                        },
                    );
                    columns[1].vertical_centered(|ui| {
                        ui.heading(
                            egui::RichText::new("mini gramps")
                                .color(brand_accent)
                                .strong(),
                        );
                        ui.label(
                            egui::RichText::new("FAMILIENARCHIV")
                                .small()
                                .color(accent_dim),
                        );
                    });
                    columns[2].with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if icon_button_big(ui, ICON_SETTINGS, "settings", "Einstellungen")
                                .clicked()
                            {
                                self.show_settings = !self.show_settings;
                            }
                            ui.label(
                                egui::RichText::new(&self.status)
                                    .small()
                                    .color(Color32::LIGHT_GRAY),
                            );
                        },
                    );
                });
            });

        egui::SidePanel::left("people")
            .resizable(true)
            .default_width(230.0)
            .frame(egui::Frame::new().fill(panel_color).inner_margin(10))
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("PERSONEN")
                            .small()
                            .strong()
                            .color(section_accent),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_only_button(ui, ICON_SETTINGS, "group-sort")
                            .on_hover_text(if self.group_by_count {
                                "Sortierung: Anzahl (Klick für Alphabet)"
                            } else {
                                "Sortierung: Alphabet (Klick für Anzahl)"
                            })
                            .clicked()
                        {
                            self.group_by_count = !self.group_by_count;
                        }
                    });
                });
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let mut groups: Vec<(String, Vec<&Person>)> = Vec::new();
                        for p in &self.data.people {
                            let surname = p
                                .name
                                .split_whitespace()
                                .last()
                                .unwrap_or(p.name.as_str())
                                .to_lowercase();
                            match groups.iter_mut().find(|(name, _)| *name == surname) {
                                Some((_, members)) => members.push(p),
                                None => groups.push((surname, vec![p])),
                            }
                        }
                        if self.group_by_count {
                            groups.sort_by(|(a, members_a), (b, members_b)| {
                                members_b.len().cmp(&members_a.len()).then_with(|| a.cmp(b))
                            });
                        } else {
                            groups.sort_by(|(a, _), (b, _)| a.cmp(b));
                        }
                        for (surname, members) in &mut groups {
                            members.sort_by_key(|p| p.name.to_lowercase());
                            let mut chars = surname.chars();
                            let display = match chars.next() {
                                Some(first) => {
                                    first.to_uppercase().collect::<String>() + chars.as_str()
                                }
                                None => String::new(),
                            };
                            ui.collapsing(format!("{display} · {}", members.len()), |ui| {
                                for p in members.iter() {
                                    let active = self.selected.as_deref() == Some(&p.id);
                                    if ui
                                        .selectable_label(
                                            active,
                                            format!("{}  {}", gender_symbol(p.gender), p.name),
                                        )
                                        .clicked()
                                    {
                                        self.selected = Some(p.id.clone());
                                    }
                                }
                            });
                        }
                    });
                ui.separator();
                if icon_button(ui, ICON_ADD_PERSON, "add-person", "Person hinzufügen").clicked() {
                    self.editing = None;
                    self.draft = person(
                        &format!("p{}", self.data.people.len() + 1),
                        "Neue Person",
                        "",
                        Gender::Unknown,
                    );
                    self.show_editor = true;
                }
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(format!(
                        "{} Personen · {} Familien",
                        self.data.people.len(),
                        self.data.families.len()
                    ))
                    .small()
                    .color(Color32::GRAY),
                );
            });

        egui::SidePanel::right("details")
            .default_width(250.0)
            .frame(egui::Frame::new().fill(panel_color).inner_margin(10))
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("PERSON")
                            .small()
                            .strong()
                            .color(section_accent),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let icon = if self.inline_edit {
                            ICON_SAVE
                        } else {
                            ICON_EDIT
                        };
                        if icon_only_button(ui, icon, "profile-edit").clicked() {
                            if self.inline_edit {
                                if let Some(person) = self
                                    .data
                                    .people
                                    .iter_mut()
                                    .find(|person| person.id == self.draft.id)
                                {
                                    *person = self.draft.clone();
                                }
                                self.photo_cache.remove(&self.draft.id);
                                self.status = "Profil gespeichert".into();
                                self.inline_edit = false;
                                self.relation_picker = None;
                                self.relation_query.clear();
                            } else if let Some(id) = &self.selected {
                                if let Some(person) = self.data.find(id).cloned() {
                                    self.draft = person;
                                    self.inline_edit = true;
                                }
                            }
                        }
                    });
                });
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if let Some(id) = &self.selected.clone() {
                            if self.reference.as_deref() != Some(id.as_str()) {
                                if ui.small_button("Als Referenz setzen").clicked() {
                                    self.reference = self.selected.clone();
                                    let name = self
                                        .data
                                        .find(id)
                                        .map(|p| p.name.clone())
                                        .unwrap_or_default();
                                    self.status = format!("Referenzperson: {name}");
                                    self.log(format!("Referenzperson gesetzt: {name} ({id})"));
                                }
                            }
                            if let Some(p) = self.data.find(id).cloned() {
                                let parents: Vec<_> =
                                    self.data.parents_of(id).into_iter().cloned().collect();
                                let siblings: Vec<_> =
                                    self.data.siblings_of(id).into_iter().cloned().collect();
                                let children: Vec<_> =
                                    self.data.children_of(id).into_iter().cloned().collect();
                                let partners: Vec<_> =
                                    self.data.partners_of(id).into_iter().cloned().collect();
                                ui.add_space(18.0);
                                if self.inline_edit {
                                    if avatar_ui(ui, &self.draft, &mut self.photo_cache, &self.library, 64.0)
                                        .clicked()
                                    {
                                        if let Some(path) = FileDialog::new()
                                            .add_filter("Bilder", &["png", "jpg", "jpeg", "webp"])
                                            .pick_file()
                                        {
                                            self.draft.photo =
                                                Some(path.to_string_lossy().into_owned());
                                        }
                                    }
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.draft.name)
                                            .hint_text("Vor- und Nachname"),
                                    );
                                    ui.horizontal(|ui| {
                                        ui.radio_value(&mut self.draft.gender, Gender::Female, "W");
                                        ui.radio_value(&mut self.draft.gender, Gender::Male, "M");
                                        ui.radio_value(
                                            &mut self.draft.gender,
                                            Gender::Unknown,
                                            "?",
                                        );
                                    });
                                } else {
                                    avatar_ui(ui, &p, &mut self.photo_cache, &self.library, 64.0);
                                    ui.heading(&p.name);
                                    ui.label(gender_label(p.gender));
                                }
                                ui.add_space(12.0);
                                if self.inline_edit {
                                    editable_info_row(
                                        ui,
                                        "GEBOREN",
                                        &mut self.draft.birth,
                                        &mut self.draft.birth_place,
                                    );
                                    editable_info_row(
                                        ui,
                                        "GESTORBEN",
                                        &mut self.draft.death,
                                        &mut self.draft.death_place,
                                    );
                                    ui.label("QUELLE");
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.draft.source)
                                            .hint_text("Quelle"),
                                    );
                                    ui.label("NOTIZEN");
                                    ui.add(
                                        egui::TextEdit::multiline(&mut self.draft.notes)
                                            .hint_text("Notizen")
                                            .desired_rows(2),
                                    );
                                } else {
                                    info_row(ui, "GEBOREN", &dated_place(&p.birth, &p.birth_place));
                                    info_row(
                                        ui,
                                        "GESTORBEN",
                                        &dated_place(&p.death, &p.death_place),
                                    );
                                }
                                if !p.source.is_empty() {
                                    info_row(ui, "QUELLE", &p.source);
                                }
                                if !p.notes.is_empty() {
                                    ui.label(
                                        egui::RichText::new("NOTIZEN").small().color(Color32::GRAY),
                                    );
                                    ui.label(&p.notes);
                                }
                                ui.separator();
                                ui.label(
                                    egui::RichText::new("FAMILIE")
                                        .small()
                                        .strong()
                                        .color(section_accent),
                                );
                                ui.add_space(5.0);
                                relation_header(
                                    ui,
                                    "PARTNER",
                                    RelationKind::Partner,
                                    &mut self.relation_picker,
                                    self.inline_edit,
                                );
                                if partners.is_empty() {
                                    ui.label(
                                        egui::RichText::new("Nicht hinterlegt")
                                            .italics()
                                            .color(Color32::GRAY),
                                    );
                                }
                                for partner in partners {
                                    if relationship_row(
                                        ui,
                                        ICON_PARTNER,
                                        "partner",
                                        &partner,
                                        &mut self.photo_cache,
                                    ) {
                                        self.selected = Some(partner.id);
                                    }
                                }
                                if self.relation_picker == Some(RelationKind::Partner) {
                                    self.relation_suggestions(ui, RelationKind::Partner, &p.id);
                                }
                                ui.add_space(7.0);
                                relation_header(
                                    ui,
                                    "ELTERN",
                                    RelationKind::Parent,
                                    &mut self.relation_picker,
                                    self.inline_edit,
                                );
                                if parents.is_empty() {
                                    ui.label(
                                        egui::RichText::new("Nicht hinterlegt")
                                            .italics()
                                            .color(Color32::GRAY),
                                    );
                                }
                                for parent in parents {
                                    if relationship_row(
                                        ui,
                                        ICON_PARENT,
                                        "parent",
                                        &parent,
                                        &mut self.photo_cache,
                                    ) {
                                        self.selected = Some(parent.id);
                                    }
                                }
                                if self.relation_picker == Some(RelationKind::Parent) {
                                    self.relation_suggestions(ui, RelationKind::Parent, &p.id);
                                }
                                ui.add_space(7.0);
                                relation_header(
                                    ui,
                                    "GESCHWISTER",
                                    RelationKind::Sibling,
                                    &mut self.relation_picker,
                                    self.inline_edit,
                                );
                                if siblings.is_empty() {
                                    ui.label(
                                        egui::RichText::new("Nicht hinterlegt")
                                            .italics()
                                            .color(Color32::GRAY),
                                    );
                                }
                                for sibling in siblings {
                                    if relationship_row(
                                        ui,
                                        ICON_SIBLING,
                                        "sibling",
                                        &sibling,
                                        &mut self.photo_cache,
                                    ) {
                                        self.selected = Some(sibling.id);
                                    }
                                }
                                if self.relation_picker == Some(RelationKind::Sibling) {
                                    self.relation_suggestions(ui, RelationKind::Sibling, &p.id);
                                }
                                ui.add_space(7.0);
                                relation_header(
                                    ui,
                                    "KINDER",
                                    RelationKind::Child,
                                    &mut self.relation_picker,
                                    self.inline_edit,
                                );
                                if children.is_empty() {
                                    ui.label(
                                        egui::RichText::new("Nicht hinterlegt")
                                            .italics()
                                            .color(Color32::GRAY),
                                    );
                                }
                                for child in children {
                                    let relation = self.data.relation_of_child(&p.id, &child.id);
                                    let mut display = child.clone();
                                    if relation != ChildRelation::Birth {
                                        display.name =
                                            format!("{} · {}", display.name, relation.label());
                                    }
                                    if relationship_row(
                                        ui,
                                        ICON_CHILD,
                                        "child",
                                        &display,
                                        &mut self.photo_cache,
                                    ) {
                                        self.selected = Some(child.id);
                                    }
                                }
                                if self.relation_picker == Some(RelationKind::Child) {
                                    if self.pending_child_for.as_deref() != Some(&p.id) {
                                        self.pending_child_for = Some(p.id.clone());
                                        self.pending_child_partner = self
                                            .data
                                            .partners_of(&p.id)
                                            .first()
                                            .map(|partner| partner.id.clone());
                                        self.pending_child_relation = ChildRelation::Birth;
                                    }
                                    let partners: Vec<_> =
                                        self.data.partners_of(&p.id).into_iter().cloned().collect();
                                    if let Some(id) = &self.pending_child_partner {
                                        if !partners.iter().any(|partner| &partner.id == id) {
                                            self.pending_child_partner = None;
                                        }
                                    }
                                    self.relation_suggestions(ui, RelationKind::Child, &p.id);
                                }
                                ui.separator();
                                ui.label(
                                    egui::RichText::new("GALERIE")
                                        .small()
                                        .strong()
                                        .color(section_accent),
                                );
                                let (drop_response, _) = ui.allocate_painter(
                                    Vec2::new(ui.available_width(), 70.0),
                                    Sense::hover(),
                                );
                                ui.painter().rect_stroke(
                                    drop_response.rect,
                                    6.0,
                                    Stroke::new(1.0, Color32::DARK_GRAY),
                                    egui::StrokeKind::Inside,
                                );
                                ui.painter().text(
                                    drop_response.rect.center(),
                                    Align2::CENTER_CENTER,
                                    "Foto hier ablegen",
                                    FontId::proportional(12.0),
                                    Color32::GRAY,
                                );
                                for file in ui.ctx().input(|input| input.raw.dropped_files.clone())
                                {
                                    if drop_response.hovered() {
                                        if let Some(path) = file.path {
                                            self.pending_image = Some(path);
                                        }
                                    }
                                }
                                ui.horizontal_wrapped(|ui| {
                                    for (index, path) in p.gallery.iter().enumerate() {
                                        let mut gallery_photo = p.clone();
                                        gallery_photo.id = format!("gallery-{}-{index}", p.id);
                                        gallery_photo.photo = Some(path.clone());
                                        avatar_ui(ui, &gallery_photo, &mut self.photo_cache, &self.library, 42.0);
                                    }
                                });
                            }
                        }
                    });
            });

        egui::TopBottomPanel::bottom("debug")
            .frame(egui::Frame::new().fill(panel_color).inner_margin(10))
            .show(ctx, |ui| {
                ui.collapsing("Debug-Log", |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Kopieren").clicked() {
                            ui.ctx().copy_text(self.log_lines.join("\n"));
                            self.status = "Log in Zwischenablage kopiert".into();
                        }
                        if ui.button("Leeren").clicked() {
                            self.log_lines.clear();
                        }
                        ui.label(
                            egui::RichText::new(format!("{} Einträge", self.log_lines.len()))
                                .small()
                                .color(Color32::GRAY),
                        );
                    });
                    egui::ScrollArea::vertical()
                        .max_height(160.0)
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for line in &self.log_lines {
                                ui.monospace(line);
                            }
                        });
                });
            });

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
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(8.0);
                        let hint =
                            "Mausrad: Zoom · Ziehen: Verschieben · Klick: Referenzperson wählen";
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
                let (response, painter) = ui.allocate_painter(available, Sense::drag());
                if response.dragged() {
                    self.pan += response.drag_delta();
                }
                let scroll = ui.input(|i| i.raw_scroll_delta.y);
                if response.hovered() && scroll != 0.0 {
                    self.zoom = (self.zoom * (1.0 + scroll * 0.001)).clamp(0.45, 1.8);
                }
                if !ui.input(|i| i.pointer.primary_down()) {
                    self.long_press_used = false;
                }
                let mut action: Option<TreeAction> = None;
                let viewed = self.selected.clone();
                let reference = self.reference.clone();
                let expanded = self.expanded.clone();
                let mut long_press_used = self.long_press_used;
                draw_tree(
                    &painter,
                    response.rect,
                    &self.data,
                    reference.as_deref(),
                    viewed.as_deref(),
                    &mut action,
                    &expanded,
                    &mut long_press_used,
                    self.max_generations,
                    &mut self.photo_cache,
                    self.tree_view,
                    self.tree_orientation,
                    self.zoom,
                    self.pan,
                );
                self.long_press_used = long_press_used;
                match action {
                    Some(TreeAction::View(id)) => self.selected = Some(id),
                    Some(TreeAction::Reference(id)) => {
                        let name = self
                            .data
                            .find(&id)
                            .map(|p| p.name.clone())
                            .unwrap_or_default();
                        self.reference = Some(id.clone());
                        self.selected = Some(id);
                        self.status = format!("Referenzperson: {name}");
                        self.log(format!("Referenzperson gesetzt: {name}"));
                    }
                    Some(TreeAction::ToggleExpand(id)) => {
                        if !self.expanded.remove(&id) {
                            self.expanded.insert(id);
                        }
                    }
                    None => {}
                }
            });

        if self.show_editor {
            self.person_editor(ctx);
        }
        if self.show_open {
            self.open_window(ctx);
        }
        self.settings_window(ctx);
        if self.pending_image.is_some() {
            self.image_intent_window(ctx);
        }
    }
}

impl MiniGramps {
    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = true;
        egui::Window::new(window_title("Einstellungen"))
            .open(&mut open)
            .movable(false)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::RIGHT_TOP, [-12.0, 54.0])
            .default_width(340.0)
            .show(ctx, |ui| {
                let section_accent = if self.dark_mode {
                    Color32::from_rgb(135, 191, 183)
                } else {
                    Color32::from_rgb(0, 105, 70)
                };
                ui.label(
                    egui::RichText::new("DARSTELLUNG")
                        .small()
                        .strong()
                        .color(section_accent),
                );
                ui.horizontal(|ui| {
                    ui.label("Design");
                    ui.selectable_value(&mut self.dark_mode, true, "Dunkel");
                    ui.selectable_value(&mut self.dark_mode, false, "Hell");
                });
                ui.horizontal(|ui| {
                    ui.label("Generationen");
                    for limit in [3, 5, 7] {
                        ui.selectable_value(&mut self.max_generations, limit, format!("{limit}"));
                    }
                    ui.selectable_value(&mut self.max_generations, 0, "Alle");
                });
                ui.separator();
                ui.label(
                    egui::RichText::new("SPEICHERORT")
                        .small()
                        .strong()
                        .color(section_accent),
                );
                ui.monospace(self.library.display().to_string());
                if ui.button("Datenordner ändern...").clicked() {
                    self.change_library();
                }
            });
        self.show_settings = open;
    }

    fn image_intent_window(&mut self, ctx: &egui::Context) {
        let Some(path) = self.pending_image.clone() else {
            return;
        };
        egui::Window::new(window_title("Foto hinzufügen"))
            .movable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label(path.display().to_string());
                ui.label("Wofür soll dieses Foto verwendet werden?");
                if ui.button("Als Profilbild verwenden").clicked() {
                    if let Some(id) = &self.selected {
                        if let Some(person) =
                            self.data.people.iter_mut().find(|person| person.id == *id)
                        {
                            person.photo = Some(path.to_string_lossy().into_owned());
                            self.photo_cache.remove(id);
                        }
                    }
                    self.pending_image = None;
                }
                if ui.button("Zur Galerie hinzufügen").clicked() {
                    if let Some(id) = &self.selected {
                        if let Some(person) =
                            self.data.people.iter_mut().find(|person| person.id == *id)
                        {
                            person.gallery.push(path.to_string_lossy().into_owned());
                        }
                    }
                    self.pending_image = None;
                }
                if ui.button("Abbrechen").clicked() {
                    self.pending_image = None;
                }
            });
    }

    fn relation_suggestions(&mut self, ui: &mut egui::Ui, kind: RelationKind, selected_id: &str) {
        if kind == RelationKind::Child {
            let partners: Vec<_> = self
                .data
                .partners_of(selected_id)
                .into_iter()
                .cloned()
                .collect();
            ui.horizontal(|ui| {
                ui.label("Elternteil");
                let selected_text = self
                    .pending_child_partner
                    .as_deref()
                    .and_then(|id| partners.iter().find(|person| person.id == id))
                    .map(|person| person.name.clone())
                    .unwrap_or_else(|| "Ohne Partner".into());
                egui::ComboBox::from_id_salt("child-partner-inline")
                    .selected_text(selected_text)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.pending_child_partner, None, "Ohne Partner");
                        for partner in &partners {
                            ui.selectable_value(
                                &mut self.pending_child_partner,
                                Some(partner.id.clone()),
                                &partner.name,
                            );
                        }
                    });
                ui.separator();
                ui.label("Beziehung");
                for (relation, label) in [
                    (ChildRelation::Birth, "Leiblich"),
                    (ChildRelation::Adopted, "Adoptiert"),
                    (ChildRelation::Step, "Stiefkind"),
                    (ChildRelation::Foster, "Pflegekind"),
                ] {
                    ui.radio_value(&mut self.pending_child_relation, relation, label);
                }
            });
        }
        ui.text_edit_singleline(&mut self.relation_query);
        let needle = self.relation_query.to_lowercase();
        let suggestions: Vec<_> = self
            .data
            .people
            .iter()
            .filter(|person| {
                person.id != selected_id && person.name.to_lowercase().contains(&needle)
            })
            .take(5)
            .cloned()
            .collect();
        for candidate in suggestions {
            if ui.small_button(&candidate.name).clicked() {
                match kind {
                    RelationKind::Partner => self.data.link_partner(selected_id, &candidate.id),
                    RelationKind::Parent => self.data.link_child(&candidate.id, selected_id),
                    RelationKind::Child => {
                        self.data.link_child_to(
                            Some(selected_id),
                            self.pending_child_partner.as_deref(),
                            &candidate.id,
                            self.pending_child_relation,
                        );
                    }
                    RelationKind::Sibling => {
                        let parent_id = self
                            .data
                            .parents_of(selected_id)
                            .first()
                            .map(|parent| parent.id.clone());
                        if let Some(parent_id) = parent_id {
                            self.data.link_child(&parent_id, &candidate.id);
                        }
                    }
                }
                self.relation_picker = None;
                self.relation_query.clear();
            }
        }
    }
    fn open_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_open;
        egui::Window::new(window_title("Projekt öffnen"))
            .open(&mut open)
            .movable(false)
            .default_width(560.0)
            .show(ctx, |ui| {
                ui.label("Gefundene Projekte in MiniGramps- und Gramps-Ordnern");
                ui.separator();
                for path in discover_projects(&self.library, &self.project_locations) {
                    let size = fs::metadata(&path)
                        .map(|meta| meta.len() / 1024)
                        .unwrap_or(0);
                    let label = format!(
                        "{}  ·  {} KB\n{}",
                        path.file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("Unbenannt"),
                        size,
                        path.display()
                    );
                    if ui.button(label).clicked() {
                        self.load_path(&path);
                    }
                }
                ui.separator();
                if ui.button("Dateipfad hinzufügen").clicked() {
                    if let Some(path) = FileDialog::new().pick_folder() {
                        if !self.project_locations.contains(&path) {
                            let mut found = Vec::new();
                            collect_project_files(&path, &mut found, 3);
                            self.project_locations.push(path.clone());
                            self.log(format!(
                                "Suchordner hinzugefügt: {} ({} Projektdateien)",
                                path.display(),
                                found.len()
                            ));
                        } else {
                            self.log(format!("Suchordner bereits vorhanden: {}", path.display()));
                        }
                    }
                }
                if ui.button("Datei manuell laden...").clicked() {
                    self.import_dialog();
                }
            });
        self.show_open = self.show_open && open;
    }
    fn person_editor(&mut self, ctx: &egui::Context) {
        let mut open = self.show_editor;
        let title = if self.editing.is_some() {
            "Profil bearbeiten"
        } else {
            "Neue Person"
        };
        egui::Window::new(window_title(title))
            .open(&mut open)
            .movable(false)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.label("Name");
                ui.add(
                    egui::TextEdit::singleline(&mut self.draft.name).hint_text("Vor- und Nachname"),
                );
                ui.horizontal(|ui| {
                    ui.label("Geboren");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.draft.birth)
                            .hint_text("Jahr")
                            .desired_width(80.0),
                    );
                    ui.label("Ort");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.draft.birth_place).hint_text("Ort"),
                    );
                    ui.label("Gestorben");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.draft.death)
                            .hint_text("Jahr")
                            .desired_width(80.0),
                    );
                    ui.label("Ort");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.draft.death_place).hint_text("Ort"),
                    );
                });
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.draft.gender, Gender::Female, "Weiblich");
                    ui.radio_value(&mut self.draft.gender, Gender::Male, "Männlich");
                    ui.radio_value(&mut self.draft.gender, Gender::Unknown, "Unbekannt");
                });
                ui.horizontal(|ui| {
                    if ui.button("Foto auswählen").clicked() {
                        if let Some(path) = FileDialog::new()
                            .add_filter("Bilder", &["png", "jpg", "jpeg", "webp"])
                            .pick_file()
                        {
                            self.draft.photo = Some(path.to_string_lossy().into_owned());
                        }
                    }
                    if self.draft.photo.is_some() && ui.button("Foto entfernen").clicked() {
                        self.draft.photo = None;
                    }
                });
                ui.label("Notizen");
                ui.add(
                    egui::TextEdit::multiline(&mut self.draft.notes)
                        .hint_text("Notizen")
                        .desired_rows(3),
                );
                ui.label("Quelle");
                ui.add(egui::TextEdit::singleline(&mut self.draft.source).hint_text("Quelle"));
                ui.separator();
                if ui.button("Speichern").clicked() {
                    let id = self.draft.id.clone();
                    if let Some(existing) = &self.editing {
                        if let Some(person) = self
                            .data
                            .people
                            .iter_mut()
                            .find(|person| person.id == *existing)
                        {
                            *person = self.draft.clone();
                        }
                    } else {
                        self.data.people.push(self.draft.clone());
                    }
                    self.photo_cache.remove(&id);
                    self.selected = Some(id);
                    self.status = "Profil gespeichert".into();
                    self.show_editor = false;
                }
                if let Some(id) = self.editing.clone() {
                    if ui
                        .button(egui::RichText::new("Person löschen").color(Color32::LIGHT_RED))
                        .clicked()
                    {
                        self.data.people.retain(|person| person.id != id);
                        self.data.families.iter_mut().for_each(|family| {
                            if family.parent_a.as_deref() == Some(&id) {
                                family.parent_a = None;
                            }
                            if family.parent_b.as_deref() == Some(&id) {
                                family.parent_b = None;
                            }
                            family.children.retain(|child| child != &id);
                        });
                        self.selected = self.data.people.first().map(|person| person.id.clone());
                        self.show_editor = false;
                    }
                }
            });
        self.show_editor = open;
    }
}

fn info_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(egui::RichText::new(label).small().color(Color32::GRAY));
    ui.label(if value.is_empty() { "-" } else { value });
    ui.add_space(7.0);
}

fn editable_info_row(ui: &mut egui::Ui, label: &str, date: &mut String, place: &mut String) {
    ui.label(egui::RichText::new(label).small().color(Color32::GRAY));
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(date)
                .hint_text("Jahr")
                .desired_width(80.0),
        );
        ui.add(
            egui::TextEdit::singleline(place)
                .hint_text("Ort")
                .desired_width(ui.available_width()),
        );
    });
    ui.add_space(5.0);
}

fn dated_place(date: &str, place: &str) -> String {
    match (date.is_empty(), place.is_empty()) {
        (true, true) => "-".into(),
        (false, true) => date.into(),
        (true, false) => place.into(),
        (false, false) => format!("{date} · {place}"),
    }
}

fn relation_header(
    ui: &mut egui::Ui,
    label: &str,
    kind: RelationKind,
    active: &mut Option<RelationKind>,
    editable: bool,
) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).small().color(Color32::GRAY));
        if editable && ui.small_button("+").clicked() {
            *active = if *active == Some(kind) {
                None
            } else {
                Some(kind)
            };
        }
    });
}
fn gender_symbol(g: Gender) -> &'static str {
    match g {
        Gender::Male => "♂",
        Gender::Female => "♀",
        Gender::Unknown => "○",
    }
}
fn gender_label(g: Gender) -> &'static str {
    match g {
        Gender::Male => "Männlich",
        Gender::Female => "Weiblich",
        Gender::Unknown => "Nicht angegeben",
    }
}

fn photo_texture<'a>(
    ctx: &egui::Context,
    person: &Person,
    cache: &'a mut HashMap<String, TextureHandle>,
    media_base: &Path,
) -> Option<&'a TextureHandle> {
    if !cache.contains_key(&person.id) {
        let photo = person.photo.as_deref()?;
        let raw = Path::new(photo);
        let path = if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            media_base.join(raw)
        };
        let image = image::open(path).ok()?.to_rgba8();
        let size = [image.width() as usize, image.height() as usize];
        let pixels = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
        cache.insert(
            person.id.clone(),
            ctx.load_texture(format!("photo-{}", person.id), pixels, Default::default()),
        );
    }
    cache.get(&person.id)
}

fn import_media_file(library: &Path, source: &Path) -> Option<String> {
    use std::hash::{Hash, Hasher};
    let bytes = fs::read(source).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    let digest = hasher.finish();
    let extension = source
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .unwrap_or_else(|| "jpg".into());
    let relative = format!("media/{digest:016x}.{extension}");
    let target = library.join(&relative);
    if !target.exists() {
        fs::create_dir_all(target.parent()?).ok()?;
        fs::write(&target, &bytes).ok()?;
    }
    Some(relative)
}

fn initials(person: &Person) -> String {
    person
        .name
        .split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(2)
        .collect::<String>()
        .to_uppercase()
}

fn avatar_ui(
    ui: &mut egui::Ui,
    person: &Person,
    cache: &mut HashMap<String, TextureHandle>,
    media_base: &Path,
    size: f32,
) -> egui::Response {
    if let Some(texture) = photo_texture(ui.ctx(), person, cache, media_base) {
        ui.image((texture.id(), Vec2::splat(size)))
    } else {
        let (response, painter) = ui.allocate_painter(Vec2::splat(size), Sense::click());
        painter.circle_filled(
            response.rect.center(),
            size / 2.0,
            Color32::from_rgb(55, 91, 101),
        );
        painter.text(
            response.rect.center(),
            Align2::CENTER_CENTER,
            initials(person),
            FontId::proportional(size * 0.32),
            Color32::WHITE,
        );
        response
    }
}

fn relationship_row(
    ui: &mut egui::Ui,
    icon_bytes: &'static [u8],
    icon_id: &'static str,
    person: &Person,
    cache: &mut HashMap<String, TextureHandle>,
    media_base: &Path,
) -> bool {
    let mut selected = false;
    ui.horizontal(|ui| {
        avatar_ui(ui, person, cache, media_base, 27.0);
        icon(ui, icon_bytes, icon_id);
        selected = ui.selectable_label(false, &person.name).clicked();
    });
    selected
}

fn draw_tree(
    painter: &egui::Painter,
    rect: Rect,
    data: &TreeData,
    reference: Option<&str>,
    viewed: Option<&str>,
    action: &mut Option<TreeAction>,
    expanded: &HashSet<String>,
    long_press_used: &mut bool,
    generation_limit: usize,
    photo_cache: &mut HashMap<String, TextureHandle>,
    view: TreeView,
    orientation: TreeOrientation,
    zoom: f32,
    pan: Vec2,
) {
    let Some(root) = reference
        .filter(|id| data.find(id).is_some())
        .map(str::to_string)
        .or_else(|| data.people.first().map(|p| p.id.clone()))
    else {
        return;
    };
    let ancestors = view != TreeView::Descendants;
    let mut levels = HashMap::from([(root.to_string(), 0usize)]);
    let mut frontier = vec![root.to_string()];
    while let Some(id) = frontier.pop() {
        let level = levels[&id];
        if generation_limit > 0 && level + 1 >= generation_limit && !expanded.contains(&id) {
            continue;
        }
        let relatives = if ancestors {
            data.parents_of(&id)
        } else {
            data.children_of(&id)
        };
        for relative in relatives {
            if !levels.contains_key(&relative.id) {
                levels.insert(relative.id.clone(), level + 1);
                frontier.push(relative.id.clone());
            }
        }
    }
    // Expand level by level so that relatives appear next to the person they
    // belong to (parents right behind their child, children under their parent).
    let max_level = levels.values().copied().max().unwrap_or(0);
    let mut rows: Vec<Vec<&str>> = vec![Vec::new(); max_level + 1];
    let mut placed: HashSet<&str> = HashSet::new();
    if let Some(root_level) = levels.get(&root) {
        rows[*root_level].push(root.as_str());
        placed.insert(root.as_str());
    }
    for level in 1..=max_level {
        let previous: Vec<&str> = rows[level - 1].clone();
        for prev_id in &previous {
            let relatives = if ancestors {
                data.parents_of(prev_id)
            } else {
                data.children_of(prev_id)
            };
            for relative in relatives {
                if levels[&relative.id] == level && placed.insert(relative.id.as_str()) {
                    rows[level].push(relative.id.as_str());
                }
            }
        }
    }
    for (id, level) in &levels {
        if placed.insert(id.as_str()) {
            rows[*level].push(id.as_str());
        }
    }
    let visible = |id: &str| levels.contains_key(id);
    let card_h = 78.0f32;
    let couple_gap = 8.0f32;
    let gap = 24.0f32;
    let gen_gap = 36.0f32;
    let row_widths: Vec<f32> = rows
        .iter()
        .map(|ids| {
            let mut width = 215.0f32;
            for id in ids {
                let person = data.find(id).unwrap();
                width = width.max(card_width_for(person, painter));
                for partner in data.partners_of(id) {
                    if !visible(&partner.id) {
                        width = width.max(card_width_for(partner, painter));
                    }
                }
            }
            width
        })
        .collect();
    let mut col_x: Vec<f32> = Vec::with_capacity(rows.len());
    let mut acc = 0.0f32;
    for (row, _) in rows.iter().enumerate() {
        if row > 0 {
            acc += row_widths[row - 1] + gen_gap;
        }
        col_x.push(acc);
    }
    let widths: HashMap<&str, f32> = data
        .people
        .iter()
        .map(|person| (person.id.as_str(), card_width_for(person, painter)))
        .collect();
    // Horizontal trees give every card of a generation the same width.
    let card_w = |row: usize, id: &str| -> f32 {
        if orientation == TreeOrientation::Horizontal {
            row_widths[row]
        } else {
            widths[id]
        }
    };
    let group_of: HashMap<&str, &str> = if view == TreeView::Descendants {
        let mut map = HashMap::new();
        for family in &data.families {
            for child in &family.children {
                map.insert(child.as_str(), family.id.as_str());
            }
        }
        map
    } else {
        HashMap::new()
    };

    // Initial packing, then negotiate: children pull toward their parents'
    // junction, parents follow their children, overlapping cards push apart.
    let mut spread: HashMap<&str, f32> = HashMap::new();
    for (row, ids) in rows.iter().enumerate() {
        if view == TreeView::Fan {
            continue;
        }
        let mut footprints: Vec<(&str, f32)> = Vec::new();
        let mut total = 0.0f32;
        for id in ids {
            let mut footprint = card_w(row, id);
            for partner in data.partners_of(id) {
                if !visible(&partner.id) {
                    footprint += card_w(row, &partner.id) + couple_gap;
                }
            }
            footprints.push((*id, footprint));
            total += footprint;
        }
        total += gap * footprints.len().saturating_sub(1) as f32;
        let mut cursor = -total / 2.0;
        for (id, footprint) in footprints {
            spread.insert(id, cursor + card_w(row, id) / 2.0);
            cursor += footprint + gap;
        }
    }
    for _ in 0..12 {
        for (row, ids) in rows.iter().enumerate() {
            if view == TreeView::Fan {
                continue;
            }
            for id in ids {
                let mut cursor = spread[*id] + card_w(row, id) / 2.0 + couple_gap;
                for partner in data.partners_of(id) {
                    if visible(&partner.id) {
                        continue;
                    }
                    spread.insert(
                        partner.id.as_str(),
                        cursor + card_w(row, &partner.id) / 2.0,
                    );
                    cursor += card_w(row, &partner.id) + couple_gap;
                }
            }
        }
            let junction = parents.iter().map(|p| spread[*p]).sum::<f32>() / parents.len() as f32;
            if view == TreeView::Descendants {
                // Siblings move as one rigid group (they share the container).
                let mean = children.iter().map(|c| spread[*c]).sum::<f32>() / children.len() as f32;
                let delta = 0.5 * (junction - mean);
                for child in &children {
                    if let Some(value) = spread.get_mut(child) {
                        *value += delta;
                    }
                }
            } else {
                for child in &children {
                    if let Some(value) = spread.get_mut(child) {
                        *value += 0.5 * (junction - *value);
                    }
                }
            }
            let target = children.iter().map(|c| spread[*c]).sum::<f32>() / children.len() as f32;
            let parent_mean =
                parents.iter().map(|p| spread[*p]).sum::<f32>() / parents.len() as f32;
            let delta = 0.4 * (target - parent_mean);
            for parent in &parents {
                if let Some(value) = spread.get_mut(parent) {
                    *value += delta;
                }
            }
        }
        for (row, ids) in rows.iter().enumerate() {
            if view == TreeView::Fan {
                continue;
            }
            let mut members: Vec<(&str, f32, f32)> = ids
                .iter()
                .map(|id| {
                    let mut footprint = card_w(row, id);
                    for partner in data.partners_of(id) {
                        if !visible(&partner.id) {
                            footprint += card_w(row, &partner.id) + couple_gap;
                        }
                    }
                    (*id, spread[*id], footprint)
                })
                .collect();
            members.sort_by(|a, b| a.1.total_cmp(&b.1));
            // Group members by origin family (descendants) so containers can
            // negotiate as rigid blocks, individually inside and against each
            // other outside.
            let group_key = |id: &str| group_of.get(id).copied().unwrap_or(id);
            let mut groups: Vec<Vec<(&str, f32, f32)>> = Vec::new();
            for member in members {
                match groups.last_mut() {
                    Some(last) if group_key(last[0].0) == group_key(member.0) => {
                        last.push(member)
                    }
                    _ => groups.push(vec![member]),
                }
            }
            // Inside a group (and for singleton groups) push cards apart.
            for group in &groups {
                let mut inner: Vec<(&str, f32, f32)> = group.clone();
                inner.sort_by(|a, b| a.1.total_cmp(&b.1));
                for window in inner.windows(2) {
                    let need = (window[0].2 + window[1].2) / 2.0 + gap;
                    let actual = window[1].1 - window[0].1;
                    if actual < need {
                        let deficit = need - actual;
                        if let Some(value) = spread.get_mut(window[0].0) {
                            *value -= deficit / 2.0;
                        }
                        if let Some(value) = spread.get_mut(window[1].0) {
                            *value += deficit / 2.0;
                        }
                    }
                }
            }
            // Outer repulsion between groups (containers).
            let mut spans: Vec<(Vec<(&str, f32, f32)>, f32, f32)> = groups
                .iter()
                .map(|group| {
                    let start = group
                        .iter()
                        .map(|(_, center, footprint)| center - footprint / 2.0)
                        .fold(f32::MAX, f32::min);
                    let end = group
                        .iter()
                        .map(|(_, center, footprint)| center + footprint / 2.0)
                        .fold(f32::MIN, f32::max);
                    (group.clone(), (start + end) / 2.0, end - start)
                })
                .collect();
            spans.sort_by(|a, b| a.1.total_cmp(&b.1));
            for window in spans.windows(2) {
                let need = (window[0].2 + window[1].2) / 2.0 + gap;
                let actual = window[1].1 - window[0].1;
                if actual < need {
                    let deficit = need - actual;
                    let shift_left = -deficit / 2.0;
                    let shift_right = deficit / 2.0;
                    for (id, _, _) in &window[0].0 {
                        if let Some(value) = spread.get_mut(id) {
                            *value += shift_left;
                        }
                    }
                    for (id, _, _) in &window[1].0 {
                        if let Some(value) = spread.get_mut(id) {
                            *value += shift_right;
                        }
                    }
                }
            }
        }
    }

    let mut positions: HashMap<&str, (f32, f32)> = HashMap::new();
    for (row, ids) in rows.iter().enumerate() {
        if view == TreeView::Fan {
            for (index, id) in ids.iter().enumerate() {
                let count = ids.len();
                let angle = if count <= 1 {
                    if orientation == TreeOrientation::Vertical {
                        -std::f32::consts::FRAC_PI_2
                    } else {
                        0.
                    }
                } else if orientation == TreeOrientation::Vertical {
                    std::f32::consts::PI + std::f32::consts::PI * index as f32 / (count - 1) as f32
                } else {
                    -std::f32::consts::FRAC_PI_2
                        + std::f32::consts::PI * index as f32 / (count - 1) as f32
                };
                let radius = 175. * row as f32;
                positions.insert(*id, (radius * angle.cos(), radius * angle.sin()));
            }
            continue;
        }
        for id in ids {
            let s = spread[*id];
            if orientation == TreeOrientation::Vertical {
                let y = row as f32 * (card_h + 40.0);
                positions.insert(*id, (s, y));
                let mut px = s + card_w(row, id) / 2.0 + couple_gap;
                for partner in data.partners_of(id) {
                    if visible(&partner.id) {
                        continue;
                    }
                    let pw = card_w(row, &partner.id);
                    positions.insert(partner.id.as_str(), (px + pw / 2.0, y));
                    px += pw + couple_gap;
                }
            } else {
                let x = col_x[row] + card_w(row, id) / 2.0;
                positions.insert(*id, (x, s));
                let mut py = s + card_h + couple_gap;
                for partner in data.partners_of(id) {
                    if visible(&partner.id) {
                        continue;
                    }
                    positions.insert(partner.id.as_str(), (x, py + card_h / 2.0));
                    py += card_h + couple_gap;
                }
            }
        }
    }
    let center = rect.center() + pan;
    let pos = |id: &str| {
        positions
            .get(id)
            .map(|(x, y)| center + Vec2::new(*x * zoom, *y * zoom))
    };
    // Couple frames (merged pair instances) for the descendants view.
    if view == TreeView::Descendants {
        for person in &data.people {
            if !visible(&person.id) {
                continue;
            }
            let Some(at) = pos(&person.id) else {
                continue;
            };
            let Some(row) = levels.get(&person.id) else {
                continue;
            };
            let mut frame = Rect::from_center_size(
                at,
                Vec2::new(card_w(*row, &person.id), card_h) * zoom,
            );
            let mut merged = false;
            for partner in data.partners_of(&person.id) {
                if visible(&partner.id) {
                    continue;
                }
                if let Some(pp) = pos(&partner.id) {
                    frame = frame.union(Rect::from_center_size(
                        pp,
                        Vec2::new(card_w(*row, &partner.id), card_h) * zoom,
                    ));
                    merged = true;
                }
            }
            if merged {
                painter.rect_stroke(
                    frame.expand(4.0 * zoom),
                    12.0 * zoom,
                    Stroke::new(1.5, Color32::from_rgb(120, 170, 160)),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }
    for family in &data.families {
        let pa = family.parent_a.as_deref().and_then(&pos);
        let pb = family.parent_b.as_deref().and_then(&pos);
        if view == TreeView::Descendants {
            // One fused couple block above, one sibling container below (only
            // when more than one sibling is visible), and a single connector
            // line starting at the couple frame.
            let couple_frame: Option<Rect> = {
                let mut frame: Option<Rect> = None;
                for parent in [&family.parent_a, &family.parent_b].into_iter().flatten() {
                    if let Some(pp) = pos(parent) {
                        let Some(prow) = levels.get(parent) else {
                            continue;
                        };
                        let rect = Rect::from_center_size(
                            pp,
                            Vec2::new(card_w(*prow, parent), card_h) * zoom,
                        );
                        frame = Some(match frame {
                            Some(previous) => previous.union(rect),
                            None => rect,
                        });
                    }
                }
                frame
            };
            let Some(couple_frame) = couple_frame else {
                continue;
            };
            let mut blocks: Vec<Rect> = Vec::new();
            for child in &family.children {
                let Some(at) = pos(child) else {
                    continue;
                };
                let Some(crow) = levels.get(child) else {
                    continue;
                };
                let mut block = Rect::from_center_size(
                    at,
                    Vec2::new(card_w(*crow, child), card_h) * zoom,
                );
                for partner in data.partners_of(child) {
                    if visible(&partner.id) {
                        continue;
                    }
                    if let Some(pp) = pos(&partner.id) {
                        block = block.union(Rect::from_center_size(
                            pp,
                            Vec2::new(card_w(*crow, &partner.id), card_h) * zoom,
                        ));
                    }
                }
                blocks.push(block);
            }
            if blocks.is_empty() {
                continue;
            }
            let origin = if orientation == TreeOrientation::Vertical {
                Pos2::new(couple_frame.center().x, couple_frame.bottom())
            } else {
                Pos2::new(couple_frame.right(), couple_frame.center().y)
            };
            let mut container: Option<Rect> = None;
            if blocks.len() > 1 {
                let mut bounds = blocks[0];
                for block in &blocks[1..] {
                    bounds = bounds.union(*block);
                }
                let container_rect = bounds.expand(12.0 * zoom);
                painter.rect_filled(
                    container_rect,
                    10.0 * zoom,
                    Color32::from_rgba_unmultiplied(120, 170, 160, 14),
                );
                painter.rect_stroke(
                    container_rect,
                    10.0 * zoom,
                    Stroke::new(1.0, Color32::from_rgb(70, 105, 110)),
                    egui::StrokeKind::Outside,
                );
                container = Some(container_rect);
            }
            let target = match &container {
                Some(container_rect) => {
                    if orientation == TreeOrientation::Vertical {
                        Pos2::new(
                            origin.x.clamp(
                                container_rect.left() + 10.0 * zoom,
                                container_rect.right() - 10.0 * zoom,
                            ),
                            container_rect.top(),
                        )
                    } else {
                        Pos2::new(
                            container_rect.left(),
                            origin.y.clamp(
                                container_rect.top() + 10.0 * zoom,
                                container_rect.bottom() - 10.0 * zoom,
                            ),
                        )
                    }
                }
                None => {
                    let block = blocks[0];
                    if orientation == TreeOrientation::Vertical {
                        Pos2::new(
                            origin.x.clamp(block.left() + 10.0 * zoom, block.right() - 10.0 * zoom),
                            block.top(),
                        )
                    } else {
                        Pos2::new(
                            block.left(),
                            origin.y.clamp(block.top() + 10.0 * zoom, block.bottom() - 10.0 * zoom),
                        )
                    }
                }
            };
            painter.line_segment(
                [origin, target],
                Stroke::new(2.0, Color32::from_rgb(74, 111, 119)),
            );
            continue;
        }
        if let (Some(a), Some(b)) = (pa, pb) {
            painter.line_segment([a, b], Stroke::new(2.0, Color32::from_rgb(120, 170, 160)));
        }
        let junction = match (pa, pb) {
            (Some(a), Some(b)) => Pos2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0),
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => continue,
        };
        for child in &family.children {
            if let Some(c) = pos(child) {
                painter.line_segment(
                    [junction, c],
                    Stroke::new(2.0, Color32::from_rgb(74, 111, 119)),
                );
            }
        }
    }
    for person in &data.people {
        let Some(at) = pos(&person.id) else {
            continue;
        };
        let Some(level) = levels.get(&person.id) else {
            continue;
        };
        let width = match view {
            TreeView::Fan => 260.0,
            _ => card_w(*level, &person.id),
        };
        let card = Rect::from_center_size(at, Vec2::new(width, card_h) * zoom);
        let card_clicked = draw_person_card(
            painter,
            person,
            at,
            width,
            zoom,
            viewed == Some(person.id.as_str()),
            false,
            photo_cache,
        );
        let has_more = {
            let relatives = if ancestors {
                data.parents_of(&person.id)
            } else {
                data.children_of(&person.id)
            };
            relatives
                .iter()
                .any(|relative| !levels.contains_key(&relative.id))
        };
        let mut badge_clicked = false;
        if view != TreeView::Fan && (has_more || expanded.contains(&person.id)) {
            let badge_at = Pos2::new(card.right() - 12.0 * zoom, card.top() + 12.0 * zoom);
            let badge_r = 9.0 * zoom;
            painter.circle_filled(badge_at, badge_r, Color32::from_rgb(24, 40, 48));
            painter.circle_stroke(
                badge_at,
                badge_r,
                Stroke::new(1.0, Color32::from_rgb(120, 170, 160)),
            );
            painter.text(
                badge_at,
                Align2::CENTER_CENTER,
                if expanded.contains(&person.id) {
                    "-"
                } else {
                    "+"
                },
                FontId::proportional(11.0 * zoom),
                Color32::WHITE,
            );
            let badge_rect = Rect::from_center_size(badge_at, Vec2::splat(badge_r * 2.0));
            badge_clicked = painter.ctx().input(|i| {
                i.pointer.any_click()
                    && i.pointer
                        .interact_pos()
                        .is_some_and(|q| badge_rect.contains(q))
            });
            if badge_clicked {
                *action = Some(TreeAction::ToggleExpand(person.id.clone()));
            }
        }
        if !badge_clicked && card_clicked {
            let double_clicked = painter.ctx().input(|i| i.pointer.double_click());
            let shift = painter.ctx().input(|i| i.modifiers.shift);
            *action = Some(if shift || double_clicked {
                TreeAction::Reference(person.id.clone())
            } else {
                TreeAction::View(person.id.clone())
            });
        }
        if !badge_clicked {
            let long_pressed = painter.ctx().input(|i| {
                i.pointer.press_origin().is_some_and(|q| card.contains(q))
                    && i.pointer
                        .press_start_time()
                        .is_some_and(|t0| i.time - t0 >= 0.6)
            });
            if long_pressed && !*long_press_used {
                *action = Some(TreeAction::Reference(person.id.clone()));
                *long_press_used = true;
            }
        }
        if view == TreeView::Fan {
            continue;
        }
        for partner in data.partners_of(&person.id) {
            if visible(&partner.id) {
                continue;
            }
            let Some(pat) = pos(&partner.id) else {
                continue;
            };
            let partner_width = card_w(*level, &partner.id);
            let (line_a, line_b) = if orientation == TreeOrientation::Vertical {
                (
                    Pos2::new(at.x + width * zoom / 2.0, at.y),
                    Pos2::new(pat.x - partner_width * zoom / 2.0, pat.y),
                )
            } else {
                (
                    Pos2::new(at.x, at.y + card_h * zoom / 2.0),
                    Pos2::new(pat.x, pat.y - card_h * zoom / 2.0),
                )
            };
            if view != TreeView::Descendants {
                painter.line_segment(
                    [line_a, line_b],
                    Stroke::new(2.0, Color32::from_rgb(120, 170, 160)),
                );
            }
            let partner_card = Rect::from_center_size(pat, Vec2::new(partner_width, card_h) * zoom);
            let partner_clicked = draw_person_card(
                painter,
                partner,
                pat,
                partner_width,
                zoom,
                viewed == Some(partner.id.as_str()),
                true,
                photo_cache,
            );
            let partner_long = painter.ctx().input(|i| {
                i.pointer
                    .press_origin()
                    .is_some_and(|q| partner_card.contains(q))
                    && i.pointer
                        .press_start_time()
                        .is_some_and(|t0| i.time - t0 >= 0.6)
            });
            if partner_clicked {
                let double_clicked = painter.ctx().input(|i| i.pointer.double_click());
                let shift = painter.ctx().input(|i| i.modifiers.shift);
                *action = Some(if shift || double_clicked {
                    TreeAction::Reference(partner.id.clone())
                } else {
                    TreeAction::View(partner.id.clone())
                });
            }
            if partner_long && !*long_press_used {
                *action = Some(TreeAction::Reference(partner.id.clone()));
                *long_press_used = true;
            }
        }
    }
}

fn card_width_for(person: &Person, painter: &egui::Painter) -> f32 {
    let name = painter
        .layout_no_wrap(
            person.name.clone(),
            FontId::proportional(13.0),
            Color32::WHITE,
        )
        .size()
        .x;
    let birth_text = if person.birth.is_empty() {
        "Unbekannt".to_string()
    } else {
        person.birth.clone()
    };
    let birth = painter
        .layout_no_wrap(birth_text, FontId::proportional(12.0), Color32::WHITE)
        .size()
        .x;
    (64.0 + name.max(birth) + 20.0).max(215.0)
}

fn draw_person_card(
    painter: &egui::Painter,
    person: &Person,
    at: Pos2,
    width: f32,
    zoom: f32,
    selected_now: bool,
    muted: bool,
    photo_cache: &mut HashMap<String, TextureHandle>,
) -> bool {
    let size = Vec2::new(width, 78.0) * zoom;
    let card = Rect::from_center_size(at, size);
    let fill = match (person.gender, selected_now) {
        (_, true) => Color32::from_rgb(43, 121, 113),
        (Gender::Female, _) => Color32::from_rgb(112, 66, 72),
        (Gender::Male, _) => Color32::from_rgb(50, 70, 108),
        _ => Color32::from_rgb(56, 65, 75),
    };
    let stroke_color = if muted {
        Color32::from_rgb(108, 138, 138)
    } else {
        Color32::from_rgb(158, 213, 199)
    };
    painter.rect(
        card,
        10. * zoom,
        fill,
        Stroke::new(
            if selected_now {
                2.5
            } else if muted {
                0.8
            } else {
                1.0
            },
            stroke_color,
        ),
        egui::StrokeKind::Outside,
    );
    let avatar_size = 48. * zoom;
    let avatar = Rect::from_center_size(
        card.left_center() + Vec2::new(35. * zoom, 0.),
        Vec2::splat(avatar_size),
    );
    if let Some(texture) = photo_texture(painter.ctx(), person, photo_cache) {
        painter.image(
            texture.id(),
            avatar,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
            Color32::WHITE,
        );
    } else {
        painter.circle_filled(
            avatar.center(),
            avatar_size / 2.,
            Color32::from_rgb(55, 91, 101),
        );
        painter.text(
            avatar.center(),
            Align2::CENTER_CENTER,
            initials(person),
            FontId::proportional(13. * zoom),
            Color32::WHITE,
        );
    }
    painter.text(
        card.left_top() + Vec2::new(64. * zoom, 21. * zoom),
        Align2::LEFT_CENTER,
        &person.name,
        FontId::proportional(13. * zoom),
        Color32::WHITE,
    );
    painter.text(
        card.left_bottom() + Vec2::new(64. * zoom, -17. * zoom),
        Align2::LEFT_CENTER,
        if person.birth.is_empty() {
            "Unbekannt"
        } else {
            &person.birth
        },
        FontId::proportional(12. * zoom),
        Color32::from_rgb(202, 222, 221),
    );
    painter.ctx().input(|i| {
        i.pointer.any_click() && i.pointer.interact_pos().is_some_and(|q| card.contains(q))
    })
}

fn default_library() -> PathBuf {
    ProjectDirs::from("org", "minigramps", "MiniGramps")
        .map(|d| d.data_local_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("minigramps-data"))
}

fn discover_projects(library: &Path, custom_locations: &[PathBuf]) -> Vec<PathBuf> {
    let mut folders = vec![library.to_path_buf(), default_library()];
    if let Some(documents) =
        directories::UserDirs::new().and_then(|dirs| dirs.document_dir().map(Path::to_path_buf))
    {
        folders.push(documents.join("Gramps"));
    }
    folders.extend(custom_locations.iter().cloned());
    let mut projects = Vec::new();
    for folder in folders {
        collect_project_files(&folder, &mut projects, 3);
    }
    projects.sort();
    projects
}

fn collect_project_files(folder: &Path, projects: &mut Vec<PathBuf>, depth: u8) {
    if let Ok(entries) = fs::read_dir(folder) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && depth > 0 {
                collect_project_files(&path, projects, depth - 1);
            }
            let supported = path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
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

fn load_file(path: &Path) -> Result<TreeData, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let bytes = gunzip_if_needed(bytes)?;
    let content = decode_bytes(&bytes);
    match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "json" => serde_json::from_str(&content).map_err(|e| e.to_string()),
        "ged" | "gedcom" => parse_gedcom(&content),
        "gramps" | "xml" => parse_gramps_xml(&content),
        _ => Err("Nicht unterstütztes Dateiformat".into()),
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

fn parse_gedcom(text: &str) -> Result<TreeData, String> {
    let mut data = TreeData::default();
    let mut current_person: Option<Person> = None;
    let mut current_family: Option<usize> = None;
    let mut current_event: Option<&str> = None;
    for line in text.lines() {
        let part: Vec<_> = line.split_whitespace().collect();
        if part.len() < 2 {
            continue;
        }
        if part[0] == "0" {
            if let Some(person) = current_person.take() {
                data.people.push(person);
            }
            current_family = None;
            current_event = None;
            if part.get(2) == Some(&"INDI") {
                current_person = Some(person(
                    part[1].trim_matches('@'),
                    "Unbenannt",
                    "",
                    Gender::Unknown,
                ));
            } else if part.get(2) == Some(&"FAM") {
                let id = part[1].trim_matches('@').to_string();
                data.families.push(Family {
                    id,
                    parent_a: None,
                    parent_b: None,
                    children: vec![],
                });
                current_family = Some(data.families.len() - 1);
            }
        } else if let Some(p) = current_person.as_mut() {
            match part.get(1).copied() {
                Some("NAME") => {
                    p.name = part[2..].join(" ").replace('/', "");
                    current_event = None;
                }
                Some("SEX") => {
                    p.gender = match part.get(2) {
                        Some(&"M") => Gender::Male,
                        Some(&"F") => Gender::Female,
                        _ => Gender::Unknown,
                    };
                }
                Some("BIRT") => current_event = Some("BIRT"),
                Some("DEAT") => current_event = Some("DEAT"),
                Some("DATE") => match current_event {
                    Some("BIRT") => p.birth = part[2..].join(" "),
                    Some("DEAT") => p.death = part[2..].join(" "),
                    _ => {}
                },
                Some("PLAC") => match current_event {
                    Some("BIRT") => p.birth_place = part[2..].join(" "),
                    Some("DEAT") => p.death_place = part[2..].join(" "),
                    _ => {}
                },
                _ => {
                    if part[0] == "1" {
                        current_event = None;
                    }
                }
            }
        }
        if part.len() >= 3 {
            let value = part[2].trim_matches('@').to_string();
            if let Some(family_index) = current_family {
                let f = &mut data.families[family_index];
                match part[1] {
                    "HUSB" => f.parent_a = Some(value),
                    "WIFE" => f.parent_b = Some(value),
                    "CHIL" => f.children.push(value),
                    _ => {}
                }
            }
        }
    }
    if let Some(person) = current_person {
        data.people.push(person);
    }
    if data.people.is_empty() {
        Err("Keine Personen in GEDCOM gefunden".into())
    } else {
        Ok(data)
    }
}

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
    let mut events: HashMap<String, (&str, String)> = HashMap::new();
    for node in doc.descendants().filter(|n| n.has_tag_name("event")) {
        let handle = node.attribute("handle").unwrap_or_default().to_string();
        let event_type = node.attribute("type").unwrap_or_default();
        let date = event_date(node);
        if let Some(date) = date {
            events.insert(handle, (event_type, date));
        }
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
        for reference in node.children().filter(|n| n.has_tag_name("eventref")) {
            let Some(link) = reference.attribute("hlink") else {
                continue;
            };
            if let Some((event_type, date)) = events.get(link) {
                if event_type.eq_ignore_ascii_case("Birth") {
                    birth = date.clone();
                } else if event_type.eq_ignore_ascii_case("Death") {
                    death = date.clone();
                }
            }
        }
        data.people.push(person(
            &id,
            format!("{name} {surname}").trim(),
            &birth,
            gender,
        ));
        if let Some(p) = data.people.last_mut() {
            p.death = death;
        }
    }
    for node in doc.descendants().filter(|n| n.has_tag_name("family")) {
        let mut f = Family {
            id: node.attribute("handle").unwrap_or_default().into(),
            parent_a: None,
            parent_b: None,
            children: vec![],
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

fn main() -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1280., 760.])
        .with_min_inner_size([900., 560.]);
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

fn configure_fonts(ctx: &egui::Context) {
    egui_extras::install_image_loaders(ctx);
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "libre-baskerville".into(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/LibreBaskerville-Regular.ttf"
        ))),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "libre-baskerville".into());
    ctx.set_fonts(fonts);
}

fn whitened_svg(bytes: &'static [u8]) -> Vec<u8> {
    if bytes.windows(12).any(|window| window == b"currentColor") {
        String::from_utf8_lossy(bytes)
            .replace("currentColor", "#ffffff")
            .into_bytes()
    } else {
        bytes.to_vec()
    }
}

fn icon(ui: &mut egui::Ui, bytes: &'static [u8], id: &'static str) {
    ui.add(
        egui::Image::from_bytes(format!("bytes://{id}.svg"), whitened_svg(bytes))
            .fit_to_exact_size(Vec2::splat(15.0))
            .tint(ui.visuals().text_color()),
    );
}

fn icon_button(
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

fn icon_only_button(ui: &mut egui::Ui, bytes: &'static [u8], id: &'static str) -> egui::Response {
    let image = egui::Image::from_bytes(format!("bytes://{id}.svg"), whitened_svg(bytes))
        .fit_to_exact_size(Vec2::splat(16.0))
        .tint(ui.visuals().text_color());
    ui.add(egui::Button::image(image))
}

fn icon_button_big(
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

fn window_title(text: &str) -> egui::RichText {
    egui::RichText::new(text).size(13.0).strong()
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
        assert_eq!(data.people[0].name, "Jonas Doe");
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
    fn imports_gedcom_family_relationships() {
        let data = parse_gedcom("0 @I1@ INDI\n1 NAME Alex /Muster/\n1 SEX M\n0 @I2@ INDI\n1 NAME Bea /Muster/\n1 SEX F\n0 @I3@ INDI\n1 NAME Chris /Muster/\n0 @F1@ FAM\n1 HUSB @I1@\n1 WIFE @I2@\n1 CHIL @I3@\n").unwrap();
        assert_eq!(data.people.len(), 3);
        assert_eq!(data.parents_of("I3").len(), 2);
        assert_eq!(data.children_of("I1")[0].name, "Chris Muster");
        assert_eq!(data.partners_of("I1")[0].name, "Bea Muster");
    }

    #[test]
    fn finds_siblings_in_shared_family() {
        let data = TreeData::demo();
        assert_eq!(data.siblings_of("p5")[0].id, "p6");
        assert!(data.parents_of("p1").is_empty());
    }

    #[test]
    fn creates_family_links_without_duplicates() {
        let mut data = TreeData {
            people: vec![
                person("a", "Alex", "", Gender::Unknown),
                person("b", "Bea", "", Gender::Unknown),
                person("c", "Chris", "", Gender::Unknown),
            ],
            families: Vec::new(),
            child_relations: HashMap::new(),
        };
        data.link_partner("a", "b");
        data.link_child("a", "c");
        data.link_child("a", "c");
        assert_eq!(data.partners_of("a")[0].id, "b");
        assert_eq!(data.parents_of("c")[0].id, "a");
        assert_eq!(data.families[0].children.len(), 1);
    }

    #[test]
    fn stores_child_relation_with_family() {
        let mut data = TreeData {
            people: vec![
                person("a", "Alex", "", Gender::Unknown),
                person("b", "Bea", "", Gender::Unknown),
                person("c", "Chris", "", Gender::Unknown),
            ],
            families: Vec::new(),
            child_relations: HashMap::new(),
        };
        data.link_child_to(Some("a"), Some("b"), "c", ChildRelation::Adopted);
        data.link_child_to(Some("a"), Some("b"), "c", ChildRelation::Adopted);
        assert_eq!(data.families.len(), 1);
        assert_eq!(data.parents_of("c").len(), 2);
        assert_eq!(data.relation_of_child("a", "c"), ChildRelation::Adopted);
        assert_eq!(data.relation_of_child("b", "c"), ChildRelation::Adopted);
    }
}
