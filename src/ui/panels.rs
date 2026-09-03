//! Farbwelten der Oberfläche.
//!
//! Verdrahtung:
//! - `palette` liefert die Themenfarben je Modus (dunkel/hell); genutzt von
//!   `header::show`, `sidebar::show_left/show_right`, `dialogs` und dem
//!   zentralen Panel in `ui::MiniGramps::update`.
//! - Das Debug-Log ist nur noch im Terminal (`MiniGramps::log` → `println!`),
//!   es gibt keine Log-Leiste mehr in der Oberfläche.

use eframe::egui::{self, Color32};

/// Themenfarben der Oberfläche.
pub struct Palette {
    pub header: Color32,
    pub panel: Color32,
    pub canvas: Color32,
    pub brand: Color32,
    pub accent_dim: Color32,
    pub section: Color32,
}

pub fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            header: Color32::from_rgb(24, 56, 68),
            panel: Color32::from_rgb(23, 33, 43),
            canvas: Color32::from_rgb(15, 22, 30),
            brand: Color32::from_rgb(161, 224, 204),
            accent_dim: Color32::from_rgb(170, 200, 206),
            section: Color32::from_rgb(135, 191, 183),
        }
    } else {
        Palette {
            header: Color32::from_rgb(211, 235, 229),
            panel: Color32::from_rgb(242, 247, 246),
            canvas: Color32::from_rgb(250, 252, 251),
            // Tannengrün für den hellen Modus.
            brand: Color32::from_rgb(0, 86, 58),
            accent_dim: Color32::from_rgb(43, 107, 88),
            section: Color32::from_rgb(0, 105, 70),
        }
    }
}

/// Gedämpfte neutrale Textfarbe je Modus: im Dunkelmodus heller als
/// egui-Grau (128), im Hellmodus wie bisher.
pub fn dim_text(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_gray(178)
    } else {
        Color32::GRAY
    }
}
