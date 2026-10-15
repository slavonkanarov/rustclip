use super::*;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PlatformError, PointerEventButton, WindowAdapter, WindowEvent};
use slint::{LogicalPosition, PhysicalSize, Rgb8Pixel};
use std::rc::Rc;

struct TestPlatform(Rc<MinimalSoftwareWindow>);
impl Platform for TestPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.0.clone())
    }
}

fn click(ui: &MainWindow, x: f32, y: f32) {
    let position = LogicalPosition::new(x, y);
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
}

fn snapshot(window: &MinimalSoftwareWindow, name: &str) {
    let size = window.size();
    let mut pixels = vec![Rgb8Pixel::default(); size.width as usize * size.height as usize];
    window.request_redraw();
    window.draw_if_needed(|renderer| {
        renderer.render(&mut pixels, size.width as usize);
    });
    let bytes: Vec<u8> = pixels.into_iter().flat_map(|p| [p.r, p.g, p.b]).collect();
    let directory = PathBuf::from("target/ui-review");
    std::fs::create_dir_all(&directory).unwrap();
    image::save_buffer(
        directory.join(format!("{name}.bmp")),
        &bytes,
        size.width,
        size.height,
        image::ColorType::Rgb8,
    )
    .unwrap();
}

#[test]
fn settings_navigation_and_double_click_use_real_ui_callbacks() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(TestPlatform(window.clone()))).unwrap();
    let ui = MainWindow::new().unwrap();
    let state = Arc::new(Mutex::new(UiState::new(Settings::default())));
    let data = Arc::new(Mutex::new(UiData {
        entries: Arc::new(
            [
                "Newest copied text",
                "Previous clipboard entry",
                "Another saved clip",
            ]
            .into_iter()
            .enumerate()
            .map(|(i, text)| Summary {
                id: (i + 1) as u128,
                preview: text.into(),
                detail: text.into(),
                search: text.to_lowercase(),
                kind: "TEXT CLIP",
                thumbnail: None,
            })
            .collect(),
        ),
        ..Default::default()
    }));
    let (worker, commands) = mpsc::sync_channel(8);
    let (monitor_sender, _monitor_receiver) = mpsc::channel();
    let monitor = MonitorHandle {
        window: 0,
        sender: monitor_sender,
    };
    wire_ui(
        &ui,
        &state,
        &data,
        &worker,
        &monitor,
        PathBuf::from("target/ui-review/test-settings.conf"),
    );
    render_shortcuts(&ui, &state);
    render_general_settings(&ui, &state);
    render_history(&ui, &state, &data);
    ui.show().unwrap();
    window.set_size(PhysicalSize::new(940, 680));
    snapshot(&window, "history");
    click(&ui, 866.0, 36.0);
    assert!(
        ui.get_settings_open(),
        "the visible Settings button must open settings"
    );
    click(&ui, 95.0, 138.0);
    assert_eq!(
        ui.get_settings_section(),
        1,
        "keyboard settings must be reachable"
    );
    snapshot(&window, "shortcuts");
    window.set_size(PhysicalSize::new(760, 560));
    snapshot(&window, "shortcuts-minimum");
    click(&ui, 95.0, 92.0);
    assert_eq!(ui.get_settings_section(), 0);
    snapshot(&window, "general-minimum");
    ui.invoke_cancel_settings();
    window.set_size(PhysicalSize::new(940, 680));
    lock(&state).last_external_window = 42;
    snapshot(&window, "history");
    // Two real pointer clicks must retain the row and emit exactly one paste request.
    click(&ui, 240.0, 204.0);
    std::thread::sleep(Duration::from_millis(80));
    click(&ui, 240.0, 204.0);
    let command = commands
        .try_recv()
        .expect("double-click must request paste");
    assert!(matches!(
        command,
        WorkerCommand::Paste {
            id: 2,
            target: 42,
            expected_sequence: None
        }
    ));
    assert!(
        commands.try_recv().is_err(),
        "double-click must paste only once"
    );
}
