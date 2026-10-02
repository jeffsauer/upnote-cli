use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use rusqlite::{Connection, params};
use serde_json::Value;

use crate::model::{Catalog, FileMeta, Note, Notebook, Stats, Tag};

pub const NOTE_COLS: &str = "id, COALESCE(title,'') AS title, COALESCE(summary,'') AS summary, \
    COALESCE(html,'') AS html, COALESCE(text,'') AS text, COALESCE(fileIds,'[]') AS fileIds, \
    COALESCE(tagLinks,'[]') AS tagLinks, \
    COALESCE(pinned,0), COALESCE(bookmarked,0), COALESCE(trashed,0), COALESCE(isTemplate,0), \
    COALESCE(shared,0), createdAt, updatedAt";

pub struct Db {
    conn: Connection,
    db_path: PathBuf,
}

#[derive(Clone, Copy)]
pub enum ListMode {
    All,
    Trash,
    Pinned,
    Bookmarked,
    Templates,
}

fn json_str_array(s: &str) -> Vec<String> {
    match serde_json::from_str::<Value>(s) {
        Ok(Value::Array(items)) => items
            .into_iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        _ => Vec::new(),
    }
}

fn map_note(row: &rusqlite::Row) -> rusqlite::Result<Note> {
    Ok(Note {
        id: row.get(0)?,
        title: row.get(1)?,
        summary: row.get(2)?,
        html: row.get(3)?,
        text: row.get(4)?,
        file_ids: json_str_array(&row.get::<_, String>(5)?),
        notebook_ids: Vec::new(),
        tag_links: json_str_array(&row.get::<_, String>(6)?),
        pinned: row.get::<_, i64>(7)? != 0,
        bookmarked: row.get::<_, i64>(8)? != 0,
        trashed: row.get::<_, i64>(9)? != 0,
        is_template: row.get::<_, i64>(10)? != 0,
        shared: row.get::<_, i64>(11)? != 0,
        created_at: row.get::<_, Option<f64>>(12)?.map(|v| v as i64),
        updated_at: row.get::<_, Option<f64>>(13)?.map(|v| v as i64),
    })
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if !path.is_file() {
            return Err(anyhow!(
                "database not found: {} (is the UpNote desktop app installed?)",
                path.display()
            ));
        }
        let path = path.to_path_buf();
        let uri = format!("file:{}?mode=ro", path.to_string_lossy());
        let conn = match Connection::open(&uri) {
            Ok(c) => c,
            Err(_) => Connection::open(&path)
                .with_context(|| format!("cannot open database {}", path.display()))?,
        };
        Ok(Self {
            conn,
            db_path: path,
        })
    }

    pub fn get_note(&self, id: &str) -> Result<Option<Note>> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {NOTE_COLS} FROM notes WHERE id = ?1"))?;
        let mut rows = stmt.query_map(params![id], map_note)?;
        let mut note = None;
        if let Some(r) = rows.next() {
            note = Some(r?);
        }
        if let Some(n) = note.as_mut() {
            n.notebook_ids = self.notebook_ids_for(&n.id);
        }
        Ok(note)
    }

    fn notebook_ids_for(&self, note_id: &str) -> Vec<String> {
        let mut out = Vec::new();
        let stmt = self
            .conn
            .prepare("SELECT notebookId FROM organizers WHERE deleted = 0 AND noteId = ?1")
            .ok();
        if let Some(mut stmt) = stmt
            && let Ok(rows) = stmt.query_map(params![note_id], |r| r.get::<_, String>(0))
        {
            for r in rows.flatten() {
                out.push(r);
            }
        }
        if out.is_empty()
            && let Ok(links) = self.conn.query_row(
                "SELECT COALESCE(notebookLinks,'[]') FROM notes WHERE id = ?1",
                params![note_id],
                |r| r.get::<_, String>(0),
            )
        {
            out = json_str_array(&links);
        }
        out
    }

    pub fn find_note(&self, query: &str) -> Result<Note> {
        if let Some(n) = self.get_note(query)? {
            return Ok(n);
        }
        let pattern = format!("%{query}%");
        let sql = format!(
            "SELECT {NOTE_COLS} FROM notes WHERE deleted = 0 \
             AND (title = ?1 COLLATE NOCASE OR title LIKE ?2 COLLATE NOCASE) \
             ORDER BY updatedAt DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows: Vec<Note> = stmt
            .query_map(params![query, pattern], map_note)?
            .collect::<Result<_, _>>()?;
        match rows.len() {
            0 => Err(anyhow!("no note found matching \"{query}\"")),
            1 => {
                let mut n = rows.into_iter().next().unwrap();
                n.notebook_ids = self.notebook_ids_for(&n.id);
                Ok(n)
            }
            n => Err(anyhow!(
                "\"{query}\" matches {} notes:\n{}",
                n,
                rows.iter()
                    .map(|x| format!("  {}  ({})", x.title, x.id))
                    .collect::<Vec<_>>()
                    .join("\n")
            )),
        }
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<(Vec<Note>, i64)> {
        let pattern = format!("%{query}%");
        let where_clause = "deleted = 0 AND trashed = 0 \
             AND (title LIKE ?1 COLLATE NOCASE OR summary LIKE ?1 COLLATE NOCASE \
             OR text LIKE ?1 COLLATE NOCASE OR html LIKE ?1 COLLATE NOCASE)";
        let total: i64 = self
            .conn
            .query_row(
                &format!("SELECT COUNT(*) FROM notes WHERE {where_clause}"),
                params![pattern],
                |r| r.get(0),
            )
            .context("search count")?;
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {NOTE_COLS} FROM notes WHERE {where_clause} ORDER BY updatedAt DESC LIMIT ?2"
        ))?;
        let notes: Vec<Note> = stmt
            .query_map(params![pattern, limit as i64], map_note)?
            .collect::<Result<_, _>>()?;
        let orgs = self.notebook_map()?;
        let notes: Vec<Note> = notes
            .into_iter()
            .map(|mut n| {
                n.notebook_ids = orgs.get(&n.id).cloned().unwrap_or_default();
                n
            })
            .collect();
        Ok((notes, total))
    }

    pub fn notebook_map(&self) -> Result<HashMap<String, Vec<String>>> {
        let mut stmt = self
            .conn
            .prepare("SELECT noteId, notebookId FROM organizers WHERE deleted = 0")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for r in rows {
            let (note, nb) = r?;
            map.entry(note).or_default().push(nb);
        }
        Ok(map)
    }

    pub fn list(&self, mode: ListMode, limit: usize) -> Result<(Vec<Note>, i64)> {
        let filter = match mode {
            ListMode::All => "deleted = 0 AND trashed = 0",
            ListMode::Trash => "deleted = 0 AND trashed = 1",
            ListMode::Pinned => "deleted = 0 AND trashed = 0 AND pinned = 1",
            ListMode::Bookmarked => "deleted = 0 AND trashed = 0 AND bookmarked = 1",
            ListMode::Templates => "deleted = 0 AND trashed = 0 AND isTemplate = 1",
        };
        let total: i64 = self
            .conn
            .query_row(
                &format!("SELECT COUNT(*) FROM notes WHERE {filter}"),
                [],
                |r| r.get(0),
            )
            .context("list count")?;
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {NOTE_COLS} FROM notes WHERE {filter} ORDER BY updatedAt DESC LIMIT ?1"
        ))?;
        let notes: Vec<Note> = stmt
            .query_map(params![limit as i64], map_note)?
            .collect::<Result<_, _>>()?;
        let orgs = self.notebook_map()?;
        let notes: Vec<Note> = notes
            .into_iter()
            .map(|mut n| {
                n.notebook_ids = orgs.get(&n.id).cloned().unwrap_or_default();
                n
            })
            .collect();
        Ok((notes, total))
    }

    pub fn notebooks(&self) -> Result<Vec<Notebook>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, COALESCE(title,''), parent, locked, updatedAt \
             FROM notebooks WHERE deleted = 0",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Notebook {
                id: r.get(0)?,
                title: r.get(1)?,
                parent: r.get(2)?,
                locked: r.get::<_, i64>(3)? != 0,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn find_notebook(&self, query: &str) -> Result<Notebook> {
        let mut stmt = self.conn.prepare(
            "SELECT id, COALESCE(title,''), parent, locked, updatedAt FROM notebooks \
             WHERE deleted = 0 AND (id = ?1 OR title = ?1 COLLATE NOCASE OR title LIKE ?2 COLLATE NOCASE) \
             ORDER BY CASE WHEN id = ?1 OR title = ?1 COLLATE NOCASE THEN 0 ELSE 1 END, title",
        )?;
        let rows: Vec<Notebook> = stmt
            .query_map(params![query, format!("%{query}%")], |r| {
                Ok(Notebook {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    parent: r.get(2)?,
                    locked: r.get::<_, i64>(3)? != 0,
                })
            })?
            .collect::<Result<_, _>>()?;
        match rows.len() {
            0 => Err(anyhow!("no notebook found matching \"{query}\"")),
            1 => Ok(rows.into_iter().next().unwrap()),
            n => Err(anyhow!(
                "\"{query}\" matches {} notebooks:\n{}",
                n,
                rows.iter()
                    .map(|x| format!("  {}  ({})", x.title, x.id))
                    .collect::<Vec<_>>()
                    .join("\n")
            )),
        }
    }

    pub fn notebook_descendants(&self, id: &str) -> Result<Vec<String>> {
        let all = self.notebooks()?;
        let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
        for n in &all {
            if let Some(p) = &n.parent {
                children.entry(p.as_str()).or_default().push(n.id.as_str());
            }
        }
        let mut out = vec![id.to_string()];
        let mut stack = vec![id.to_string()];
        let mut seen: HashSet<String> = HashSet::new();
        seen.insert(id.to_string());
        while let Some(cur) = stack.pop() {
            if let Some(kids) = children.get(cur.as_str()) {
                for child in kids {
                    if seen.insert(child.to_string()) {
                        out.push(child.to_string());
                        stack.push(child.to_string());
                    }
                }
            }
        }
        Ok(out)
    }

    pub fn notes_in_notebooks(&self, ids: &[String], limit: usize) -> Result<(Vec<Note>, i64)> {
        let marks: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
        let in_clause = marks.join(",");
        let vals: Vec<rusqlite::types::Value> = ids
            .iter()
            .map(|s| rusqlite::types::Value::Text(s.clone()))
            .collect();
        let total: i64 = self.conn.query_row(
            &format!(
                "SELECT COUNT(*) FROM notes n WHERE n.deleted = 0 \
                 AND n.id IN (SELECT noteId FROM organizers WHERE deleted = 0 AND notebookId IN ({in_clause}))"
            ),
            rusqlite::params_from_iter(vals.iter().cloned()),
            |r| r.get(0),
        )
        .context("notebook notes count")?;
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {NOTE_COLS} FROM notes n WHERE n.deleted = 0 \
             AND n.id IN (SELECT noteId FROM organizers WHERE deleted = 0 AND notebookId IN ({in_clause})) \
             ORDER BY n.updatedAt DESC"
        ))?;
        let notes: Vec<Note> = stmt
            .query_map(rusqlite::params_from_iter(vals), map_note)?
            .collect::<Result<_, _>>()?;
        let orgs = self.notebook_map()?;
        let mut notes: Vec<Note> = notes
            .into_iter()
            .map(|mut n| {
                n.notebook_ids = orgs.get(&n.id).cloned().unwrap_or_default();
                n
            })
            .collect();
        notes.truncate(limit);
        Ok((notes, total))
    }

    pub fn tags(&self) -> Result<Vec<Tag>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, COALESCE(title,'') AS title FROM tags WHERE deleted = 0 ORDER BY title COLLATE NOCASE")?;
        let rows = stmt.query_map([], |r| Ok(Tag { title: r.get(1)? }))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn tag_link_counts(&self) -> Result<HashMap<String, i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT COALESCE(tagLinks,'[]') FROM notes WHERE deleted = 0 AND trashed = 0",
        )?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut counts: HashMap<String, i64> = HashMap::new();
        for r in rows {
            let links = json_str_array(&r?);
            for l in links {
                *counts.entry(l.to_lowercase()).or_insert(0) += 1;
            }
        }
        Ok(counts)
    }

    pub fn notes_in_tag(&self, link: &str, limit: usize) -> Result<(Vec<Note>, i64)> {
        let target = link.to_lowercase();
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {NOTE_COLS} FROM notes WHERE deleted = 0 AND trashed = 0 ORDER BY updatedAt DESC"
        ))?;
        let all: Vec<Note> = stmt.query_map([], map_note)?.collect::<Result<_, _>>()?;
        let orgs = self.notebook_map()?;
        let matched: Vec<Note> = all
            .into_iter()
            .filter(|n| n.tag_links.iter().any(|l| l.to_lowercase() == target))
            .map(|mut n| {
                n.notebook_ids = orgs.get(&n.id).cloned().unwrap_or_default();
                n
            })
            .collect();
        let total = matched.len() as i64;
        let mut truncated = matched;
        truncated.truncate(limit);
        Ok((truncated, total))
    }

    pub fn files_for(&self, ids: &[String]) -> Result<Vec<FileMeta>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let marks: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
        let vals: Vec<rusqlite::types::Value> = ids
            .iter()
            .map(|s| rusqlite::types::Value::Text(s.clone()))
            .collect();
        let mut stmt = self.conn.prepare(&format!(
            "SELECT id, COALESCE(name,'') AS name, downloadURL FROM files \
             WHERE deleted = 0 AND id IN ({})",
            marks.join(",")
        ))?;
        let rows = stmt.query_map(rusqlite::params_from_iter(vals), |r| {
            Ok(FileMeta {
                id: r.get(0)?,
                name: r.get(1)?,
                download_url: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn catalog(&self) -> Result<Catalog> {
        let notebooks = self.notebooks()?;
        let tags = self.tags()?;
        let mut notebook_titles: HashMap<String, String> = HashMap::new();
        for n in &notebooks {
            notebook_titles.insert(n.id.clone(), n.title.clone());
        }
        let mut tag_by_link: HashMap<String, String> = HashMap::new();
        for t in &tags {
            let link = t.title.trim_start_matches('#').to_lowercase();
            if !link.is_empty() {
                tag_by_link.insert(link, t.title.clone());
            }
        }
        Ok(Catalog {
            notebook_titles,
            tag_by_link,
        })
    }

    pub fn stats(&self, images_dir: &Path) -> Result<Stats> {
        let (notes, trashed, pinned, bookmarked, templates): (i64, i64, i64, i64, i64) = self
            .conn
            .query_row(
                "SELECT COUNT(*), \
                 COALESCE(SUM(CASE WHEN trashed=1 THEN 1 ELSE 0 END),0), \
                 COALESCE(SUM(CASE WHEN pinned=1 AND trashed=0 THEN 1 ELSE 0 END),0), \
                 COALESCE(SUM(CASE WHEN bookmarked=1 AND trashed=0 THEN 1 ELSE 0 END),0), \
                 COALESCE(SUM(CASE WHEN isTemplate=1 AND trashed=0 THEN 1 ELSE 0 END),0) \
                 FROM notes WHERE deleted = 0",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .context("note stats")?;
        let notebooks: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM notebooks WHERE deleted = 0",
                [],
                |r| r.get(0),
            )
            .context("notebook count")?;
        let tags: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM tags WHERE deleted = 0", [], |r| {
                r.get(0)
            })
            .context("tag count")?;
        let files: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM files WHERE deleted = 0", [], |r| {
                r.get(0)
            })
            .context("file count")?;
        let mut cached_images = 0u64;
        if let Ok(rd) = std::fs::read_dir(images_dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if crate::images::is_image_name(&name) {
                    cached_images += 1;
                }
            }
        }
        let db_size = self.db_path.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(Stats {
            notes,
            trashed,
            pinned,
            bookmarked,
            templates,
            notebooks,
            tags,
            files,
            cached_images,
            db_size,
        })
    }
}
