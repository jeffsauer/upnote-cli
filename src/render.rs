use chrono::DateTime;
use glamour::{Style, TermRenderer};

use crate::convert::ImageRef;
use crate::images::{ImageCtx, ImagePayload, resolve};
use crate::model::{Catalog, FileMeta, Note, Notebook, Stats, Tag};

pub enum Theme {
    Dark,
    Light,
    Pink,
    Dracula,
    TokyoNight,
    Ascii,
}

impl Theme {
    pub fn style(&self) -> Style {
        match self {
            Theme::Dark => Style::Dark,
            Theme::Light => Style::Light,
            Theme::Pink => Style::Pink,
            Theme::Dracula => Style::Dracula,
            Theme::TokyoNight => Style::TokyoNight,
            Theme::Ascii => Style::Ascii,
        }
    }
}

pub struct RenderCfg {
    pub width: usize,
    pub theme: Theme,
    pub color: bool,
    pub images: ImageCtx,
}

impl RenderCfg {
    fn renderer(&self) -> TermRenderer {
        let mut r = TermRenderer::new().with_word_wrap(self.width);
        if self.color {
            r = r.with_style(self.theme.style());
        } else {
            r = r.with_style(Style::NoTty);
        }
        r
    }

    pub fn render_markdown(&self, md: &str) -> String {
        self.renderer().render(md)
    }

    pub fn render_note_body(&self, md: &str, images: &[ImageRef]) -> String {
        let mut out = self.render_markdown(md);
        for img in images {
            let repl = match resolve(&img.src, &img.alt, &self.images) {
                ImagePayload::Inline(s) => s,
                ImagePayload::Text(t) => t,
            };
            out = out.replace(&img.sentinel, &repl);
        }
        out
    }
}

fn sgr(code: &str, text: &str) -> String {
    format!("\x1b[{code}m{text}\x1b[0m")
}

fn c(cfg: &RenderCfg, code: &str, text: &str) -> String {
    if cfg.color {
        sgr(code, text)
    } else {
        text.to_string()
    }
}

fn fmt_ts(ms: Option<i64>) -> String {
    match ms.and_then(DateTime::from_timestamp_millis) {
        Some(dt) => dt
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        None => "-".to_string(),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{cut}…")
}

fn badges(note: &Note) -> Vec<&'static str> {
    let mut v = Vec::new();
    if note.pinned {
        v.push("pinned");
    }
    if note.bookmarked {
        v.push("bookmarked");
    }
    if note.is_template {
        v.push("template");
    }
    if note.trashed {
        v.push("trash");
    }
    if note.shared {
        v.push("shared");
    }
    v
}

fn badge_str(b: &str) -> &'static str {
    match b {
        "pinned" => "1;36",
        "bookmarked" => "1;33",
        "template" => "1;35",
        "trash" => "1;31",
        "shared" => "1;34",
        _ => "2",
    }
}

pub fn note_header(note: &Note, cat: &Catalog, files: &[FileMeta], cfg: &RenderCfg) -> String {
    let mut out = String::new();
    let title = note.title_or_placeholder();
    out.push_str(&c(cfg, "1;4", title));
    out.push('\n');
    let bar = "─".repeat(cfg.width.clamp(10, 100));
    out.push_str(&c(cfg, "8", &bar));
    out.push('\n');
    let mut line1 = String::new();
    line1.push_str(&c(
        cfg,
        "2",
        &format!("created {}   ", fmt_ts(note.created_at)),
    ));
    line1.push_str(&c(
        cfg,
        "2",
        &format!("updated {}   ", fmt_ts(note.updated_at)),
    ));
    if !note.notebook_ids.is_empty() {
        let nbs: Vec<String> = note
            .notebook_ids
            .iter()
            .filter_map(|id| cat.notebook_title(id))
            .map(|t| t.to_string())
            .collect();
        if !nbs.is_empty() {
            line1.push_str(&c(cfg, "2", "notebook: "));
            line1.push_str(&c(cfg, "3", &nbs.join(", ")));
        }
    }
    out.push_str(&line1);
    out.push('\n');
    let mut line2 = String::new();
    if !note.tag_links.is_empty() {
        let tags: Vec<String> = note
            .tag_links
            .iter()
            .filter_map(|l| cat.tag_title(l))
            .map(|t| t.to_string())
            .collect();
        if !tags.is_empty() {
            line2.push_str(&c(cfg, "2", "tags: "));
            line2.push_str(&c(cfg, "3", &tags.join(", ")));
        }
    }
    for b in badges(note) {
        line2.push_str(&c(cfg, badge_str(b), &format!("[{b}]")));
        line2.push(' ');
    }
    if !line2.is_empty() {
        out.push_str(&line2);
        out.push('\n');
    }
    if !files.is_empty() {
        out.push_str(&c(cfg, "2", "files: "));
        let names: Vec<String> = files.iter().map(|f| f.name.clone()).collect();
        out.push_str(&c(cfg, "3", &names.join(", ")));
        out.push('\n');
        for f in files {
            let local = f.id.split("__").next().unwrap_or(&f.id);
            if let Some(url) = &f.download_url {
                out.push_str(&c(
                    cfg,
                    "2",
                    &format!("  {}  →  {}", f.name, truncate(url, cfg.width - 12)),
                ));
                out.push('\n');
            } else {
                out.push_str(&c(cfg, "2", &format!("  {local}")));
                out.push('\n');
            }
        }
    }
    out
}

fn note_row(n: &Note, cat: &Catalog, cfg: &RenderCfg, prefix: Option<&str>) -> String {
    let pad = prefix.map(|p| p.chars().count()).unwrap_or(0);
    let mut out = String::new();
    if let Some(p) = prefix {
        out.push_str(&c(cfg, "2", p));
    }
    out.push_str(&c(
        cfg,
        "1",
        &n.title
            .chars()
            .take((cfg.width / 2).saturating_sub(pad))
            .collect::<String>(),
    ));
    if !n.title.is_empty() {
        out.push(' ');
    }
    out.push_str(&c(cfg, "2", &format!("· {}", fmt_ts(n.updated_at))));
    if let Some(nb) = n.notebook_ids.iter().find_map(|id| cat.notebook_title(id)) {
        out.push_str(&c(cfg, "2", &format!(" · {nb}")));
    }
    for b in badges(n) {
        out.push_str(&c(cfg, badge_str(b), &format!(" [{b}]")));
    }
    out.push('\n');
    if !n.summary.is_empty() {
        let summary = n.summary.lines().next().unwrap_or("").trim();
        if !summary.is_empty() {
            let indent = " ".repeat(if pad == 0 { 4 } else { pad });
            out.push_str(&c(
                cfg,
                "2",
                &format!(
                    "{indent}{}",
                    truncate(summary, cfg.width.saturating_sub(6 + pad))
                ),
            ));
            out.push('\n');
        }
    }
    out.push('\n');
    out
}

pub fn list_notes(notes: &[Note], cat: &Catalog, cfg: &RenderCfg) -> String {
    notes.iter().map(|n| note_row(n, cat, cfg, None)).collect()
}

pub fn list_notes_numbered(notes: &[Note], cat: &Catalog, cfg: &RenderCfg) -> String {
    let w = notes.len().to_string().len();
    notes
        .iter()
        .enumerate()
        .map(|(i, n)| note_row(n, cat, cfg, Some(&format!("{:>w$}. ", i + 1, w = w))))
        .collect()
}

pub fn count_line(shown: usize, total: i64, label: &str, cfg: &RenderCfg) -> String {
    if shown as i64 == total {
        c(cfg, "2", &format!("{total} {label}"))
    } else {
        c(cfg, "2", &format!("showing {shown} of {total} {label}"))
    }
}

fn render_tree_node(
    n: &Notebook,
    prefix: &str,
    last: bool,
    children: &std::collections::HashMap<String, Vec<&Notebook>>,
    counts: &std::collections::HashMap<String, i64>,
    cfg: &RenderCfg,
    out: &mut String,
) {
    let (branch, child_prefix) = if last {
        ("└── ", "    ")
    } else {
        ("├── ", "│   ")
    };
    out.push_str(prefix);
    out.push_str(&c(cfg, "2", branch));
    let locked = if n.locked { " [locked]" } else { "" };
    out.push_str(&c(cfg, "1;4", &n.title));
    if !locked.is_empty() {
        out.push_str(&c(cfg, "33", locked));
    }
    if let Some(cnt) = counts.get(&n.id)
        && *cnt > 0
    {
        out.push_str(&c(cfg, "2", &format!("  ({cnt})")));
    }
    out.push('\n');
    if let Some(kids) = children.get(&n.id) {
        for (i, k) in kids.iter().enumerate() {
            render_tree_node(
                k,
                &format!("{prefix}{child_prefix}"),
                i == kids.len() - 1,
                children,
                counts,
                cfg,
                out,
            );
        }
    }
}

pub fn notebook_tree(
    notebooks: &[Notebook],
    counts: &std::collections::HashMap<String, i64>,
    cfg: &RenderCfg,
) -> String {
    let ids: std::collections::HashSet<&str> = notebooks.iter().map(|n| n.id.as_str()).collect();
    let mut children: std::collections::HashMap<String, Vec<&Notebook>> =
        std::collections::HashMap::new();
    let mut roots: Vec<&Notebook> = Vec::new();
    for n in notebooks {
        match &n.parent {
            Some(p) if ids.contains(p.as_str()) => {
                children.entry(p.clone()).or_default().push(n);
            }
            _ => roots.push(n),
        }
    }
    let sort_by = |v: &mut Vec<&Notebook>| {
        v.sort_by(|a, b| a.title.cmp(&b.title).then(a.id.cmp(&b.id)));
    };
    sort_by(&mut roots);
    for v in children.values_mut() {
        sort_by(v);
    }
    let mut out = String::new();
    for (i, r) in roots.iter().enumerate() {
        render_tree_node(
            r,
            "",
            i == roots.len() - 1,
            &children,
            counts,
            cfg,
            &mut out,
        );
    }
    out
}

pub fn tags_list(tags: &[(Tag, i64)], cfg: &RenderCfg) -> String {
    let mut out = String::new();
    for (t, count) in tags {
        out.push_str(&c(cfg, "1;4", &t.title));
        out.push_str(&c(cfg, "2", &format!("  ({count})")));
        out.push('\n');
    }
    out
}

pub fn stats_view(s: &Stats, cfg: &RenderCfg) -> String {
    let row = |k: &str, v: &str| -> String {
        format!("{}  {}", c(cfg, "2", &format!("{k:<20}")), c(cfg, "1", v))
    };
    let mut out = String::new();
    out.push_str(&row("notes", &s.notes.to_string()));
    out.push('\n');
    out.push_str(&row("trash", &s.trashed.to_string()));
    out.push('\n');
    out.push_str(&row("pinned", &s.pinned.to_string()));
    out.push('\n');
    out.push_str(&row("bookmarked", &s.bookmarked.to_string()));
    out.push('\n');
    out.push_str(&row("templates", &s.templates.to_string()));
    out.push('\n');
    out.push_str(&row("notebooks", &s.notebooks.to_string()));
    out.push('\n');
    out.push_str(&row("tags", &s.tags.to_string()));
    out.push('\n');
    out.push_str(&row("files (tracked)", &s.files.to_string()));
    out.push('\n');
    out.push_str(&row("cached images", &s.cached_images.to_string()));
    out.push('\n');
    out.push_str(&row("database size", &format_human(s.db_size)));
    out.push('\n');
    out
}

fn format_human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} {}", UNITS[0])
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}
