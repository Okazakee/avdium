//! The Settings screen: locations, appearance, logging and provider choice.

use crate::app::{AvdiumApp, PathTarget};
use crate::settings::{LogLevel, ProviderPreference, Theme};
use crate::ui::{banner, copyable, disabled_reason, key_value, title, tokens};

/// Render the screen.
pub fn show(ui: &mut egui::Ui, app: &mut AvdiumApp) {
    title(
        ui,
        "Settings",
        "Avdium stores its own settings in the XDG config directory. Your SDK and AVDs stay where they are.",
    );

    if !app.settings_warnings.is_empty() {
        for warning in app.settings_warnings.clone() {
            banner(ui, ui.visuals().warn_fg_color, "Settings", &warning);
            ui.add_space(tokens::SPACING_XS);
        }
        ui.add_space(tokens::SPACING_XS);
    }

    ui.label(egui::RichText::new("Locations").strong());
    ui.add_space(tokens::SPACING_XS);

    path_row(
        ui,
        app,
        PathTarget::Sdk,
        "SDK root",
        "The Android SDK. Existing installations are used as-is and never upgraded automatically.",
    );
    path_row(
        ui,
        app,
        PathTarget::AvdHome,
        "AVD home",
        "Where device definitions live. It is read from the filesystem, so discovery works even with no SDK.",
    );

    ui.add_space(tokens::SPACING_S);
    ui.label(egui::RichText::new("Appearance and logging").strong());
    ui.add_space(tokens::SPACING_XS);
    egui::Grid::new("settings-general")
        .num_columns(2)
        .spacing(egui::vec2(tokens::SPACING_M, tokens::SPACING_XS))
        .show(ui, |ui| {
            ui.label(egui::RichText::new("Theme").size(tokens::TEXT_SMALL).weak());
            egui::ComboBox::from_id_salt("theme")
                .selected_text(app.settings.theme.label())
                .show_ui(ui, |ui| {
                    for theme in Theme::ALL {
                        ui.selectable_value(&mut app.settings.theme, theme, theme.label());
                    }
                });
            ui.end_row();

            ui.label(
                egui::RichText::new("Log level")
                    .size(tokens::TEXT_SMALL)
                    .weak(),
            );
            egui::ComboBox::from_id_salt("log-level")
                .selected_text(app.settings.log_level.label())
                .show_ui(ui, |ui| {
                    for level in LogLevel::ALL {
                        ui.selectable_value(&mut app.settings.log_level, level, level.label());
                    }
                });
            ui.end_row();

            ui.label(
                egui::RichText::new("Provider")
                    .size(tokens::TEXT_SMALL)
                    .weak(),
            );
            egui::ComboBox::from_id_salt("provider")
                .selected_text(app.settings.provider.label())
                .show_ui(ui, |ui| {
                    for preference in ProviderPreference::ALL {
                        ui.selectable_value(
                            &mut app.settings.provider,
                            preference,
                            preference.label(),
                        );
                    }
                });
            ui.end_row();

            ui.label(
                egui::RichText::new("Network checks")
                    .size(tokens::TEXT_SMALL)
                    .weak(),
            );
            ui.checkbox(
                &mut app.settings.allow_network_checks,
                "Allow reachability checks when a setup step needs them",
            );
            ui.end_row();
        });
    ui.add_space(tokens::SPACING_XS);
    disabled_reason(
        ui,
        "The provider choice is advanced on purpose. It is not part of onboarding, because an \
         existing SDK works without it.",
    );

    ui.add_space(tokens::SPACING_M);
    ui.horizontal(|ui| {
        if ui.button("Save settings").clicked() {
            app.save_settings();
        }
        if ui.button("Revert").clicked() {
            app.reload_settings();
        }
    });

    ui.add_space(tokens::SPACING_M);
    ui.separator();
    ui.label(egui::RichText::new("Resolved configuration").strong());
    ui.add_space(tokens::SPACING_XS);
    egui::Grid::new("settings-resolved")
        .num_columns(2)
        .spacing(egui::vec2(tokens::SPACING_M, tokens::SPACING_XS))
        .show(ui, |ui| {
            key_value(ui, "SDK root", &app.paths.sdk_summary());
            key_value(
                ui,
                "AVD home",
                &format!(
                    "{} (from {})",
                    app.paths.avd_home.display(),
                    app.paths.avd_home_origin.label()
                ),
            );
            key_value(
                ui,
                "Android user home",
                &app.paths.android_user_home.display().to_string(),
            );
            key_value(
                ui,
                "Emulator home",
                &app.paths.emulator_home.display().to_string(),
            );
            key_value(
                ui,
                "Settings file",
                &app.dirs.settings_file().display().to_string(),
            );
            key_value(ui, "Logs", &app.dirs.logs_dir().display().to_string());
            key_value(ui, "Detected providers", &app.providers.summary());
        });

    if !app.paths.conflicts.is_empty() {
        ui.add_space(tokens::SPACING_S);
        for conflict in &app.paths.conflicts {
            banner(
                ui,
                ui.visuals().warn_fg_color,
                "Conflicting configuration",
                &conflict.summary(),
            );
            ui.add_space(tokens::SPACING_XS);
        }
        disabled_reason(
            ui,
            "Conflicts are shown rather than resolved silently. Avdium never edits shell startup \
             files, and changing the SDK here selects a location; it does not move or rewrite it.",
        );
    }

    if !app.paths.warnings.is_empty() {
        ui.add_space(tokens::SPACING_S);
        for warning in &app.paths.warnings {
            ui.label(
                egui::RichText::new(warning)
                    .size(tokens::TEXT_SMALL)
                    .color(ui.visuals().warn_fg_color),
            );
        }
    }

    ui.add_space(tokens::SPACING_S);
    ui.label(egui::RichText::new("Environment overrides passed to tools").strong());
    ui.add_space(tokens::SPACING_XS);
    for (key, value) in app.paths.child_env() {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("{}=", key.to_string_lossy()))
                    .monospace()
                    .size(tokens::TEXT_SMALL)
                    .weak(),
            );
            copyable(ui, &value.to_string_lossy());
        });
    }
}

fn path_row(ui: &mut egui::Ui, app: &mut AvdiumApp, target: PathTarget, label: &str, help: &str) {
    ui.label(egui::RichText::new(label).size(tokens::TEXT_SMALL).weak());
    // The borrow of the input buffer ends before `app` is used again.
    let mut browse = false;
    let mut clear = false;
    ui.horizontal(|ui| {
        let text = match target {
            PathTarget::Sdk => &mut app.sdk_input,
            PathTarget::AvdHome => &mut app.avd_home_input,
        };
        ui.add(
            egui::TextEdit::singleline(text)
                .desired_width(420.0)
                .hint_text("Leave empty to use the discovered location"),
        )
        .on_hover_text("Manual entry always works, with or without a desktop portal.");
        browse = ui.button("Browse").clicked();
        clear = ui.button("Clear").clicked();
    });
    if clear {
        match target {
            PathTarget::Sdk => app.sdk_input.clear(),
            PathTarget::AvdHome => app.avd_home_input.clear(),
        }
    }
    if browse {
        app.pick_folder(target);
    }
    disabled_reason(ui, help);
    ui.add_space(tokens::SPACING_S);
}
