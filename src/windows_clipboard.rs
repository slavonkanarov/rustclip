use crate::store::{self, ClipData, ImageBlob};
use windows::Win32::Foundation::{
    ERROR_SUCCESS, GetLastError, GlobalFree, HANDLE, HGLOBAL, HWND, SetLastError,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardSequenceNumber, IsClipboardFormatAvailable, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};

const CF_UNICODETEXT: u32 = 13;
const CF_DIB: u32 = 8;
const CF_DIBV5: u32 = 17;
const MAX_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;

pub struct ClipboardSnapshot {
    formats: Vec<(u32, Vec<u8>)>,
}

pub enum RestoreOutcome {
    Restored { sequence: u32, complete: bool },
    ChangedExternally,
}

struct ClipboardGuard;
impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        let _ = unsafe { CloseClipboard() };
    }
}

pub unsafe fn read() -> Option<ClipData> {
    unsafe {
        OpenClipboard(None).ok()?;
        let _guard = ClipboardGuard;
        ClipData::new(read_text_open(), read_image_open())
    }
}

unsafe fn read_text_open() -> Option<String> {
    unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT).is_err() {
            return None;
        }
        let global = HGLOBAL(GetClipboardData(CF_UNICODETEXT).ok()?.0);
        let bytes = GlobalSize(global);
        if bytes < 2 || bytes > (store::MAX_TEXT_BYTES + 1) * 2 {
            return None;
        }
        let pointer = GlobalLock(global) as *const u16;
        if pointer.is_null() {
            return None;
        }
        let units = std::slice::from_raw_parts(pointer, bytes / 2);
        let text = units
            .iter()
            .position(|unit| *unit == 0)
            .and_then(|end| String::from_utf16(&units[..end]).ok());
        let _ = GlobalUnlock(global);
        text.filter(|text| text.len() <= store::MAX_TEXT_BYTES)
    }
}

unsafe fn read_image_open() -> Option<ImageBlob> {
    unsafe {
        for format in [CF_DIBV5, CF_DIB] {
            if IsClipboardFormatAvailable(format).is_err() {
                continue;
            }
            let Ok(handle) = GetClipboardData(format) else {
                continue;
            };
            let global = HGLOBAL(handle.0);
            let bytes = GlobalSize(global);
            if bytes < 40 || bytes > store::MAX_IMAGE_BYTES {
                continue;
            }
            let pointer = GlobalLock(global) as *const u8;
            if pointer.is_null() {
                continue;
            }
            let dib = std::slice::from_raw_parts(pointer, bytes).to_vec();
            let _ = GlobalUnlock(global);
            if let Some(image) = ImageBlob::from_dib(format, dib) {
                return Some(image);
            }
        }
        None
    }
}

pub struct WriteResult {
    pub sequence: u32,
    pub complete: bool,
}

pub unsafe fn write(data: &ClipData, owner: HWND) -> Result<WriteResult, String> {
    unsafe { write_checked(data, owner, None) }
}

pub unsafe fn write_checked(
    data: &ClipData,
    owner: HWND,
    expected_sequence: Option<u32>,
) -> Result<WriteResult, String> {
    unsafe {
        let text_bytes = data.text.as_ref().map(|text| {
            text.encode_utf16()
                .chain(std::iter::once(0))
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<u8>>()
        });
        let text_memory = text_bytes
            .as_ref()
            .map(|bytes| allocate(bytes))
            .transpose()?;
        let image_memory = match data
            .image
            .as_ref()
            .map(|image| allocate(&image.dib))
            .transpose()
        {
            Ok(memory) => memory,
            Err(error) => {
                if let Some(memory) = text_memory {
                    let _ = GlobalFree(Some(memory));
                }
                return Err(error);
            }
        };
        if let Err(error) = OpenClipboard(Some(owner)) {
            if let Some(memory) = text_memory {
                let _ = GlobalFree(Some(memory));
            }
            if let Some(memory) = image_memory {
                let _ = GlobalFree(Some(memory));
            }
            return Err(error.to_string());
        }
        let _guard = ClipboardGuard;
        if expected_sequence.is_some_and(|sequence| sequence != GetClipboardSequenceNumber()) {
            if let Some(memory) = text_memory {
                let _ = GlobalFree(Some(memory));
            }
            if let Some(memory) = image_memory {
                let _ = GlobalFree(Some(memory));
            }
            return Err("Clipboard changed during selection; paste was cancelled.".into());
        }

        if let Err(error) = EmptyClipboard() {
            if let Some(memory) = text_memory {
                let _ = GlobalFree(Some(memory));
            }
            if let Some(memory) = image_memory {
                let _ = GlobalFree(Some(memory));
            }
            return Err(error.to_string());
        }
        let mut restored = 0;
        let mut requested = 0;
        if let Some(memory) = text_memory {
            requested += 1;
            if SetClipboardData(CF_UNICODETEXT, Some(HANDLE(memory.0))).is_ok() {
                restored += 1;
            } else {
                let _ = GlobalFree(Some(memory));
            }
        }
        if let Some(memory) = image_memory {
            requested += 1;
            let format = data.image.as_ref().unwrap().format;
            if SetClipboardData(format, Some(HANDLE(memory.0))).is_ok() {
                restored += 1;
            } else {
                let _ = GlobalFree(Some(memory));
            }
        }
        if restored == 0 {
            return Err("Could not write clipboard formats.".into());
        }
        Ok(WriteResult {
            sequence: GetClipboardSequenceNumber(),
            complete: restored == requested,
        })
    }
}

unsafe fn allocate(bytes: &[u8]) -> Result<HGLOBAL, String> {
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).map_err(|error| error.to_string())?;
        let pointer = GlobalLock(memory) as *mut u8;
        if pointer.is_null() {
            let _ = GlobalFree(Some(memory));
            return Err("Could not allocate clipboard memory.".into());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer, bytes.len());
        let _ = GlobalUnlock(memory);
        Ok(memory)
    }
}

pub unsafe fn snapshot_all() -> Result<ClipboardSnapshot, String> {
    unsafe {
        OpenClipboard(None).map_err(|error| error.to_string())?;
        let _guard = ClipboardGuard;
        let mut formats = Vec::new();
        let mut previous = 0;
        let mut total = 0_usize;
        loop {
            SetLastError(ERROR_SUCCESS);
            let format = EnumClipboardFormats(previous);
            if format == 0 {
                if GetLastError() != ERROR_SUCCESS {
                    return Err("Could not inspect all clipboard formats.".into());
                }
                break;
            }
            previous = format;
            if matches!(format, 2 | 3 | 9 | 14) {
                return Err(
                    "The previous clipboard has a format that cannot be safely restored.".into(),
                );
            }
            let handle = GetClipboardData(format)
                .map_err(|_| "Could not read a clipboard format.".to_string())?;
            let global = HGLOBAL(handle.0);
            let size = GlobalSize(global);
            if size == 0 || size > MAX_SNAPSHOT_BYTES.saturating_sub(total) {
                return Err("The previous clipboard cannot be safely preserved.".into());
            }
            let pointer = GlobalLock(global) as *const u8;
            if pointer.is_null() {
                return Err("Could not preserve a clipboard format.".into());
            }
            let bytes = std::slice::from_raw_parts(pointer, size).to_vec();
            let _ = GlobalUnlock(global);
            total += size;
            formats.push((format, bytes));
        }
        Ok(ClipboardSnapshot { formats })
    }
}

pub unsafe fn restore_all(
    snapshot: ClipboardSnapshot,
    expected_sequence: u32,
    owner: HWND,
) -> Result<RestoreOutcome, String> {
    unsafe {
        let mut allocated = Vec::with_capacity(snapshot.formats.len());
        for (format, bytes) in &snapshot.formats {
            match allocate(bytes) {
                Ok(memory) => allocated.push((*format, memory)),
                Err(error) => {
                    for (_, memory) in allocated {
                        let _ = GlobalFree(Some(memory));
                    }
                    return Err(error);
                }
            }
        }
        if let Err(error) = OpenClipboard(Some(owner)) {
            for (_, memory) in allocated {
                let _ = GlobalFree(Some(memory));
            }
            return Err(error.to_string());
        }
        let _guard = ClipboardGuard;
        if GetClipboardSequenceNumber() != expected_sequence {
            for (_, memory) in allocated {
                let _ = GlobalFree(Some(memory));
            }
            return Ok(RestoreOutcome::ChangedExternally);
        }
        if let Err(error) = EmptyClipboard() {
            for (_, memory) in allocated {
                let _ = GlobalFree(Some(memory));
            }
            return Err(error.to_string());
        }
        let mut complete = true;
        for (format, memory) in allocated {
            if SetClipboardData(format, Some(HANDLE(memory.0))).is_err() {
                complete = false;
                let _ = GlobalFree(Some(memory));
            }
        }
        Ok(RestoreOutcome::Restored {
            sequence: GetClipboardSequenceNumber(),
            complete,
        })
    }
}
