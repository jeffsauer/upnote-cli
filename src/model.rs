use std::collections::HashMap;

pub struct Note {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub html: String,
    pub text: String,
    pub file_ids: Vec<String>,
    pub notebook_ids: Vec<String>,
    pub tag_links: Vec<String>,
    pub pinned: bool,
    pub bookmarked: bool,
    pub trashed: bool,
    pub is_template: bool,
    pub shared: bool,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}

impl Note {
    pub fn title_or_placeholder(&self) -> &str {
        if self.title.is_empty() {
            "(untitled)"
        } else {
            &self.title
        }
    }
}

pub struct Notebook {
    pub id: String,
    pub title: String,
    pub parent: Option<String>,
    pub locked: bool,
}

#[derive(Debug, Clone)]
pub struct Tag {
    pub title: String,
}

pub struct FileMeta {
    pub id: String,
    pub name: String,
    pub download_url: Option<String>,
}

pub struct Stats {
    pub notes: i64,
    pub trashed: i64,
    pub pinned: i64,
    pub bookmarked: i64,
    pub templates: i64,
    pub notebooks: i64,
    pub tags: i64,
    pub files: i64,
    pub cached_images: u64,
    pub db_size: u64,
}

#[derive(Default)]
pub struct Catalog {
    pub notebook_titles: HashMap<String, String>,
    pub tag_by_link: HashMap<String, String>,
}

impl Catalog {
    pub fn notebook_title(&self, id: &str) -> Option<&str> {
        self.notebook_titles.get(id).map(|s| s.as_str())
    }

    pub fn tag_title(&self, link: &str) -> Option<&str> {
        self.tag_by_link.get(link).map(|s| s.as_str())
    }
}
