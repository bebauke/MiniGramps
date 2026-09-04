//! Medien (Fotos): Speichern nach Gramps-Prinzip, Texturen, Avatare.
//!
//! Verdrahtung:
//! - `import_media_file` kopiert ein ausgewähltes Foto in den Medien-
//!   Basisordner `<library>/media` und liefert den relativen Pfad
//!   `media/<inhalts-hash>.<ext>` (Prüfsummen-Prinzip wie bei Gramps).
//!   Aufgerufen beim Setzen von Profilfotos und Galeriebildern.
//! - `photo_texture` löst relative Pfade gegen den Medien-Basisordner auf
//!   (`MiniGramps::library` wird als `media_base` durchgereicht) und cached
//!   Texturen in `MiniGramps::photo_cache`, Schlüssel = `Person::id`. Die
//!   Galerie erzeugt Pseudo-IDs `gallery-<id>-<index>` (siehe `ui`), um
//!   mehrere Bilder derselben Person zu cachen.
//! - `avatar_ui` wird in der rechten Seitenleiste (Profil, Beziehungsreihen,
//!   Galerie) genutzt; `ui::tree` zeichnet Karten mit `photo_texture` und
//!   `initials` direkt.

use std::{
    collections::HashMap,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    sync::{Mutex, OnceLock, mpsc},
    thread,
    time::UNIX_EPOCH,
};

use eframe::egui::{self, Align2, Color32, FontId, Sense, TextureHandle, Vec2};
use image::{GenericImageView, RgbaImage, imageops::FilterType};

use crate::model::{Person, PhotoCrop};

static PENDING_THUMBS: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
static ACTIVE_THUMBS: AtomicUsize = AtomicUsize::new(0);
const MAX_ACTIVE_THUMBS: usize = 2;

/// UV-Fenster für den Cover-Beschnitt: Das Bild füllt den Rahmen vollständig
/// OHNE Verzerrung (Seitenverhältnis wird beschnitten); `crop` verschiebt
/// und zoomt den Ausschnitt (zoom ≥ 1).
pub fn cover_uv(aspect: f32, crop: Option<&PhotoCrop>) -> egui::Rect {
    cover_uv_to(aspect, 1.0, crop)
}

/// UV-Fenster für ein beliebiges Ziel-Seitenverhältnis. Verhindert Stretching,
/// wenn z.B. Galerie-Thumbnails rechteckig statt quadratisch sind.
pub fn cover_uv_to(aspect: f32, target_aspect: f32, crop: Option<&PhotoCrop>) -> egui::Rect {
    let target_aspect = target_aspect.max(0.01);
    let (mut w, mut h) = if aspect > target_aspect {
        (target_aspect / aspect, 1.0)
    } else {
        (1.0, aspect / target_aspect)
    };
    let zoom = crop.map(|c| c.zoom.max(1.0)).unwrap_or(1.0);
    w /= zoom;
    h /= zoom;
    let (cx, cy) = crop.map(|c| (c.x, c.y)).unwrap_or((0.0, 0.0));
    let max_x = (1.0 - w) / 2.0;
    let max_y = (1.0 - h) / 2.0;
    let cx = (cx * max_x).clamp(-max_x, max_x);
    let cy = (cy * max_y).clamp(-max_y, max_y);
    egui::Rect::from_min_max(
        egui::pos2(0.5 - w / 2.0 + cx, 0.5 - h / 2.0 + cy),
        egui::pos2(0.5 + w / 2.0 + cx, 0.5 + h / 2.0 + cy),
    )
}

/// Foto in den Medien-Basisordner kopieren und relativen Pfad liefern.
/// Der Dateiname ist ein Inhalts-Hash, doppelte Bilder werden so vermieden.
#[cfg(test)]
pub fn import_media_file(library: &Path, source: &Path) -> Option<String> {
    let (relative, target) = media_target(library, source)?;
    if !target.exists() {
        fs::create_dir_all(target.parent()?).ok()?;
        fs::copy(source, &target).ok()?;
    }
    Some(relative)
}

/// Schneller UI-Import: liefert sofort den Zielpfad, kopiert das Original aber
/// im Hintergrund. Galerie/Profil zeigen bis dahin ihren Platzhalter/Cache.
pub fn import_media_file_async(
    ctx: &egui::Context,
    library: &Path,
    source: &Path,
) -> Option<String> {
    let (relative, target) = media_target(library, source)?;
    if target.exists() {
        return Some(relative);
    }
    let source = source.to_path_buf();
    let key = format!("copy:{}", target.display());
    let target_for_job = target.clone();
    spawn_thumb_job(key, ctx.clone(), move || {
        if let Some(parent) = target_for_job.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::copy(source, target_for_job);
    });
    Some(relative)
}

fn media_target(library: &Path, source: &Path) -> Option<(String, PathBuf)> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let metadata = fs::metadata(source).ok()?;
    source.hash(&mut hasher);
    metadata.len().hash(&mut hasher);
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .hash(&mut hasher);
    let digest = hasher.finish();
    let extension = source
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .unwrap_or_else(|| "jpg".into());
    let relative = format!("media/{digest:016x}.{extension}");
    let target = library.join(&relative);
    Some((relative, target))
}

/// Textur für das Profilbild einer Person (lazy geladen und gecached).
/// Relative Pfade gelten gegenüber `media_base` (= `MiniGramps::library`).
#[allow(dead_code)]
pub fn photo_texture<'a>(
    ctx: &egui::Context,
    person: &Person,
    cache: &'a mut HashMap<String, TextureHandle>,
    media_base: &Path,
) -> Option<&'a TextureHandle> {
    photo_texture_with_limit(ctx, person, cache, media_base, None, "")
}

#[allow(dead_code)]
fn photo_texture_with_limit<'a>(
    ctx: &egui::Context,
    person: &Person,
    cache: &'a mut HashMap<String, TextureHandle>,
    media_base: &Path,
    max_side: Option<u32>,
    cache_prefix: &str,
) -> Option<&'a TextureHandle> {
    let cache_key = format!("{cache_prefix}{}", person.id);
    if !cache.contains_key(&cache_key) {
        let photo = person.photo.as_deref()?;
        let raw = Path::new(photo);
        let path = if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            media_base.join(raw)
        };
        let image = if let Some(max_side) = max_side {
            image::open(path)
                .ok()?
                .thumbnail(max_side, max_side)
                .to_rgba8()
        } else {
            image::open(path).ok()?.to_rgba8()
        };
        let size = [image.width() as usize, image.height() as usize];
        let pixels = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
        let texture_id = cache_key.clone();
        cache.insert(
            texture_id.clone(),
            ctx.load_texture(texture_id, pixels, Default::default()),
        );
    }
    cache.get(&cache_key)
}

fn media_path(media_base: &Path, photo: &str) -> PathBuf {
    let raw = Path::new(photo);
    if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        media_base.join(raw)
    }
}

fn thumb_key(person: &Person, size: u32, round: bool) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    person.photo.hash(&mut hasher);
    size.hash(&mut hasher);
    round.hash(&mut hasher);
    if round {
        if let Some(crop) = &person.photo_crop {
            // Rasterung vermeidet beim Ziehen hunderte fast identische Cache-Dateien.
            ((crop.x * 20.0).round() as i32).hash(&mut hasher);
            ((crop.y * 20.0).round() as i32).hash(&mut hasher);
            ((crop.zoom * 20.0).round() as i32).hash(&mut hasher);
        }
    }
    format!("{:016x}", hasher.finish())
}

fn thumb_path(media_base: &Path, key: &str) -> PathBuf {
    media_base.join("media").join(".thumbs").join(key)
}

fn spawn_thumb_job(key: String, ctx: egui::Context, job: impl FnOnce() + Send + 'static) {
    if ACTIVE_THUMBS.load(Ordering::Relaxed) >= MAX_ACTIVE_THUMBS {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        return;
    }
    let pending = PENDING_THUMBS.get_or_init(|| Mutex::new(std::collections::HashSet::new()));
    if !pending
        .lock()
        .ok()
        .is_some_and(|mut set| set.insert(key.clone()))
    {
        return;
    }
    ACTIVE_THUMBS.fetch_add(1, Ordering::Relaxed);
    thread::spawn(move || {
        job();
        ACTIVE_THUMBS.fetch_sub(1, Ordering::Relaxed);
        if let Some(pending) = PENDING_THUMBS.get() {
            if let Ok(mut set) = pending.lock() {
                set.remove(&key);
            }
        }
        ctx.request_repaint();
    });
}

fn resize_short_side(image: image::DynamicImage, short_side: u32) -> RgbaImage {
    let (w, h) = image.dimensions();
    let shortest = w.min(h).max(1);
    if shortest <= short_side {
        return image.to_rgba8();
    }
    let scale = short_side as f32 / shortest as f32;
    let width = (w as f32 * scale).round().max(1.0) as u32;
    let height = (h as f32 * scale).round().max(1.0) as u32;
    image::imageops::resize(&image, width, height, FilterType::Triangle)
}

fn draw_avatar_placeholder(
    ui: &mut egui::Ui,
    person: &Person,
    size: f32,
    sense: Sense,
) -> egui::Response {
    let (response, painter) = ui.allocate_painter(Vec2::splat(size), sense);
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

/// Live-Editor-Avatar: benutzt eine normale Textur und UV-Cropping, damit
/// Ziehen/Zoomen sofort sichtbar ist. Persistente runde PNGs bleiben für
/// Graph/Listen zuständig.
pub fn avatar_ui_live(
    ui: &mut egui::Ui,
    person: &Person,
    cache: &mut HashMap<String, TextureHandle>,
    media_base: &Path,
    size: f32,
) -> egui::Response {
    if let Some(texture) = photo_preview_texture(ui.ctx(), person, cache, media_base) {
        let tv = texture.size_vec2();
        let aspect = tv.x / tv.y.max(1.0);
        ui.add(
            egui::Image::from_texture(texture)
                .fit_to_exact_size(Vec2::splat(size))
                .maintain_aspect_ratio(false)
                .uv(cover_uv(aspect, person.photo_crop.as_ref()))
                .corner_radius(size / 2.0)
                .sense(Sense::click_and_drag()),
        )
    } else {
        draw_avatar_placeholder(ui, person, size, Sense::click_and_drag())
    }
}

fn ensure_gallery_thumb(
    ctx: &egui::Context,
    media_base: &Path,
    person: &Person,
    size: u32,
) -> Option<PathBuf> {
    let key = format!("gallery-{}.png", thumb_key(person, size, false));
    let target = thumb_path(media_base, &key);
    if target.exists() {
        return Some(target);
    }
    let source = media_path(media_base, person.photo.as_deref()?);
    let request_key = format!("job:{key}");
    let target_for_job = target.clone();
    spawn_thumb_job(request_key, ctx.clone(), move || {
        if let Some(parent) = target_for_job.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(image) = image::open(source) {
            let _ = resize_short_side(image, size).save(target_for_job);
        }
    });
    None
}

fn ensure_round_avatar(
    ctx: &egui::Context,
    media_base: &Path,
    person: &Person,
    size: u32,
) -> Option<PathBuf> {
    let key = format!("avatar-{}.png", thumb_key(person, size, true));
    let target = thumb_path(media_base, &key);
    if target.exists() {
        return Some(target);
    }
    let preview = thumb_path(
        media_base,
        &format!("gallery-{}.png", thumb_key(person, 150, false)),
    );
    let source = if preview.exists() {
        preview
    } else {
        media_path(media_base, person.photo.as_deref()?)
    };
    let crop = person.photo_crop;
    let request_key = format!("job:{key}");
    let target_for_job = target.clone();
    spawn_thumb_job(request_key, ctx.clone(), move || {
        if let Ok(image) = image::open(source) {
            let _ = write_round_avatar_image(image, crop, size, &target_for_job);
        }
    });
    None
}

fn write_round_avatar_image(
    image: image::DynamicImage,
    crop: Option<PhotoCrop>,
    size: u32,
    target: &Path,
) -> Option<()> {
    let (w, h) = image.dimensions();
    let aspect = w as f32 / h.max(1) as f32;
    let uv = cover_uv(aspect, crop.as_ref());
    let left = (uv.left().clamp(0.0, 1.0) * w as f32).round() as u32;
    let top = (uv.top().clamp(0.0, 1.0) * h as f32).round() as u32;
    let right = (uv.right().clamp(0.0, 1.0) * w as f32).round() as u32;
    let bottom = (uv.bottom().clamp(0.0, 1.0) * h as f32).round() as u32;
    let crop_w = right.saturating_sub(left).max(1);
    let crop_h = bottom.saturating_sub(top).max(1);
    let cropped = image.crop_imm(left, top, crop_w, crop_h);
    let mut avatar: RgbaImage = image::imageops::resize(&cropped, size, size, FilterType::Triangle);
    let center = (size as f32 - 1.0) / 2.0;
    let radius = size as f32 / 2.0;
    for (x, y, pixel) in avatar.enumerate_pixels_mut() {
        let dx = x as f32 - center;
        let dy = y as f32 - center;
        if (dx * dx + dy * dy).sqrt() > radius {
            pixel.0[3] = 0;
        }
    }
    fs::create_dir_all(target.parent()?).ok()?;
    avatar.save(target).ok()?;
    Some(())
}

/// Avatar sofort aus dem kleinen Galerie-Cache erzeugen. Das ist der schnelle
/// Speichern-Pfad nach Crop-Aenderungen; kein Warten auf Originalbild-Jobs.
pub fn write_round_avatar_now(media_base: &Path, person: &Person) -> Option<()> {
    let avatar_size = 100;
    let target = thumb_path(
        media_base,
        &format!("avatar-{}.png", thumb_key(person, avatar_size, true)),
    );
    let preview = thumb_path(
        media_base,
        &format!("gallery-{}.png", thumb_key(person, 150, false)),
    );
    let source = if preview.exists() {
        preview
    } else {
        media_path(media_base, person.photo.as_deref()?)
    };
    write_round_avatar_image(
        image::open(source).ok()?,
        person.photo_crop,
        avatar_size,
        &target,
    )
}

fn texture_from_file<'a>(
    ctx: &egui::Context,
    cache: &'a mut HashMap<String, TextureHandle>,
    key: String,
    path: PathBuf,
) -> Option<&'a TextureHandle> {
    if !cache.contains_key(&key) {
        let image = image::open(path).ok()?.to_rgba8();
        let size = [image.width() as usize, image.height() as usize];
        let pixels = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
        cache.insert(
            key.clone(),
            ctx.load_texture(key.clone(), pixels, Default::default()),
        );
    }
    cache.get(&key)
}

fn cached_texture_with_prefix<'a>(
    cache: &'a HashMap<String, TextureHandle>,
    prefix: &str,
) -> Option<&'a TextureHandle> {
    cache
        .iter()
        .find_map(|(key, texture)| key.starts_with(prefix).then_some(texture))
}

/// Vorschau-Textur für kleine Listen/Thumbnails.
pub fn photo_preview_texture<'a>(
    ctx: &egui::Context,
    person: &Person,
    cache: &'a mut HashMap<String, TextureHandle>,
    media_base: &Path,
) -> Option<&'a TextureHandle> {
    let thumb_size = 150;
    let prefix = format!("preview:{}:", person.id);
    let path = match ensure_gallery_thumb(ctx, media_base, person, thumb_size) {
        Some(path) => path,
        None => return cached_texture_with_prefix(cache, &prefix),
    };
    texture_from_file(
        ctx,
        cache,
        format!(
            "preview:{}:{}",
            person.id,
            thumb_key(person, thumb_size, false)
        ),
        path,
    )
}

/// Zustand der Lightbox-Textur während des asynchronen Ladens.
pub enum LightboxState<'a> {
    /// Vollbild-Textur ist bereit.
    Ready(&'a TextureHandle),
    /// Noch nicht fertig; `preview` (klein, sofort) anzeigen wo verfügbar.
    Loading,
}

/// Ergebnis eines asynchronen Lightbox-Dekodiervorgangs.
pub struct AsyncImage {
    /// Cache-Schlüssel (`lightbox-<id>`).
    pub key: String,
    /// Dekodierte Pixel (volles Bild, unverkleinert).
    pub image: egui::ColorImage,
}

/// Fragt die vorbereitete Vorschau-Textur ab (blockiert nie) und stellt einen
/// asynchronen Vollbild-Dekodierauftrag; Ergebnis landet über den shared
/// Sender `tx`. Liefert `Ready`, sobald der Vollbild-Cache gefüllt ist.
pub fn lightbox_texture_async<'a>(
    ctx: &egui::Context,
    person: &Person,
    cache: &'a mut HashMap<String, TextureHandle>,
    media_base: &Path,
    loading: &mut std::collections::HashSet<String>,
    tx: &mpsc::Sender<AsyncImage>,
) -> LightboxState<'a> {
    let cache_key = format!("lightbox-{}", person.id);
    if cache.contains_key(&cache_key) {
        return LightboxState::Ready(cache.get(&cache_key).unwrap());
    }
    // Sofort (nicht blockierend) das kleine Vorschaubild anzeigen.
    photo_preview_texture(ctx, person, cache, media_base);
    // Noch kein Job angelaufen → Vollbild im Hintergrund dekodieren.
    if !loading.contains(&cache_key) {
        let Some(photo) = person.photo.as_deref() else {
            return LightboxState::Loading;
        };
        let path = media_path(media_base, photo);
        if let Some(guard) = load_guard() {
            loading.insert(cache_key.clone());
            let tx = tx.clone();
            let key = cache_key.clone();
            let ctx_clone = ctx.clone();
            thread::spawn(move || {
                let _guard = guard;
                if let Ok(image) = image::open(&path) {
                    // Auf max 1200px herunterskalieren, um GPU-Upload und Speicher extrem zu beschleunigen.
                    let resized = image.thumbnail(1200, 1200);
                    let size = [resized.width() as usize, resized.height() as usize];
                    let rgba = resized.to_rgba8();
                    let ci = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                    let _ = tx.send(AsyncImage { key, image: ci });
                }
                ctx_clone.request_repaint();
            });
        }
    }
    LightboxState::Loading
}

/// Liefert die Bereitschaft der Vollbild-Textur (ob schon gecacht) zurück.
#[allow(dead_code)]
pub fn lightbox_ready<'a>(
    person: &Person,
    cache: &'a HashMap<String, TextureHandle>,
) -> Option<&'a TextureHandle> {
    cache.get(&format!("lightbox-{}", person.id))
}

/// Alle eingegangenen asynchronen Bilder in den Textur-Cache übernehmen.
pub fn drain_lightbox_textures(
    rx: &mpsc::Receiver<AsyncImage>,
    cache: &mut HashMap<String, TextureHandle>,
    ctx: &egui::Context,
    loading: &mut std::collections::HashSet<String>,
) {
    while let Ok(AsyncImage { key, image }) = rx.try_recv() {
        loading.remove(&key);
        cache.insert(
            key.clone(),
            ctx.load_texture(key, image, Default::default()),
        );
    }
}

/// Sperre: begrenzt parallele asynchrone Bild-Dekodierungen auf
/// `MAX_ACTIVE_THUMBS`. Gibt `None`, wenn bereits genug laufen.
fn load_guard() -> Option<LoadGuard> {
    let active = ACTIVE_THUMBS.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |a| {
        (a < MAX_ACTIVE_THUMBS).then(|| a + 1)
    });
    match active {
        Ok(_) => Some(LoadGuard),
        Err(_) => None,
    }
}

struct LoadGuard;

impl Drop for LoadGuard {
    fn drop(&mut self) {
        ACTIVE_THUMBS.fetch_sub(1, Ordering::Relaxed);
    }
}

pub fn round_avatar_texture<'a>(
    ctx: &egui::Context,
    person: &Person,
    cache: &'a mut HashMap<String, TextureHandle>,
    media_base: &Path,
) -> Option<&'a TextureHandle> {
    let avatar_size = 100;
    let prefix = format!("avatar:{}:", person.id);
    let path = match ensure_round_avatar(ctx, media_base, person, avatar_size) {
        Some(path) => path,
        None => return cached_texture_with_prefix(cache, &prefix),
    };
    texture_from_file(
        ctx,
        cache,
        format!(
            "avatar:{}:{}",
            person.id,
            thumb_key(person, avatar_size, true)
        ),
        path,
    )
}

/// Nur vorhandenes rundes Avatar-Thumbnail laden. Wichtig für den Graphen:
/// dort dürfen beim Zeichnen vieler Personen keine neuen Bildjobs entstehen.
pub fn round_avatar_texture_cached<'a>(
    ctx: &egui::Context,
    person: &Person,
    cache: &'a mut HashMap<String, TextureHandle>,
    media_base: &Path,
) -> Option<&'a TextureHandle> {
    let avatar_size = 100;
    let key = thumb_key(person, avatar_size, true);
    let texture_key = format!("avatar:{}:{key}", person.id);
    if cache.contains_key(&texture_key) {
        return cache.get(&texture_key);
    }
    let path = thumb_path(media_base, &format!("avatar-{key}.png"));
    path.exists()
        .then_some(path)
        .and_then(|path| texture_from_file(ctx, cache, texture_key, path))
}

/// Rechteckige Galerie-Vorschau aus dem Vollbild. Klick öffnet die Lightbox.
pub fn gallery_thumbnail_ui(
    ui: &mut egui::Ui,
    person: &Person,
    cache: &mut HashMap<String, TextureHandle>,
    media_base: &Path,
    size: Vec2,
) -> egui::Response {
    if let Some(texture) = photo_preview_texture(ui.ctx(), person, cache, media_base) {
        let tv = texture.size_vec2();
        let aspect = tv.x / tv.y.max(1.0);
        let target_aspect = size.x / size.y.max(1.0);
        ui.add(
            egui::Image::from_texture(texture)
                .fit_to_exact_size(size)
                .maintain_aspect_ratio(false)
                .uv(cover_uv_to(aspect, target_aspect, None))
                .corner_radius(5.0)
                .sense(Sense::click()),
        )
    } else {
        ui.allocate_response(size, Sense::click())
    }
}

/// Initialen für den Platzhalter-Avatar (maximal 2 Buchstaben).
pub fn initials(person: &Person) -> String {
    person
        .display_name()
        .split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(2)
        .collect::<String>()
        .to_uppercase()
}

/// Avatar-Vorschau: runder Avatar aus Cache oder Live-Vorschau.
pub fn avatar_ui_preview(
    ui: &mut egui::Ui,
    person: &Person,
    cache: &mut HashMap<String, TextureHandle>,
    media_base: &Path,
    size: f32,
) -> egui::Response {
    if let Some(texture) = round_avatar_texture(ui.ctx(), person, cache, media_base) {
        let (response, painter) = ui.allocate_painter(Vec2::splat(size), Sense::click_and_drag());
        painter.circle_filled(
            response.rect.center(),
            size / 2.0,
            Color32::from_black_alpha(24),
        );
        painter.image(
            texture.id(),
            response.rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        painter.circle_stroke(
            response.rect.center(),
            size / 2.0,
            egui::Stroke::new(1.0, Color32::from_white_alpha(40)),
        );
        response
    } else {
        avatar_ui_live(ui, person, cache, media_base, size)
    }
}

/// Alle Texturen einer Person aus dem Cache entfernen.
pub fn clear_person_photo_cache(cache: &mut HashMap<String, TextureHandle>, person_id: &str) {
    cache.retain(|key, _| {
        key != person_id
            && !key.starts_with(&format!("preview:{person_id}:"))
            && !key.starts_with(&format!("avatar:{person_id}:"))
            && !key.starts_with(&format!("gallery-{person_id}-"))
    });
}

/// Entfernt ungenutzte Lightbox-Vollbilder aus dem Cache, um GPU-Speicher freizugeben.
pub fn clear_lightbox_cache(cache: &mut HashMap<String, TextureHandle>, keep_path: Option<&str>) {
    let keep_key = keep_path.map(|path| format!("lightbox-{path}"));
    cache.retain(|key, _| {
        if key.starts_with("lightbox-") {
            if let Some(ref keep) = keep_key {
                key == keep
            } else {
                false
            }
        } else {
            true
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_photo_into_media_folder() {
        let temp = std::env::temp_dir().join("minigramps-media-test");
        let _ = fs::remove_dir_all(&temp);
        let source = temp.join("quelle.png");
        fs::create_dir_all(&temp).unwrap();
        fs::write(&source, b"pngdata").unwrap();
        let relative = import_media_file(&temp, &source).unwrap();
        assert!(relative.starts_with("media/"));
        assert!(temp.join(&relative).exists());
        // Erneutes Importieren derselben Daten liefert denselben Pfad.
        assert_eq!(import_media_file(&temp, &source).unwrap(), relative);
        let _ = fs::remove_dir_all(&temp);
    }
}
