mod convert;
mod db;
mod images;
mod model;
mod render;

use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use db::{Db, ListMode};
use render::{RenderCfg, Theme};

#[derive(Parser)]
#[command(
    name = "upnote-cli",
    version,
    about = "Query the local UpNote database and render notes as markdown in the terminal"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    #[arg(
        long,
        global = true,
        value_name = "PATH",
        help = "Path to upnote.sqlite3 (default ~/.config/UpNote/upnote.sqlite3)"
    )]
    db: Option<PathBuf>,

    #[arg(long, global = true, value_enum, default_value_t = ThemeArg::Dark, help = "Color theme")]
    theme: ThemeArg,

    #[arg(long, global = true, help = "Plain output: no color, no inline images")]
    plain: bool,

    #[arg(long, global = true, help = "Disable inline images")]
    no_images: bool,

    #[arg(
        long,
        global = true,
        value_name = "COLS",
        help = "Terminal width for wrapping"
    )]
    width: Option<usize>,

    #[arg(
        long,
        global = true,
        default_value_t = 50,
        value_name = "N",
        help = "Max notes shown in listings"
    )]
    limit: usize,
}

#[derive(Subcommand)]
enum Command {
    /// Search notes by title, summary, text, or content
    Search {
        #[arg(required = true)]
        query: Vec<String>,
    },
    /// Show a single note (by id or title)
    Note {
        /// Note id or (partial) title
        target: String,
    },
    /// Print the notebook tree
    Notebooks,
    /// List notes in a notebook (by id or name)
    Notebook {
        /// Notebook id or (partial) name
        target: String,
        #[arg(long, help = "Include notes from sub-notebooks")]
        recursive: bool,
    },
    /// List all tags with note counts
    Tags,
    /// List notes in a tag (by name)
    Tag {
        /// Tag name (with or without leading #)
        target: String,
    },
    /// List notes by category
    List {
        #[arg(value_enum, default_value_t = ModeArg::All)]
        mode: ModeArg,
    },
    /// Database statistics
    Stats,
}

#[derive(Clone, Copy, ValueEnum)]
enum ModeArg {
    All,
    Trash,
    Pinned,
    Bookmarked,
    Templates,
}

impl From<ModeArg> for ListMode {
    fn from(m: ModeArg) -> Self {
        match m {
            ModeArg::All => ListMode::All,
            ModeArg::Trash => ListMode::Trash,
            ModeArg::Pinned => ListMode::Pinned,
            ModeArg::Bookmarked => ListMode::Bookmarked,
            ModeArg::Templates => ListMode::Templates,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum ThemeArg {
    Dark,
    Light,
    Pink,
    Dracula,
    TokyoNight,
    Ascii,
}

impl From<ThemeArg> for Theme {
    fn from(t: ThemeArg) -> Self {
        match t {
            ThemeArg::Dark => Theme::Dark,
            ThemeArg::Light => Theme::Light,
            ThemeArg::Pink => Theme::Pink,
            ThemeArg::Dracula => Theme::Dracula,
            ThemeArg::TokyoNight => Theme::TokyoNight,
            ThemeArg::Ascii => Theme::Ascii,
        }
    }
}

static SALT: AtomicU64 = AtomicU64::new(0);

fn next_salt() -> u64 {
    let mut cur = SALT.load(Ordering::Relaxed);
    if cur == 0 {
        cur = salt_seed();
        SALT.store(cur, Ordering::Relaxed);
    }
    SALT.fetch_add(1, Ordering::Relaxed)
}

fn salt_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9e3779b97f4a7c15)
}

struct Env {
    db_path: PathBuf,
    images_dir: PathBuf,
    cfg: RenderCfg,
}

fn build_env(cli: &Cli) -> Result<Env> {
    let db_path = match &cli.db {
        Some(p) => p.clone(),
        None => dirs::home_dir()
            .context("cannot determine home directory")?
            .join(".config/UpNote/upnote.sqlite3"),
    };
    let config_dir = db_path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let images_dir = config_dir.join("images");
    let tty = std::io::stdout().is_terminal();
    let no_color_env = std::env::var_os("NO_COLOR").is_some();
    let color = tty && !cli.plain && !no_color_env;
    let width = match cli.width {
        Some(w) => w.clamp(40, 300),
        None => terminal_size::terminal_size()
            .map(|(w, _)| w.0 as usize)
            .unwrap_or(80)
            .clamp(40, 300),
    };
    let kitty = color && !cli.no_images && tty && images::terminal_is_kitty();
    let cfg = RenderCfg {
        width,
        theme: Theme::from(cli.theme),
        color,
        images: images::ImageCtx {
            enabled: kitty,
            kitty,
            width_cells: width,
            color,
            images_dir: images_dir.clone(),
            backup_files_dir: images::backup_files_dir(&config_dir),
        },
    };
    Ok(Env {
        db_path,
        images_dir,
        cfg,
    })
}

fn strip_dup_title(md: &str, title: &str) -> String {
    if title.is_empty() {
        return md.to_string();
    }
    let mut lines = md.split_inclusive('\n');
    let first = lines.next().unwrap_or("");
    let trimmed = first.trim_start();
    if let Some(after) = trimmed.strip_prefix('#') {
        let text = after.trim_start().trim().trim_end_matches('#').trim();
        if text.eq_ignore_ascii_case(title) {
            return lines.collect();
        }
    }
    md.to_string()
}

fn render_full_note(db: &Db, note: &model::Note, env: &Env) -> String {
    let cat = db.catalog().unwrap_or_default();
    let files = db.files_for(&note.file_ids).unwrap_or_default();
    let salt = next_salt();
    let converted = convert::convert(&note.html, &note.text, &env.images_dir, salt);
    let md = strip_dup_title(&converted.markdown, &note.title);
    let mut out = render::note_header(note, &cat, &files, &env.cfg);
    out.push('\n');
    out.push_str(&env.cfg.render_note_body(&md, &converted.images));
    out
}

fn run_search(db: &Db, query: &[String], env: &Env, limit: usize) -> Result<()> {
    let q = query.join(" ");
    let (mut notes, total) = db.search(&q, limit)?;
    if total == 0 {
        println!("no notes match \"{}\"", q);
        return Ok(());
    }
    notes.sort_by(|a, b| {
        a.title
            .to_lowercase()
            .cmp(&b.title.to_lowercase())
            .then_with(|| a.title.cmp(&b.title))
            .then_with(|| a.id.cmp(&b.id))
    });
    if notes.len() == 1 {
        println!("{}", render_full_note(db, &notes[0], env));
        return Ok(());
    }
    let cat = db.catalog().unwrap_or_default();
    let label = format!("notes matching \"{}\"", q);
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    println!(
        "{}",
        render::count_line(notes.len(), total, &label, &env.cfg)
    );
    if interactive {
        println!("{}", render::list_notes_numbered(&notes, &cat, &env.cfg));
        if let Some(idx) = prompt_index(notes.len(), env) {
            println!("{}", render_full_note(db, &notes[idx], env));
        }
    } else {
        println!("{}", render::list_notes(&notes, &cat, &env.cfg));
    }
    Ok(())
}

fn prompt_index(count: usize, env: &Env) -> Option<usize> {
    use std::io::Write;
    let prompt = format!("Enter a number (1-{count}) to view, or press Enter to cancel: ");
    loop {
        if env.cfg.color {
            print!("\x1b[2m{prompt}\x1b[0m");
        } else {
            print!("{prompt}");
        }
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(0) | Err(_) => return None,
            Ok(_) => {
                let t = line.trim();
                if t.is_empty() {
                    return None;
                }
                match t.parse::<usize>() {
                    Ok(n) if (1..=count).contains(&n) => return Some(n - 1),
                    _ => eprintln!("Please enter a number between 1 and {count}."),
                }
            }
        }
    }
}

fn run_note(db: &Db, target: &str, env: &Env) -> Result<()> {
    let note = db.find_note(target)?;
    println!("{}", render_full_note(db, &note, env));
    Ok(())
}

fn run_notebooks(db: &Db, env: &Env) -> Result<()> {
    let notebooks = db.notebooks()?;
    let mut counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    if let Ok(orgs) = db.notebook_map() {
        for (_note, nbs) in orgs {
            for nb in nbs {
                *counts.entry(nb).or_insert(0) += 1;
            }
        }
    }
    let mut counts2: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for nb in &notebooks {
        let subtree = db.notebook_descendants(&nb.id)?;
        let total: i64 = subtree
            .iter()
            .map(|id| counts.get(id).copied().unwrap_or(0))
            .sum();
        counts2.insert(nb.id.clone(), total);
    }
    println!("{}", render::notebook_tree(&notebooks, &counts2, &env.cfg));
    Ok(())
}

fn run_notebook(db: &Db, target: &str, recursive: bool, env: &Env, limit: usize) -> Result<()> {
    let nb = db.find_notebook(target)?;
    let ids = if recursive {
        db.notebook_descendants(&nb.id)?
    } else {
        vec![nb.id.clone()]
    };
    let (notes, total) = db.notes_in_notebooks(&ids, limit)?;
    if notes.is_empty() {
        println!("notebook \"{}\" has no notes", nb.title);
        return Ok(());
    }
    let label = if recursive {
        format!("notes in \"{}\" (incl. sub-notebooks)", nb.title)
    } else {
        format!("notes in \"{}\"", nb.title)
    };
    println!(
        "{}",
        render::count_line(notes.len(), total, &label, &env.cfg)
    );
    println!(
        "{}",
        render::list_notes(&notes, &db.catalog().unwrap_or_default(), &env.cfg)
    );
    Ok(())
}

fn run_tags(db: &Db, env: &Env) -> Result<()> {
    let tags = db.tags()?;
    let counts = db.tag_link_counts()?;
    let mut pairs: Vec<(model::Tag, i64)> = tags
        .into_iter()
        .map(|t| {
            let link = t.title.trim_start_matches('#').to_lowercase();
            let c = counts.get(&link).copied().unwrap_or(0);
            (t, c)
        })
        .collect();
    pairs.sort_by(|a, b| a.1.cmp(&b.1).reverse().then(a.0.title.cmp(&b.0.title)));
    println!("{}", render::tags_list(&pairs, &env.cfg));
    Ok(())
}

fn run_tag(db: &Db, target: &str, env: &Env, limit: usize) -> Result<()> {
    let tags = db.tags()?;
    let want = target.trim_start_matches('#').to_lowercase();
    let tag = tags
        .iter()
        .find(|t| t.title.trim_start_matches('#').to_lowercase() == want)
        .cloned();
    let tag = match tag {
        Some(t) => t,
        None => {
            let cands: Vec<&model::Tag> = tags
                .iter()
                .filter(|t| {
                    t.title.to_lowercase().contains(&want)
                        || t.title
                            .trim_start_matches('#')
                            .to_lowercase()
                            .contains(&want)
                })
                .collect();
            match cands.len() {
                0 => anyhow::bail!("no tag found matching \"{}\"", target),
                1 => cands[0].clone(),
                n => anyhow::bail!(
                    "\"{}\" matches {} tags:\n{}",
                    target,
                    n,
                    cands
                        .iter()
                        .map(|t| format!("  {}", t.title))
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
            }
        }
    };
    let link = tag.title.trim_start_matches('#').to_lowercase();
    let (notes, total) = db.notes_in_tag(&link, limit)?;
    if notes.is_empty() {
        println!("tag {} has no notes", tag.title);
        return Ok(());
    }
    println!(
        "{}",
        render::count_line(
            notes.len(),
            total,
            &format!("notes in tag {}", tag.title),
            &env.cfg
        )
    );
    println!(
        "{}",
        render::list_notes(&notes, &db.catalog().unwrap_or_default(), &env.cfg)
    );
    Ok(())
}

fn run_list(db: &Db, mode: ListMode, env: &Env, limit: usize) -> Result<()> {
    let (notes, total) = db.list(mode, limit)?;
    if notes.is_empty() {
        println!("no notes in this list");
        return Ok(());
    }
    let label = match mode {
        ListMode::All => "notes",
        ListMode::Trash => "trashed notes",
        ListMode::Pinned => "pinned notes",
        ListMode::Bookmarked => "bookmarked notes",
        ListMode::Templates => "templates",
    };
    println!(
        "{}",
        render::count_line(notes.len(), total, label, &env.cfg)
    );
    println!(
        "{}",
        render::list_notes(&notes, &db.catalog().unwrap_or_default(), &env.cfg)
    );
    Ok(())
}

fn run_stats(db: &Db, env: &Env) -> Result<()> {
    let stats = db.stats(&env.images_dir)?;
    println!("{}", render::stats_view(&stats, &env.cfg));
    Ok(())
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = real_main(&cli) {
        eprintln!("upnote-cli: {e:#}");
        std::process::exit(1);
    }
}

fn real_main(cli: &Cli) -> Result<()> {
    let env = build_env(cli)?;
    let db = Db::open(&env.db_path)
        .with_context(|| format!("cannot open UpNote database at {}", env.db_path.display()))?;
    match &cli.command {
        Command::Search { query } => run_search(&db, query, &env, cli.limit),
        Command::Note { target } => run_note(&db, target, &env),
        Command::Notebooks => run_notebooks(&db, &env),
        Command::Notebook { target, recursive } => {
            run_notebook(&db, target, *recursive, &env, cli.limit)
        }
        Command::Tags => run_tags(&db, &env),
        Command::Tag { target } => run_tag(&db, target, &env, cli.limit),
        Command::List { mode } => run_list(&db, ListMode::from(*mode), &env, cli.limit),
        Command::Stats => run_stats(&db, &env),
    }
}
