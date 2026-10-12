use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::PathBuf;

const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_NAME_CHARS: usize = 120;
const MAX_CONTENT_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub id: u64,
    pub parent: Option<u64>,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Snippet {
    pub id: u64,
    pub group: u64,
    pub name: String,
    pub content: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct Document {
    version: u32,
    next_id: u64,
    groups: Vec<Group>,
    snippets: Vec<Snippet>,
}

pub struct TemplateStore {
    path: PathBuf,
    document: Document,
}

impl TemplateStore {
    pub fn open(path: PathBuf) -> io::Result<Self> {
        let document = match fs::metadata(&path) {
            Ok(metadata) => {
                if metadata.len() > MAX_FILE_BYTES {
                    return Err(invalid_data("Template database is too large."));
                }
                let bytes = fs::read(&path)?;
                serde_json::from_slice::<Document>(&bytes)
                    .map_err(|_| invalid_data("Template database is malformed."))?
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Document {
                version: 1,
                next_id: 1,
                groups: Vec::new(),
                snippets: Vec::new(),
            },
            Err(error) => return Err(error),
        };
        validate_document(&document)?;
        Ok(Self { path, document })
    }

    pub fn groups(&self) -> &[Group] {
        &self.document.groups
    }
    pub fn snippets(&self) -> &[Snippet] {
        &self.document.snippets
    }

    pub fn add_group(&mut self, parent: Option<u64>, name: String) -> io::Result<u64> {
        validate_name(&name)?;
        if let Some(parent) = parent {
            if !self.document.groups.iter().any(|group| group.id == parent) {
                return Err(invalid_input("Parent group is unavailable."));
            }
        }
        self.change(|document| {
            let id = take_id(document)?;
            document.groups.push(Group { id, parent, name });
            Ok(id)
        })
    }

    pub fn rename_group(&mut self, id: u64, name: String) -> io::Result<()> {
        validate_name(&name)?;
        self.change(|document| {
            let group = document
                .groups
                .iter_mut()
                .find(|group| group.id == id)
                .ok_or_else(|| invalid_input("Group is unavailable."))?;
            group.name = name;
            Ok(())
        })
    }

    pub fn remove_group(&mut self, id: u64) -> io::Result<()> {
        if !self.document.groups.iter().any(|group| group.id == id) {
            return Err(invalid_input("Group is unavailable."));
        }
        self.change(|document| {
            let mut removed = HashSet::from([id]);
            loop {
                let before = removed.len();
                for group in &document.groups {
                    if group.parent.is_some_and(|parent| removed.contains(&parent)) {
                        removed.insert(group.id);
                    }
                }
                if removed.len() == before {
                    break;
                }
            }
            document.groups.retain(|group| !removed.contains(&group.id));
            document
                .snippets
                .retain(|snippet| !removed.contains(&snippet.group));
            Ok(())
        })
    }

    pub fn add_snippet(&mut self, group: u64, name: String) -> io::Result<u64> {
        validate_name(&name)?;
        if !self.document.groups.iter().any(|item| item.id == group) {
            return Err(invalid_input("Select a group first."));
        }
        self.change(|document| {
            let id = take_id(document)?;
            document.snippets.push(Snippet {
                id,
                group,
                name,
                content: String::new(),
            });
            Ok(id)
        })
    }

    pub fn save_snippet(&mut self, id: u64, name: String, content: String) -> io::Result<()> {
        validate_name(&name)?;
        if content.len() > MAX_CONTENT_BYTES {
            return Err(invalid_input("Template text is too large."));
        }
        self.change(|document| {
            let snippet = document
                .snippets
                .iter_mut()
                .find(|snippet| snippet.id == id)
                .ok_or_else(|| invalid_input("Template is unavailable."))?;
            snippet.name = name;
            snippet.content = content;
            Ok(())
        })
    }

    pub fn remove_snippet(&mut self, id: u64) -> io::Result<()> {
        self.change(|document| {
            let before = document.snippets.len();
            document.snippets.retain(|snippet| snippet.id != id);
            if document.snippets.len() == before {
                return Err(invalid_input("Template is unavailable."));
            }
            Ok(())
        })
    }

    fn change<T>(&mut self, change: impl FnOnce(&mut Document) -> io::Result<T>) -> io::Result<T> {
        let mut updated = self.document.clone();
        let output = change(&mut updated)?;
        validate_document(&updated).map_err(|_| {
            invalid_input("Template database limit reached or group nesting is too deep.")
        })?;
        persist(&self.path, &updated)?;
        self.document = updated;
        Ok(output)
    }
}

fn validate_document(document: &Document) -> io::Result<()> {
    if document.version != 1 || document.groups.len() > 1000 || document.snippets.len() > 10_000 {
        return Err(invalid_data("Unsupported template database."));
    }
    let mut ids = HashSet::new();
    for group in &document.groups {
        validate_name(&group.name).map_err(|_| invalid_data("Invalid group name."))?;
        if group.id == 0 || !ids.insert(group.id) {
            return Err(invalid_data("Duplicate group ID."));
        }
    }
    for group in &document.groups {
        let mut current = group.parent;
        let mut visited = HashSet::new();
        while let Some(parent) = current {
            if !visited.insert(parent) {
                return Err(invalid_data("Template groups contain a cycle."));
            }
            if visited.len() > 32 {
                return Err(invalid_data("Template groups are nested too deeply."));
            }
            current = document
                .groups
                .iter()
                .find(|candidate| candidate.id == parent)
                .ok_or_else(|| invalid_data("Missing parent group."))?
                .parent;
        }
    }
    for snippet in &document.snippets {
        validate_name(&snippet.name).map_err(|_| invalid_data("Invalid template name."))?;
        if !ids.insert(snippet.id)
            || !document
                .groups
                .iter()
                .any(|group| group.id == snippet.group)
            || snippet.content.len() > MAX_CONTENT_BYTES
        {
            return Err(invalid_data("Invalid template record."));
        }
    }
    if document.next_id <= ids.iter().copied().max().unwrap_or(0) {
        return Err(invalid_data("Invalid template ID sequence."));
    }
    Ok(())
}

fn validate_name(name: &str) -> io::Result<()> {
    if name.trim().is_empty() || name.chars().count() > MAX_NAME_CHARS || name.contains('\0') {
        Err(invalid_input("Name must be 1–120 characters."))
    } else {
        Ok(())
    }
}

fn take_id(document: &mut Document) -> io::Result<u64> {
    let id = document.next_id;
    document.next_id = document
        .next_id
        .checked_add(1)
        .ok_or_else(|| invalid_input("Template ID limit reached."))?;
    Ok(id)
}

fn persist(path: &PathBuf, document: &Document) -> io::Result<()> {
    let bytes = serde_json::to_vec(document).map_err(io::Error::other)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(invalid_input("Template database is full."));
    }
    let parent = path
        .parent()
        .ok_or_else(|| invalid_input("Invalid template path."))?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension("tmp");
    let result = (|| {
        let mut file = File::create(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn invalid_input(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn invalid_data(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "rustclip-templates-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn nested_groups_and_templates_survive_restart() {
        let path = temp_path();
        let mut store = TemplateStore::open(path.clone()).unwrap();
        let root = store.add_group(None, "Work".into()).unwrap();
        let child = store.add_group(Some(root), "Build".into()).unwrap();
        let snippet = store.add_snippet(child, "Command".into()).unwrap();
        store
            .save_snippet(snippet, "Command".into(), "cargo test".into())
            .unwrap();
        drop(store);
        let store = TemplateStore::open(path.clone()).unwrap();
        assert_eq!(store.groups().len(), 2);
        assert_eq!(store.snippets()[0].content, "cargo test");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn removing_parent_removes_descendants() {
        let path = temp_path();
        let mut store = TemplateStore::open(path.clone()).unwrap();
        let root = store.add_group(None, "Work".into()).unwrap();
        let child = store.add_group(Some(root), "Build".into()).unwrap();
        store.add_snippet(child, "Command".into()).unwrap();
        store.remove_group(root).unwrap();
        assert!(store.groups().is_empty());
        assert!(store.snippets().is_empty());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn rename_and_delete_template_persist() {
        let path = temp_path();
        let mut store = TemplateStore::open(path.clone()).unwrap();
        let group = store.add_group(None, "Old".into()).unwrap();
        store.rename_group(group, "Renamed".into()).unwrap();
        let snippet = store.add_snippet(group, "Temporary".into()).unwrap();
        store.remove_snippet(snippet).unwrap();
        drop(store);
        let store = TemplateStore::open(path.clone()).unwrap();
        assert_eq!(store.groups()[0].name, "Renamed");
        assert!(store.snippets().is_empty());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn malformed_database_is_rejected() {
        let path = temp_path();
        fs::write(&path, b"{bad").unwrap();
        assert!(TemplateStore::open(path.clone()).is_err());
        fs::remove_file(path).unwrap();
    }
}
