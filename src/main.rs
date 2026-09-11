//! MiniGramps – ein kleines, visuelles Familienarchiv (Rust + egui).
//!
//! Modulaufbau (mit Querverweisen):
//! - [`model`]   – Datenmodell (`TreeData`, `Person`, `Family`) inkl. der
//!   Verwandtschafts- und Verknüpfungslogik, die von `import` (beim Laden
//!   füllt), `ui` (Seitenleisten, Beziehungspicker) und `ui::tree`
//!   (Graph-Traversal) benutzt wird.
//! - [`import`]  – Laden/Parsen (GEDCOM, Gramps-XML, JSON), Kodierungen
//!   (UTF-8/UTF-16/CP1252), GZIP-Sicherungen und die Projekt-Suche für den
//!   Öffnen-Dialog.
//! - [`media`]   – Foto-Verwaltung nach dem Gramps-Prinzip (relative Pfade
//!   unter `<Datenordner>/media`, Inhalts-Hash als Dateiname) sowie Avatar-
//!   und Textur-Helfer.
//! - [`ui`]      – App-Zustand (`MiniGramps`), alle Panels/Dialoge und die
//!   Baumansicht in [`ui::tree`] (Generations-Layout mit Verhandlung).

#![allow(clippy::collapsible_if, clippy::too_many_arguments)]

mod import;
mod media;
mod model;
mod settings;
mod store;
mod ui;

fn main() -> eframe::Result<()> {
    ui::run()
}
