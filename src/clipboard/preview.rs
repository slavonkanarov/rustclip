use super::*;

pub(super) struct Preview {
    pub(super) hwnd: HWND,
    image: Option<image::RgbaImage>,
    text: String,
    number: Option<usize>,
    deadline: Option<Instant>,
}

impl Preview {
    pub(super) unsafe fn new() -> Self {
        Self {
            hwnd: unsafe {
                CreateWindowExW(
                    WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                    wide("STATIC").as_ptr(),
                    wide("").as_ptr(),
                    WS_POPUP,
                    0,
                    0,
                    520,
                    180,
                    null_mut(),
                    null_mut(),
                    GetModuleHandleW(null()),
                    null(),
                )
            },
            image: None,
            text: String::new(),
            number: None,
            deadline: None,
        }
    }
    pub(super) fn show(
        &mut self,
        clip: &Clip,
        target: HWND,
        timeout: u64,
        number: Option<usize>,
        language: crate::i18n::Language,
    ) {
        self.number = number;
        self.text = if clip.text.is_empty() {
            clip.title(language)
        } else {
            clip.text.chars().take(1800).collect()
        };
        self.image = image::load_from_memory(&clip.png).ok().map(|i| {
            if i.width() > 598 || i.height() > 398 {
                i.resize(598, 398, image::imageops::FilterType::Lanczos3)
                    .to_rgba8()
            } else {
                i.to_rgba8()
            }
        });
        unsafe {
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            GetMonitorInfoW(
                MonitorFromWindow(target, MONITOR_DEFAULTTONEAREST),
                &mut info,
            );
            let (w, h) = self.image.as_ref().map_or((520, 180), |img| {
                (img.width() as i32, img.height() as i32 + 42)
            });
            SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                info.rcWork.left + (info.rcWork.right - info.rcWork.left - w) / 2,
                info.rcWork.top + (info.rcWork.bottom - info.rcWork.top - h) / 2,
                w,
                h,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
        self.deadline = (timeout > 0).then(|| Instant::now() + Duration::from_millis(timeout));
        unsafe {
            let dc = GetDC(self.hwnd);
            self.paint(dc);
            ReleaseDC(self.hwnd, dc);
        }
    }
    pub(super) fn paint(&self, dc: HDC) {
        unsafe {
            let mut rect = RECT::default();
            GetClientRect(self.hwnd, &mut rect);
            let background = CreateSolidBrush(0x00201d1b);
            FillRect(dc, &rect, background);
            DeleteObject(background);

            SetBkMode(dc, TRANSPARENT as i32);
            let old_font = SelectObject(dc, GetStockObject(DEFAULT_GUI_FONT));
            SetTextColor(dc, 0x00c6b9ae);
            let heading = wide("RustClip");
            let margin = if self.image.is_some() { 8 } else { 24 };
            let mut header = RECT {
                left: margin,
                top: 12,
                right: rect.right - margin,
                bottom: 32,
            };
            DrawTextW(
                dc,
                heading.as_ptr(),
                -1,
                &mut header,
                DT_SINGLELINE | DT_VCENTER,
            );
            if let Some(number) = self.number {
                let label = wide(&format!("№{number}"));
                DrawTextW(
                    dc,
                    label.as_ptr(),
                    -1,
                    &mut header,
                    DT_SINGLELINE | DT_RIGHT | DT_VCENTER,
                );
            }

            let accent = CreateSolidBrush(0x00b77c52);
            let line = RECT {
                left: margin,
                top: 40,
                right: rect.right - margin,
                bottom: 41,
            };
            FillRect(dc, &line, accent);
            DeleteObject(accent);

            if self.image.is_none() {
                let body_font = CreateFontW(
                    -16,
                    0,
                    0,
                    0,
                    FW_NORMAL as i32,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET as u32,
                    0,
                    0,
                    0,
                    0,
                    wide("Segoe UI").as_ptr(),
                );
                let previous_font = SelectObject(dc, body_font);
                SetTextColor(dc, 0x00f3efeb);
                let content = wide(&self.text);
                let mut body = RECT {
                    left: 24,
                    top: 57,
                    right: rect.right - 24,
                    bottom: rect.bottom - 20,
                };
                DrawTextW(
                    dc,
                    content.as_ptr(),
                    -1,
                    &mut body,
                    DT_LEFT | DT_TOP | DT_WORDBREAK | DT_END_ELLIPSIS,
                );
                SelectObject(dc, previous_font);
                DeleteObject(body_font);
            }
            SelectObject(dc, old_font);
        }
        if let Some(img) = &self.image {
            unsafe {
                let mut bgra = img.as_raw().clone();
                for p in bgra.chunks_exact_mut(4) {
                    let alpha = p[3] as u16;
                    for (channel, background) in p[..3].iter_mut().zip([27, 29, 32]) {
                        *channel =
                            ((*channel as u16 * alpha + background * (255 - alpha)) / 255) as u8;
                    }
                    p.swap(0, 2);
                }
                let bmi = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: img.width() as i32,
                        biHeight: -(img.height() as i32),
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                StretchDIBits(
                    dc,
                    0,
                    42,
                    img.width() as i32,
                    img.height() as i32,
                    0,
                    0,
                    img.width() as i32,
                    img.height() as i32,
                    bgra.as_ptr().cast(),
                    &bmi,
                    DIB_RGB_COLORS,
                    SRCCOPY,
                );
            }
        }
    }
    pub(super) fn hide(&mut self) {
        unsafe {
            ShowWindow(self.hwnd, SW_HIDE);
        }
        self.image = None;
        self.deadline = None;
    }
    pub(super) fn tick(&mut self) {
        if self.deadline.is_some_and(|d| Instant::now() >= d) {
            self.hide();
        }
    }
}
