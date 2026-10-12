use std::fs::{self, File};
use std::io::{self, Write};
#[cfg(windows)]
use std::path::Path;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_LIMIT: u64 = 64 * 1024 * 1024;
pub const MAX_TEXT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_IMAGE_BYTES: usize = 48 * 1024 * 1024;
const MAGIC: &[u8; 8] = b"RCLIP001";
const HEADER_BYTES: usize = 28;
const ENTRY_OVERHEAD: u64 = 64;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageBlob {
    pub format: u32,
    pub dib: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl ImageBlob {
    pub fn from_dib(format: u32, dib: Vec<u8>) -> Option<Self> {
        if !matches!(format, 8 | 17) || dib.len() < 40 || dib.len() > MAX_IMAGE_BYTES {
            return None;
        }
        let header_size = u32::from_le_bytes(dib[0..4].try_into().ok()?) as usize;
        if header_size < 40 || header_size > dib.len() {
            return None;
        }
        let width = i32::from_le_bytes(dib[4..8].try_into().ok()?);
        let height = i32::from_le_bytes(dib[8..12].try_into().ok()?);
        let planes = u16::from_le_bytes(dib[12..14].try_into().ok()?);
        let depth = u16::from_le_bytes(dib[14..16].try_into().ok()?);
        let compression = u32::from_le_bytes(dib[16..20].try_into().ok()?);
        if width <= 0 || height == 0 || planes != 1 || !matches!(depth, 1 | 4 | 8 | 16 | 24 | 32) {
            return None;
        }
        if !matches!(compression, 0 | 3 | 6) {
            return None;
        }
        let width = width as u32;
        let height = height.unsigned_abs();
        if width.checked_mul(height)? > 80_000_000 {
            return None;
        }
        let row_bytes = (u64::from(width) * u64::from(depth)).div_ceil(32) * 4;
        let pixel_bytes = row_bytes.checked_mul(u64::from(height))?;
        if (header_size as u64).checked_add(pixel_bytes)? > dib.len() as u64 {
            return None;
        }
        Some(Self {
            format,
            dib,
            width,
            height,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClipData {
    pub text: Option<String>,
    pub image: Option<ImageBlob>,
}

impl ClipData {
    pub fn new(text: Option<String>, image: Option<ImageBlob>) -> Option<Self> {
        let text = text.filter(|value| !value.is_empty() && value.len() <= MAX_TEXT_BYTES);
        let image = image.filter(|value| value.dib.len() <= MAX_IMAGE_BYTES);
        if text.is_none() && image.is_none() {
            None
        } else {
            Some(Self { text, image })
        }
    }

    pub fn text(text: String) -> Option<Self> {
        Self::new(Some(text), None)
    }
}

#[derive(Clone)]
pub struct Entry {
    pub id: u128,
    pub data: ClipData,
    pub size: u64,
    legacy: bool,
}

pub struct Store {
    directory: PathBuf,
    limit: u64,
    used: u64,
    entries: Vec<Entry>,
}

impl Store {
    pub fn open(directory: PathBuf, limit: u64) -> io::Result<Self> {
        fs::create_dir_all(&directory)?;
        let mut entries = Vec::new();
        for file in fs::read_dir(&directory)? {
            let file = file?;
            let name = file.file_name();
            let Some((id, legacy)) = name.to_str().and_then(parse_name) else {
                continue;
            };
            let metadata = file.metadata()?;
            if metadata.len() > (MAX_TEXT_BYTES + MAX_IMAGE_BYTES + HEADER_BYTES) as u64 {
                continue;
            }
            let data = if legacy {
                fs::read_to_string(file.path())
                    .ok()
                    .and_then(ClipData::text)
            } else {
                fs::read(file.path())
                    .ok()
                    .and_then(|bytes| decode_clip(&bytes))
            };
            if let Some(data) = data {
                entries.push(Entry {
                    id,
                    size: stored_size(&data),
                    data,
                    legacy,
                });
            }
        }
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.id));
        let used = entries.iter().map(|entry| entry.size).sum();
        let mut store = Self {
            directory,
            limit,
            used,
            entries,
        };
        store.prune()?;
        Ok(store)
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    #[cfg(windows)]
    pub fn get(&self, id: u128) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    pub fn insert_data(&mut self, data: ClipData) -> io::Result<bool> {
        let Some(data) = ClipData::new(data.text, data.image) else {
            return Ok(false);
        };
        let size = stored_size(&data);
        if size > self.limit {
            return Ok(false);
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let id = now.max(
            self.entries
                .first()
                .map_or(0, |entry| entry.id.saturating_add(1)),
        );
        let temporary = self.directory.join(format!("{id:039}.tmp"));
        let final_path = self.directory.join(format!("{id:039}.clip"));
        let result = (|| {
            let encoded = encode_clip(&data)?;
            let mut file = File::create(&temporary)?;
            file.write_all(&encoded)?;
            file.sync_all()?;
            fs::rename(&temporary, &final_path)
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        self.entries.insert(
            0,
            Entry {
                id,
                data,
                size,
                legacy: false,
            },
        );
        self.used += size;
        self.prune()?;
        Ok(true)
    }

    #[cfg(test)]
    pub fn insert(&mut self, text: String) -> io::Result<bool> {
        match ClipData::text(text) {
            Some(data) => self.insert_data(data),
            None => Ok(false),
        }
    }

    #[cfg(windows)]
    pub fn clear(&mut self) -> io::Result<()> {
        while let Some(entry) = self.entries.last() {
            fs::remove_file(self.entry_path(entry))?;
            self.used -= entry.size;
            self.entries.pop();
        }
        Ok(())
    }

    #[cfg(windows)]
    pub fn remove(&mut self, id: u128) -> io::Result<bool> {
        let Some(index) = self.entries.iter().position(|entry| entry.id == id) else {
            return Ok(false);
        };
        fs::remove_file(self.entry_path(&self.entries[index]))?;
        let entry = self.entries.remove(index);
        self.used -= entry.size;
        Ok(true)
    }

    pub fn set_limit(&mut self, limit: u64) -> io::Result<()> {
        self.limit = limit;
        self.prune()
    }

    fn prune(&mut self) -> io::Result<()> {
        while self.used > self.limit {
            let Some(entry) = self.entries.last() else {
                break;
            };
            fs::remove_file(self.entry_path(entry))?;
            self.used -= entry.size;
            self.entries.pop();
        }
        Ok(())
    }

    fn entry_path(&self, entry: &Entry) -> PathBuf {
        self.directory.join(format!(
            "{:039}.{}",
            entry.id,
            if entry.legacy { "txt" } else { "clip" }
        ))
    }
}

fn parse_name(name: &str) -> Option<(u128, bool)> {
    if let Some(id) = name
        .strip_suffix(".clip")
        .and_then(|value| value.parse().ok())
    {
        Some((id, false))
    } else {
        name.strip_suffix(".txt")
            .and_then(|value| value.parse().ok())
            .map(|id| (id, true))
    }
}

fn stored_size(data: &ClipData) -> u64 {
    let text = data.text.as_ref().map_or(0, |value| value.len()) as u64;
    let image = data.image.as_ref().map_or(0, |value| value.dib.len()) as u64;
    HEADER_BYTES as u64 + text + image + ENTRY_OVERHEAD
}

fn encode_clip(data: &ClipData) -> io::Result<Vec<u8>> {
    let text = data.text.as_ref().map_or(&[][..], |value| value.as_bytes());
    let image = data.image.as_ref();
    let dib = image.map_or(&[][..], |value| value.dib.as_slice());
    let text_len = u32::try_from(text.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Text too large"))?;
    let dib_len = u32::try_from(dib.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Image too large"))?;
    let mut bytes = Vec::with_capacity(HEADER_BYTES + text.len() + dib.len());
    bytes.extend_from_slice(MAGIC);
    for value in [
        text_len,
        dib_len,
        image.map_or(0, |value| value.format),
        image.map_or(0, |value| value.width),
        image.map_or(0, |value| value.height),
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(text);
    bytes.extend_from_slice(dib);
    Ok(bytes)
}

fn decode_clip(bytes: &[u8]) -> Option<ClipData> {
    if bytes.len() < HEADER_BYTES || &bytes[..8] != MAGIC {
        return None;
    }
    let read = |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let text_len = read(8) as usize;
    let dib_len = read(12) as usize;
    let format = read(16);
    let width = read(20);
    let height = read(24);
    if text_len > MAX_TEXT_BYTES
        || dib_len > MAX_IMAGE_BYTES
        || HEADER_BYTES.checked_add(text_len)?.checked_add(dib_len)? != bytes.len()
    {
        return None;
    }
    let text = if text_len == 0 {
        None
    } else {
        Some(String::from_utf8(bytes[HEADER_BYTES..HEADER_BYTES + text_len].to_vec()).ok()?)
    };
    let image = if dib_len == 0 {
        None
    } else {
        let dib = bytes[HEADER_BYTES + text_len..].to_vec();
        let image = ImageBlob::from_dib(format, dib)?;
        if image.width != width || image.height != height {
            return None;
        }
        Some(image)
    };
    ClipData::new(text, image)
}

#[cfg(windows)]
pub fn default_directory() -> io::Result<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA is unavailable"))?;
    Ok(Path::new(&base).join("rustclip").join("history"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_directory() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "rustclip-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn sample_dib() -> ImageBlob {
        let mut dib = vec![0; 44];
        dib[0..4].copy_from_slice(&40_u32.to_le_bytes());
        dib[4..8].copy_from_slice(&1_i32.to_le_bytes());
        dib[8..12].copy_from_slice(&1_i32.to_le_bytes());
        dib[12..14].copy_from_slice(&1_u16.to_le_bytes());
        dib[14..16].copy_from_slice(&32_u16.to_le_bytes());
        ImageBlob::from_dib(8, dib).unwrap()
    }

    #[test]
    fn text_and_image_round_trip_as_one_event() {
        let path = temporary_directory();
        let mut store = Store::open(path.clone(), DEFAULT_LIMIT).unwrap();
        let data = ClipData::new(Some("caption".into()), Some(sample_dib())).unwrap();
        store.insert_data(data.clone()).unwrap();
        drop(store);
        let store = Store::open(path.clone(), DEFAULT_LIMIT).unwrap();
        assert_eq!(store.entries()[0].data, data);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn image_bytes_count_toward_pruning() {
        let path = temporary_directory();
        let mut store = Store::open(path.clone(), 200).unwrap();
        store.insert("older text".into()).unwrap();
        store
            .insert_data(ClipData::new(None, Some(sample_dib())).unwrap())
            .unwrap();
        assert_eq!(store.entries().len(), 1);
        assert!(store.entries()[0].data.image.is_some());
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn truncated_image_is_rejected_without_losing_text() {
        let mut dib = sample_dib().dib;
        dib.truncate(41);
        assert!(ImageBlob::from_dib(8, dib).is_none());
        assert_eq!(
            ClipData::new(Some("kept".into()), None)
                .unwrap()
                .text
                .as_deref(),
            Some("kept")
        );
    }

    #[test]
    fn identical_external_copies_create_separate_entries() {
        let path = temporary_directory();
        let mut store = Store::open(path.clone(), DEFAULT_LIMIT).unwrap();
        store.insert("same".into()).unwrap();
        store.insert("same".into()).unwrap();
        assert_eq!(store.entries().len(), 2);
        assert_ne!(store.entries()[0].id, store.entries()[1].id);
        drop(store);
        assert_eq!(
            Store::open(path.clone(), DEFAULT_LIMIT)
                .unwrap()
                .entries()
                .len(),
            2
        );
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn oversized_entry_does_not_prune_existing_history() {
        let path = temporary_directory();
        let mut store = Store::open(path.clone(), 100).unwrap();
        store.insert("keep".into()).unwrap();
        assert!(!store.insert("much too long".into()).unwrap());
        assert_eq!(store.entries()[0].data.text.as_deref(), Some("keep"));
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn lowering_limit_prunes_oldest_entry() {
        let path = temporary_directory();
        let mut store = Store::open(path.clone(), DEFAULT_LIMIT).unwrap();
        store.insert("old".into()).unwrap();
        store.insert("new".into()).unwrap();
        store.set_limit(95).unwrap();
        assert_eq!(store.entries().len(), 1);
        assert_eq!(store.entries()[0].data.text.as_deref(), Some("new"));
        drop(store);
        let store = Store::open(path.clone(), 95).unwrap();
        assert_eq!(store.entries().len(), 1);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn interrupted_or_malformed_files_do_not_hide_valid_history() {
        let path = temporary_directory();
        let mut store = Store::open(path.clone(), DEFAULT_LIMIT).unwrap();
        store.insert("valid".into()).unwrap();
        drop(store);
        fs::write(path.join("999.tmp"), "interrupted").unwrap();
        fs::write(path.join("999.clip"), [0xff, 0xfe]).unwrap();
        let store = Store::open(path.clone(), DEFAULT_LIMIT).unwrap();
        assert_eq!(store.entries().len(), 1);
        assert_eq!(store.entries()[0].data.text.as_deref(), Some("valid"));
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn rapid_insertions_keep_event_order() {
        let path = temporary_directory();
        let mut store = Store::open(path.clone(), DEFAULT_LIMIT).unwrap();
        for index in 0..64 {
            store.insert(format!("copy {index}")).unwrap();
        }
        drop(store);
        let store = Store::open(path.clone(), DEFAULT_LIMIT).unwrap();
        assert_eq!(store.entries().len(), 64);
        assert_eq!(store.entries()[0].data.text.as_deref(), Some("copy 63"));
        assert_eq!(store.entries()[63].data.text.as_deref(), Some("copy 0"));
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn legacy_text_files_still_load() {
        let path = temporary_directory();
        fs::write(path.join("123.txt"), "earlier copy").unwrap();
        let store = Store::open(path.clone(), DEFAULT_LIMIT).unwrap();
        assert_eq!(
            store.entries()[0].data.text.as_deref(),
            Some("earlier copy")
        );
        fs::remove_dir_all(path).unwrap();
    }
}
