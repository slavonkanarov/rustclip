mod preview;
use preview::Preview;

use crate::{
    hotkeys,
    model::{Clip, Settings, now},
};
use std::{
    ptr::{null, null_mut},
    sync::mpsc::{Receiver, Sender},
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{DataExchange::*, LibraryLoader::*, Memory::*, Threading::*},
    UI::{Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
};

pub enum Command {
    Copy(Clip, bool, Option<usize>),
    Paste(Clip, bool, Option<usize>, Option<usize>),
    Secret(Clip, bool, usize),
    Settings(Settings),
    HidePreview,
    Quit,
}
pub enum Event {
    Captured(Clip),
    ActiveApp(String),
    Open(String),
    Previous(usize),
    Next(usize),
    PlainPaste(usize),
    Error(String),
    SecretCopied(usize),
    Quit,
}
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn tray_icon(instance: HINSTANCE) -> HICON {
    let mask = [0u8; 32];
    let mut pixels = image::load_from_memory(include_bytes!("../assets/icon.png"))
        .expect("valid app icon")
        .resize_exact(16, 16, image::imageops::FilterType::Lanczos3)
        .to_rgba8()
        .into_raw();
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    unsafe { CreateIcon(instance, 16, 16, 1, 32, mask.as_ptr(), pixels.as_ptr()) }
}

const BLOCKED: &[&str] = &[
    "CF_ENHMETAFILE",
    "CF_METAFILEPICT",
    "CF_SYLK",
    "DataObject",
    "Embed Source",
    "Native",
    "OwnerLink",
    "ObjectLink",
    "Object Descriptor",
    "Link Source",
    "Link Source Descriptor",
    "Link",
    "Ole Private Data",
    "BIFF",
    "BIFF5",
    "BIFF8",
    "BIFF12",
    "HyperlinkWordBkmk",
    "Windows Bitmap",
    "GDIMetaFile",
    "Star Embed Source (XML)",
    "image/x-bmp",
    "image/x-MS-bmp",
    "image/png",
    "image/x-icon",
    "image/x-ico",
    "image/x-win-bitmap",
    "image/tiff",
    "image/jpeg",
    "WebKit Smart Paste Format",
    "Chromium Source tag Format",
    "Shell IDList Array",
    "DataObjectAttributes",
    "UIDisplayed",
    "DropDescription",
    "FileName",
    "FileNameW",
    "Shell Object Offsets",
    "AsyncFlag",
    "Preferred DropEffect",
];

unsafe extern "system" fn procedure(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_APP + 1 {
        unsafe { PostMessageW(hwnd, WM_APP + 2, w, l) };
        return 0;
    }
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}
struct ClipboardLock;
impl Drop for ClipboardLock {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}
fn lock(hwnd: HWND) -> Result<ClipboardLock, String> {
    for _ in 0..10 {
        if unsafe { OpenClipboard(hwnd) } != 0 {
            return Ok(ClipboardLock);
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err("Буфер обмена занят другим приложением".into())
}
fn registered(name: &str) -> u32 {
    unsafe { RegisterClipboardFormatW(wide(name).as_ptr()) }
}
fn executable(hwnd: HWND) -> String {
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return String::new();
        }
        let mut path = vec![0u16; 32768];
        let mut size = path.len() as u32;
        let success = QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut size);
        CloseHandle(process);
        if success == 0 {
            String::new()
        } else {
            String::from_utf16_lossy(&path[..size as usize])
        }
    }
}
fn excluded(path: &str, list: &str) -> bool {
    let name = path.rsplit('\\').next().unwrap_or(path);
    list.lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .any(|s| {
            path.eq_ignore_ascii_case(s)
                || name.eq_ignore_ascii_case(s)
                || (!s.contains('.')
                    && name.rsplit_once('.').is_some_and(|(stem, ext)| {
                        ext.eq_ignore_ascii_case("exe") && stem.eq_ignore_ascii_case(s)
                    }))
        })
}
pub fn app_matches(path: &str, app: &str) -> bool {
    !app.is_empty() && excluded(path, app)
}
fn format_name(id: u32) -> String {
    if id < 0xc000 {
        return id.to_string();
    }
    let mut text = [0u16; 512];
    let len = unsafe { GetClipboardFormatNameW(id, text.as_mut_ptr(), text.len() as i32) };
    String::from_utf16_lossy(&text[..len.max(0) as usize])
}
fn read_bytes(id: u32) -> Option<Vec<u8>> {
    unsafe {
        let handle = GetClipboardData(id);
        if handle.is_null() {
            return None;
        }
        let len = GlobalSize(handle);
        if len == 0 || len > 32 * 1024 * 1024 {
            return None;
        }
        let ptr = GlobalLock(handle);
        if ptr.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(ptr as *const u8, len).to_vec();
        GlobalUnlock(handle);
        Some(bytes)
    }
}
fn file_list(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 20 {
        return None;
    }
    let offset = u32::from_le_bytes(bytes[..4].try_into().ok()?) as usize;
    if offset < 20 || offset >= bytes.len() {
        return None;
    }
    let unicode = u32::from_le_bytes(bytes[16..20].try_into().ok()?) != 0;
    if !unicode {
        return None;
    }
    let words: Vec<_> = bytes[offset..]
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    Some(
        words
            .split(|v| *v == 0)
            .take_while(|part| !part.is_empty())
            .map(String::from_utf16_lossy)
            .collect::<Vec<_>>()
            .join("\n"),
    )
}
fn capture(hwnd: HWND, settings: &Settings, allow_own: bool) -> Result<Option<Clip>, String> {
    let source = executable(unsafe { GetClipboardOwner() });
    if !allow_own && excluded(&source, &settings.excluded_capture) {
        return Ok(None);
    }
    let guard = lock(hwnd)?;
    unsafe {
        for name in [
            "Clipboard Viewer Ignore",
            "ExcludeClipboardContentFromMonitorProcessing",
        ] {
            if IsClipboardFormatAvailable(registered(name)) != 0 {
                return Ok(None);
            }
        }
        if !allow_own && IsClipboardFormatAvailable(registered("RustClip/Owner")) != 0 {
            return Ok(None);
        }
    }
    if read_bytes(registered("CanIncludeInClipboardHistory")).as_deref() == Some(&[0, 0, 0, 0]) {
        return Ok(None);
    }
    let mut clip = Clip {
        id: now(),
        ..Default::default()
    };
    let mut id = 0;
    let mut total = 0;
    loop {
        id = unsafe { EnumClipboardFormats(id) };
        if id == 0 {
            break;
        }
        let name = format_name(id);
        // Only known HGLOBAL-backed formats; never reinterpret GDI handles as memory.
        if !matches!(id, 1 | 7 | 8 | 13 | 15 | 16 | 17)
            && !matches!(name.as_str(), "HTML Format" | "Rich Text Format" | "PNG")
        {
            continue;
        }
        if BLOCKED.iter().any(|f| f.eq_ignore_ascii_case(&name))
            || settings
                .excluded_formats
                .lines()
                .any(|f| f.trim().eq_ignore_ascii_case(&name))
        {
            continue;
        }
        if let Some(bytes) = read_bytes(id) {
            total += bytes.len();
            if total > 64 * 1024 * 1024 {
                return Err("Клип превышает лимит 64 MiB".into());
            }
            if id == 13 {
                if bytes.len() > settings.max_text_kb.saturating_mul(1024) {
                    return Ok(None);
                }
                let chars: Vec<u16> = bytes
                    .chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .take_while(|c| *c != 0)
                    .collect();
                clip.text = String::from_utf16_lossy(&chars);
            }
            if id == 15 && clip.text.is_empty() {
                clip.text = file_list(&bytes).unwrap_or_default();
            }
            clip.formats.push((name, bytes));
        }
    }
    let sequence = unsafe { GetClipboardSequenceNumber() };
    let owner = unsafe { GetClipboardOwner() };
    let mut title = [0u16; 1024];
    let len = unsafe { GetWindowTextW(owner, title.as_mut_ptr(), 1024) };
    clip.source = format!(
        "{} — {}",
        source,
        String::from_utf16_lossy(&title[..len.max(0) as usize])
    );
    drop(guard);
    if clip
        .formats
        .iter()
        .any(|(n, _)| matches!(n.as_str(), "8" | "17" | "PNG"))
    {
        if let Ok(mut cb) = arboard::Clipboard::new() {
            if let Ok(img) = cb.get_image() {
                if let Some(buffer) = image::RgbaImage::from_raw(
                    img.width as u32,
                    img.height as u32,
                    img.bytes.into_owned(),
                ) {
                    let mut cursor = std::io::Cursor::new(Vec::new());
                    if buffer
                        .write_to(&mut cursor, image::ImageFormat::Png)
                        .is_ok()
                    {
                        clip.png = cursor.into_inner();
                    }
                }
            }
        }
    }
    if unsafe { GetClipboardSequenceNumber() } != sequence {
        return Ok(None);
    }
    Ok((!clip.formats.is_empty()).then_some(clip))
}
pub fn text_clip(text: String) -> Clip {
    let bytes = text
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    Clip {
        id: now(),
        text,
        formats: vec![("13".into(), bytes)],
        ..Default::default()
    }
}
fn publish(hwnd: HWND, clip: &Clip, plain: bool) -> Result<u32, String> {
    let plain_clip;
    let formats = if plain {
        if clip.text.is_empty() {
            return Err("Нет текстового представления".into());
        }
        plain_clip = text_clip(clip.text.clone());
        &plain_clip.formats
    } else {
        &clip.formats
    };
    if formats.is_empty() {
        return Err("Клип пуст".into());
    }
    let guard = lock(hwnd)?;
    unsafe {
        if EmptyClipboard() == 0 {
            return Err("Не удалось очистить Clipboard для записи".into());
        }
        for (name, bytes) in formats
            .iter()
            .chain(std::iter::once(&("RustClip/Owner".into(), vec![1])))
        {
            let id = name.parse().unwrap_or_else(|_| registered(name));
            let handle = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
            if handle.is_null() {
                return Err("Недостаточно памяти".into());
            }
            let ptr = GlobalLock(handle);
            if ptr.is_null() {
                GlobalFree(handle);
                return Err("GlobalLock failed".into());
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
            GlobalUnlock(handle);
            if SetClipboardData(id, handle).is_null() {
                GlobalFree(handle);
                return Err(format!("Не удалось записать формат {name}"));
            }
        }
        drop(guard);
        Ok(GetClipboardSequenceNumber())
    }
}
fn released() -> bool {
    unsafe {
        [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN]
            .iter()
            .all(|k| GetAsyncKeyState(*k as i32) >= 0)
    }
}
fn hotkey_held(key: u32, modifiers: u32) -> bool {
    unsafe {
        GetAsyncKeyState(key as i32) < 0
            && [
                (MOD_CONTROL, VK_CONTROL),
                (MOD_SHIFT, VK_SHIFT),
                (MOD_ALT, VK_MENU),
            ]
            .iter()
            .all(|&(modifier, vk)| modifiers & modifier == 0 || GetAsyncKeyState(vk as i32) < 0)
            && (modifiers & MOD_WIN == 0
                || GetAsyncKeyState(VK_LWIN as i32) < 0
                || GetAsyncKeyState(VK_RWIN as i32) < 0)
    }
}
fn paste(target: HWND, ctrl_v: bool) -> Result<(), String> {
    unsafe {
        if target.is_null() || IsWindow(target) == 0 {
            return Err("Не удалось вернуть фокус целевому окну".into());
        }
        if GetForegroundWindow() != target {
            let current_thread = GetCurrentThreadId();
            let foreground_thread = GetWindowThreadProcessId(GetForegroundWindow(), null_mut());
            let attached = foreground_thread != 0
                && foreground_thread != current_thread
                && AttachThreadInput(current_thread, foreground_thread, 1) != 0;
            let focused = SetForegroundWindow(target) != 0;
            if attached {
                AttachThreadInput(current_thread, foreground_thread, 0);
            }
            if !focused {
                return Err("Не удалось вернуть фокус целевому окну".into());
            }
        }
        for _ in 0..20 {
            if GetForegroundWindow() == target {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        if GetForegroundWindow() != target {
            return Err("Целевое окно не получило фокус для вставки".into());
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while !released() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        if !released() {
            return Err("Модификаторы не отпущены для вставки".into());
        }
        let (modifier, key) = if ctrl_v {
            (VK_CONTROL, 0x56)
        } else {
            (VK_SHIFT, VK_INSERT)
        };
        let key_flags = if ctrl_v { 0 } else { KEYEVENTF_EXTENDEDKEY };
        let inputs: Vec<INPUT> = [
            (modifier, 0),
            (key, key_flags),
            (key, key_flags | KEYEVENTF_KEYUP),
            (modifier, KEYEVENTF_KEYUP),
        ]
        .into_iter()
        .map(|(vk, flags)| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    dwFlags: flags,
                    ..Default::default()
                },
            },
        })
        .collect();
        if SendInput(4, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32) != 4 {
            return Err(
                "SendInput: вставка заблокирована (возможно, права приложения выше)".into(),
            );
        }
    }
    Ok(())
}

pub fn start(
    tx: Sender<Event>,
    rx: Receiver<Command>,
    settings: Settings,
    ctx: eframe::egui::Context,
    initial_target: usize,
) {
    thread::spawn(move || unsafe {
        let mut settings = settings;
        let class = wide("RustClip.Listener");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: GetModuleHandleW(null()),
            lpszClassName: class.as_ptr(),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class.as_ptr(),
            wide("RustClip.Listener").as_ptr(),
            WS_POPUP,
            -32000,
            -32000,
            1,
            1,
            null_mut(),
            null_mut(),
            wc.hInstance,
            null(),
        );
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        AddClipboardFormatListener(hwnd);
        let icon = tray_icon(wc.hInstance);
        let mut tray = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_APP + 1,
            hIcon: icon,
            ..Default::default()
        };
        for (a, b) in tray
            .szTip
            .iter_mut()
            .zip(wide("RustClip — история буфера обмена"))
        {
            *a = b;
        }
        if Shell_NotifyIconW(NIM_ADD, &tray) == 0 {
            let _ = tx.send(Event::Error("Не удалось создать значок в трее".into()));
            ctx.request_repaint();
        } else {
            tray.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            Shell_NotifyIconW(NIM_SETVERSION, &tray);
        }
        let demo = std::env::args().any(|s| s == "--demo");
        let mut registered = [false; 4];
        if !demo {
            register_hotkeys(hwnd, &settings, &mut registered, &tx);
        }
        let mut target = initial_target as HWND;
        let mut pending: Option<(Clip, bool, Option<Clip>, HWND, bool)> = None;
        let mut secret_sequences = Vec::new();
        let mut restore: Option<(Clip, u32, Instant)> = None;
        let mut preview = Preview::new();
        let mut dirty = false;
        let mut last_sequence = GetClipboardSequenceNumber();
        let mut last_foreground = null_mut();
        let mut repeat: Option<(usize, u32, u32, usize, Instant)> = None;
        'run: loop {
            let foreground = GetForegroundWindow();
            let mut pid = 0;
            GetWindowThreadProcessId(foreground, &mut pid);
            if !foreground.is_null() && pid != std::process::id() {
                target = foreground;
                if foreground != last_foreground {
                    last_foreground = foreground;
                    let path = executable(foreground);
                    if !path.is_empty() {
                        let _ = tx.send(Event::ActiveApp(path));
                        ctx.request_repaint();
                    }
                }
            }
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                if msg.hwnd == preview.hwnd && msg.message == WM_PAINT {
                    let mut ps: PAINTSTRUCT = std::mem::zeroed();
                    let dc = BeginPaint(preview.hwnd, &mut ps);
                    preview.paint(dc);
                    EndPaint(preview.hwnd, &ps);
                    continue;
                }
                if msg.message == WM_CLIPBOARDUPDATE {
                    dirty = true;
                }
                if msg.message == WM_HOTKEY
                    && !excluded(
                        &executable(GetForegroundWindow()),
                        &settings.excluded_hotkeys,
                    )
                {
                    let active = GetForegroundWindow();
                    let destination = if active.is_null() || active == hwnd {
                        target
                    } else {
                        active
                    };
                    let event = match msg.wParam {
                        1 => Event::Open(executable(destination)),
                        2 => Event::Previous(destination as usize),
                        3 => Event::Next(destination as usize),
                        _ => Event::PlainPaste(destination as usize),
                    };
                    let _ = tx.send(event);
                    ctx.request_repaint();
                    if msg.wParam == 2 || msg.wParam == 3 {
                        if let Ok(binding) = hotkeys::parse(if msg.wParam == 2 {
                            &settings.hotkey_previous
                        } else {
                            &settings.hotkey_next
                        }) {
                            repeat = Some((
                                msg.wParam,
                                binding.key,
                                binding.modifiers,
                                destination as usize,
                                Instant::now() + Duration::from_millis(400),
                            ));
                        }
                    }
                }
                if msg.message == WM_APP + 2 {
                    let action = msg.lParam as u32 & 0xffff;
                    if action == WM_LBUTTONUP || action == NIN_SELECT {
                        let _ = tx.send(Event::Open(String::new()));
                        ctx.request_repaint();
                    }
                    if action == WM_RBUTTONUP || action == WM_CONTEXTMENU {
                        let menu = CreatePopupMenu();
                        AppendMenuW(menu, MF_STRING, 1, wide("Открыть").as_ptr());
                        AppendMenuW(menu, MF_STRING, 2, wide("Выход").as_ptr());
                        let mut p = POINT::default();
                        GetCursorPos(&mut p);
                        if SetForegroundWindow(hwnd) == 0 {
                            let _ =
                                tx.send(Event::Error("Не удалось активировать меню трея".into()));
                        }
                        let choice = TrackPopupMenu(
                            menu,
                            TPM_RETURNCMD | TPM_RIGHTBUTTON,
                            p.x,
                            p.y,
                            0,
                            hwnd,
                            null(),
                        );
                        DestroyMenu(menu);
                        PostMessageW(hwnd, WM_NULL, 0, 0);
                        match choice {
                            1 => {
                                let _ = tx.send(Event::Open(String::new()));
                            }
                            2 => {
                                let _ = tx.send(Event::Quit);
                            }
                            _ => {}
                        }
                        if choice != 0 {
                            ctx.request_repaint();
                        }
                    }
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            while let Ok(command) = rx.try_recv() {
                match command {
                    Command::Quit => break 'run,
                    Command::Settings(s) => {
                        repeat = None;
                        if !demo && hotkeys::validate(&s).is_ok() {
                            for (id, active) in registered.iter_mut().enumerate() {
                                if *active {
                                    UnregisterHotKey(hwnd, id as i32 + 1);
                                    *active = false;
                                }
                            }
                            register_hotkeys(hwnd, &s, &mut registered, &tx);
                        }
                        settings = s;
                    }
                    Command::HidePreview => preview.hide(),
                    Command::Copy(clip, plain, number) => {
                        if let Err(e) = publish(hwnd, &clip, plain) {
                            let _ = tx.send(Event::Error(e));
                        }
                        preview.show(&clip, target, settings.preview_ms, number);
                    }
                    Command::Secret(clip, should_paste, index) => {
                        if should_paste {
                            let old = capture(hwnd, &settings, true).ok().flatten();
                            pending = Some((clip, true, old, target, true));
                        } else {
                            match publish(hwnd, &clip, true) {
                                Ok(seq) => {
                                    secret_sequences.push(seq);
                                    let _ = tx.send(Event::SecretCopied(index));
                                }
                                Err(e) => {
                                    let _ = tx.send(Event::Error(e));
                                }
                            }
                        }
                    }
                    Command::Paste(clip, plain, destination, number) => {
                        let paste_target = destination.map_or(target, |handle| handle as HWND);
                        let old = if settings.restore {
                            capture(hwnd, &settings, true).ok().flatten()
                        } else {
                            None
                        };
                        preview.show(&clip, paste_target, settings.preview_ms, number);
                        pending = Some((clip, plain, old, paste_target, false));
                    }
                }
            }
            if let Some((id, key, modifiers, destination, next)) = repeat.as_mut() {
                if !hotkey_held(*key, *modifiers) {
                    repeat = None;
                } else if Instant::now() >= *next {
                    let _ = tx.send(if *id == 2 {
                        Event::Previous(*destination)
                    } else {
                        Event::Next(*destination)
                    });
                    ctx.request_repaint();
                    *next = Instant::now() + Duration::from_millis(120);
                }
            }
            if released() {
                if let Some((clip, plain, old, paste_target, secret)) = pending.take() {
                    preview.hide();
                    match publish(hwnd, &clip, plain) {
                        Ok(seq) => {
                            if secret {
                                secret_sequences.push(seq);
                            }
                            thread::sleep(Duration::from_millis(settings.paste_delay.min(2000)));
                            match paste(paste_target, settings.ctrl_v) {
                                Ok(()) => {}
                                Err(e) => {
                                    let _ = tx.send(Event::Error(e));
                                }
                            }
                            if let Some(old) = old {
                                restore = Some((old, seq, Instant::now()));
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Event::Error(e));
                        }
                    }
                }
            }
            if restore.as_ref().is_some_and(|r| {
                r.2.elapsed() > Duration::from_millis(settings.paste_delay.max(500))
            }) {
                if let Some((old, seq, _)) = restore.take() {
                    if GetClipboardSequenceNumber() == seq {
                        if let Ok(seq) = publish(hwnd, &old, false) {
                            secret_sequences.push(seq);
                        }
                    }
                }
            }
            if dirty {
                dirty = false;
                let seq = GetClipboardSequenceNumber();
                if secret_sequences.contains(&seq) {
                    secret_sequences.retain(|known| *known != seq);
                } else if seq != last_sequence && settings.monitor {
                    match capture(hwnd, &settings, false) {
                        Ok(Some(clip)) => {
                            let _ = tx.send(Event::Captured(clip));
                            ctx.request_repaint();
                        }
                        Ok(None) => {}
                        Err(e) => {
                            let _ = tx.send(Event::Error(e));
                        }
                    }
                }
                last_sequence = seq;
            }
            if GetAsyncKeyState(VK_ESCAPE as i32) < 0 {
                preview.hide();
            }
            preview.tick();
            thread::sleep(Duration::from_millis(20));
        }
        preview.hide();
        Shell_NotifyIconW(NIM_DELETE, &tray);
        DestroyIcon(icon);
        RemoveClipboardFormatListener(hwnd);
        for (id, active) in registered.iter().enumerate() {
            if *active {
                UnregisterHotKey(hwnd, id as i32 + 1);
            }
        }
        DestroyWindow(hwnd);
    });
}

unsafe fn register_hotkeys(
    hwnd: HWND,
    settings: &Settings,
    registered: &mut [bool; 4],
    tx: &Sender<Event>,
) {
    if let Err(error) = hotkeys::validate(settings) {
        let _ = tx.send(Event::Error(error));
        return;
    }
    for (id, value) in hotkeys::bindings(settings) {
        let hotkey = hotkeys::parse(value).expect("validated hotkey");
        if unsafe { RegisterHotKey(hwnd, id, hotkey.modifiers, hotkey.key) } != 0 {
            registered[id as usize - 1] = true;
        } else {
            let _ = tx.send(Event::Error(format!(
                "Горячая клавиша {value} занята другой программой"
            )));
        }
    }
}

#[cfg(test)]
mod tests;
