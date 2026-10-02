use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};

pub struct ImageCtx {
    pub enabled: bool,
    pub kitty: bool,
    pub width_cells: usize,
    pub color: bool,
    pub images_dir: PathBuf,
    /// Newest `UpNote Backup/*/files` dir when the "Backup attachments"
    /// preference is enabled. Backup folders are read-only.
    pub backup_files_dir: Option<PathBuf>,
}

pub enum ImagePayload {
    Inline(String),
    Text(String),
}

pub fn is_image_name(name: &str) -> bool {
    matches!(
        ext(name).as_deref(),
        Some("png") | Some("jpg") | Some("jpeg") | Some("gif") | Some("webp")
    )
}

pub fn ext(name: &str) -> Option<String> {
    name.rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .filter(|e| !e.is_empty() && e.len() <= 5)
}

/// Returns the image bytes in a payload format kitty accepts: PNG passes
/// through untouched, anything else is decoded and re-encoded as PNG (the
/// kitty graphics protocol only understands f=24 RGB, f=32 RGBA and f=100
/// PNG payloads).
fn to_png(path: &Path) -> Result<Vec<u8>> {
    let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok(data);
    }
    let img = image::load_from_memory(&data)
        .with_context(|| format!("decoding image {}", path.display()))?;
    let rgba = img.to_rgba8();
    use image::ImageEncoder;
    let mut out = Vec::new();
    let enc = image::codecs::png::PngEncoder::new(&mut out);
    enc.write_image(
        rgba.as_raw(),
        rgba.width(),
        rgba.height(),
        image::ExtendedColorType::Rgba8,
    )
    .context("encoding PNG")?;
    Ok(out)
}

pub fn terminal_is_kitty() -> bool {
    std::env::var_os("KITTY_WINDOW_ID").is_some()
        || matches!(
            std::env::var("TERM_PROGRAM").as_deref(),
            Ok("ghostty" | "Ghostty" | "WezTerm" | "wezterm")
        )
        || std::env::var("TERM")
            .map(|t| t.to_lowercase().contains("kitty"))
            .unwrap_or(false)
}

fn file_base(src: &str) -> &str {
    src.rsplit(['/', '\\']).next().unwrap_or(src)
}

/// When the "Backup attachments" preference is enabled in `renderer.json`,
/// locate the newest `UpNote Backup/*/files` directory (by directory mtime).
/// Backup folders are treated as read-only. Returns `None` when the
/// preference is off or no backup files directory exists.
pub fn backup_files_dir(config_dir: &Path) -> Option<PathBuf> {
    let raw = std::fs::read_to_string(config_dir.join("renderer.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    if !json.get("SHOULD_BACKUP_ATTACHMENTS")?.as_bool()? {
        return None;
    }
    let base = config_dir.join("UpNote Backup");
    let mut latest: Option<(SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(&base).ok()?.flatten() {
        let files = entry.path().join("files");
        if !files.is_dir() {
            continue;
        }
        let mtime = files
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        if latest.as_ref().is_none_or(|(t, _)| mtime > *t) {
            latest = Some((mtime, files));
        }
    }
    latest.map(|(_, p)| p)
}

fn placeholder(label: &str, detail: &str, color: bool) -> String {
    let text = if detail.is_empty() {
        format!("[{label}]")
    } else {
        format!("[{label}] {detail}")
    };
    if color {
        format!("\x1b[2m{text}\x1b[0m")
    } else {
        text
    }
}

fn kitty_escape(png: &[u8], width_px: u32, width_cells: usize) -> String {
    let mut params = String::from("a=T,f=100");
    if width_px as usize > width_cells.saturating_mul(9) {
        params.push_str(&format!(",s={width_cells}c"));
    }
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(png);
    let total = b64.len();
    const CHUNK: usize = 4096;
    let mut out = String::with_capacity(total + total.div_ceil(CHUNK) * 48 + 64);
    let mut pos = 0;
    let mut first = true;
    while pos < total {
        let end = (pos + CHUNK).min(total);
        let last = end == total;
        out.push('\u{1b}');
        out.push_str("_G");
        if first {
            out.push_str(&params);
            out.push(',');
            first = false;
        }
        if last {
            out.push_str("m=0;");
        } else {
            out.push_str("m=1;");
        }
        out.push_str(&b64[pos..end]);
        out.push('\u{1b}');
        out.push('\\');
        pos = end;
    }
    out
}

fn render_local(path: &Path, ctx: &ImageCtx, display: &str) -> ImagePayload {
    if !ctx.kitty {
        return ImagePayload::Text(placeholder(
            "image",
            &format!("{display} — {}", path.display()),
            ctx.color,
        ));
    }
    let Ok(png) = to_png(path) else {
        return ImagePayload::Text(placeholder("image", display, ctx.color));
    };
    let width_px = image::ImageReader::new(std::io::Cursor::new(&png))
        .into_dimensions()
        .map(|(w, _)| w)
        .unwrap_or(0);
    ImagePayload::Inline(kitty_escape(&png, width_px, ctx.width_cells))
}

pub fn resolve(src: &str, alt: &str, ctx: &ImageCtx) -> ImagePayload {
    let base = file_base(src);
    let display = if alt.is_empty() {
        base.to_string()
    } else {
        alt.to_string()
    };
    if !ctx.enabled {
        return ImagePayload::Text(placeholder("image", &display, ctx.color));
    }
    let local = if Path::new(src).is_absolute() {
        PathBuf::from(src)
    } else {
        ctx.images_dir.join(base)
    };
    if local.is_file() {
        return render_local(&local, ctx, &display);
    }
    // Fall back to the newest backup attachments directory (read-only) when
    // the "Backup attachments" preference is enabled.
    if let Some(dir) = &ctx.backup_files_dir {
        let backed_up = dir.join(base);
        if backed_up.is_file() {
            return render_local(&backed_up, ctx, &display);
        }
    }
    // UpNote's http://localhost:9425/ images are not fetchable by this tool:
    // the app resolves those URLs in-process and only caches the image locally
    // after it has downloaded it itself.
    if src.starts_with("http://localhost:9425/") {
        return ImagePayload::Text(placeholder(
            "image not cached",
            &format!("{display} — open this note in the UpNote app to download it"),
            ctx.color,
        ));
    }
    ImagePayload::Text(placeholder("image not cached", &display, ctx.color))
}
