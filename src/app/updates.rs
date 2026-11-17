use super::App;
use crate::{
    model,
    updates::{self, Event},
};
use eframe::egui;
use std::time::Duration;

impl App {
    pub(super) fn poll_updates(&mut self, ctx: &egui::Context) {
        let demo = std::env::args_os().any(|arg| arg == "--demo");
        if !demo
            && !self.quitting
            && self.settings.check_updates
            && !self.updates.busy
            && !self.updates.install_requested
            && self
                .updates
                .last_check
                .is_none_or(|time| time.elapsed() >= Duration::from_secs(6 * 3600))
        {
            self.updates.check(ctx, true);
        }
        while let Some(event) = self.updates.poll() {
            match event {
                Event::Releases(result) => {
                    self.updates.busy = false;
                    match result {
                        Ok(releases) => {
                            self.updates.releases = releases;
                            self.updates.selected = 0;
                            if !self.draft.prerelease_updates {
                                self.updates.selected = self
                                    .updates
                                    .releases
                                    .iter()
                                    .position(|r| !r.prerelease)
                                    .unwrap_or(0);
                            }
                            self.updates.message = "Release list updated".into();
                            if let Some(release) = self
                                .updates
                                .available(self.settings.prerelease_updates)
                                .cloned()
                            {
                                self.updates.message = format!(
                                    "{}: {}",
                                    self.settings.language.text("Update available"),
                                    release.tag_name
                                );
                                if self.updates.automatic
                                    && self.settings.check_updates
                                    && self.settings.auto_update
                                {
                                    self.updates.download(release, ctx);
                                }
                            }
                        }
                        Err(error) => {
                            self.updates.message = format!(
                                "{}: {error}",
                                self.settings.language.text("Update check failed")
                            );
                            crate::diagnostics::log(&self.updates.message);
                        }
                    }
                }
                Event::Progress(done, total) => self.updates.progress = Some((done, total)),
                Event::Downloaded(result) => {
                    self.updates.busy = false;
                    self.updates.progress = None;
                    match result {
                        Ok(downloaded) => {
                            self.updates.downloaded = Some(downloaded);
                            self.updates.message =
                                "Download verified; preparing installation…".into();
                        }
                        Err(error) => {
                            self.updates.automatic = false;
                            self.updates.install_requested = false;
                            self.updates.message = format!(
                                "{}: {error}",
                                self.settings.language.text("Download failed")
                            );
                            crate::diagnostics::log(&self.updates.message);
                        }
                    }
                }
            }
        }
        if (self.updates.install_requested
            || (self.updates.automatic
                && self.settings.check_updates
                && self.settings.auto_update
                && !self.settings_open
                && self.updates.downloaded.as_ref().is_some_and(|d| {
                    d.release.newer() && (!d.release.prerelease || self.settings.prerelease_updates)
                })))
            && self.updates.downloaded.is_some()
            && !self.updates.busy
            && self.can_install_update()
        {
            self.install_update(ctx);
        }
    }

    fn can_install_update(&self) -> bool {
        !self.quitting
            && self.editor.is_none()
            && self.secret_editor.is_none()
            && self.folder_editor.is_none()
            && !self.unlock_open
            && self.draft == self.settings
    }

    fn install_update(&mut self, ctx: &egui::Context) {
        self.updates.automatic = false;
        self.updates.install_requested = false;
        let Some(downloaded) = &self.updates.downloaded else {
            return;
        };
        let mut settings = self.settings.clone();
        // Keep the chosen version until the user explicitly enables automatic installation again.
        if !downloaded.release.newer() {
            settings.auto_update = false;
        }
        let result = model::write_json(&model::directory().join("settings.json"), &settings)
            .and_then(|()| updates::install(downloaded, self.updates.installed.as_deref()));
        match result {
            Ok(()) => {
                self.settings = settings;
                self.draft = self.settings.clone();
                self.quitting = true;
                self.settings_open = false;
                ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
            }
            Err(error) => self.updates.message = error,
        }
    }

    pub(super) fn updates_ui(&mut self, ui: &mut egui::Ui) {
        let language = self.draft.language;
        ui.label(format!(
            "{}: {}",
            language.text("Current version"),
            updates::VERSION
        ));
        ui.checkbox(
            &mut self.draft.check_updates,
            language.text("Check for updates automatically"),
        );
        ui.add_enabled_ui(self.draft.check_updates, |ui| {
            ui.checkbox(
                &mut self.draft.auto_update,
                language.text("Download, install, and restart automatically"),
            );
        });
        ui.checkbox(
            &mut self.draft.prerelease_updates,
            language.text("Include prereleases"),
        );
        ui.label(language.text("Automatic installation waits until settings and editors are closed. MSI updates require Windows administrator approval."));
        if ui
            .add_enabled(
                !self.updates.busy,
                egui::Button::new(language.text("Check for updates")),
            )
            .clicked()
        {
            self.updates.check(ui.ctx(), false);
        }
        ui.label(language.message(&self.updates.message));
        if self.updates.busy && ui.button(language.text("Cancel")).clicked() {
            self.updates.cancel();
        }
        if let Some((done, total)) = self.updates.progress {
            ui.add(egui::ProgressBar::new(done as f32 / total as f32).show_percentage());
        }
        ui.separator();
        let selected = self.updates.releases.get(self.updates.selected);
        egui::ComboBox::from_id_salt("update_version")
            .selected_text(
                selected.map_or(language.text("Select a version"), |r| r.tag_name.as_str()),
            )
            .show_ui(ui, |ui| {
                for (index, release) in self.updates.releases.iter().enumerate() {
                    if !release.prerelease || self.draft.prerelease_updates {
                        ui.selectable_value(&mut self.updates.selected, index, &release.tag_name);
                    }
                }
            });
        if let Some(release) = self.updates.releases.get(self.updates.selected).cloned()
            && (!release.prerelease || self.draft.prerelease_updates)
        {
            if let Some(body) = &release.body {
                egui::CollapsingHeader::new(language.text("Release notes")).show(ui, |ui| {
                    ui.label(body);
                });
            }
            if ui
                .add_enabled(
                    !self.updates.busy && self.can_install_update(),
                    egui::Button::new(language.text("Update")),
                )
                .clicked()
            {
                self.updates.automatic = false;
                self.updates.install_requested = true;
                self.updates.download(release, ui.ctx());
            }
        }
        if !self.can_install_update() {
            ui.label(language.text("Save settings and close editors before installing."));
        }
        ui.label(language.text("RustClip will close, back up history, settings, and encrypted secrets, install the selected version, and restart. Older versions may not support newer data formats. Installing an older version disables automatic installation."));
    }
}
