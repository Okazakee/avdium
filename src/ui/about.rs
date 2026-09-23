//! The About screen: versions, capability matrix and diagnostic export.

use crate::app::AvdiumApp;
use crate::identity;
use crate::ui::{badge, copyable, disabled_reason, key_value, title, tokens};

/// Render the screen.
pub fn show(ui: &mut egui::Ui, app: &mut AvdiumApp) {
    title(ui, identity::NAME, identity::TAGLINE);

    egui::Grid::new("about-versions")
        .num_columns(2)
        .spacing(egui::vec2(tokens::SPACING_M, tokens::SPACING_XS))
        .show(ui, |ui| {
            key_value(ui, "Avdium", identity::VERSION);
            key_value(ui, "Application id", identity::APP_ID);
            key_value(
                ui,
                "License",
                "MIT. Reused code keeps its original notices.",
            );
            key_value(
                ui,
                "Telemetry",
                "None. Avdium sends nothing about you. The Android guest may still make its own \
                 network requests.",
            );
            key_value(
                ui,
                "Host",
                &format!("{} {}", std::env::consts::OS, app.paths.home.display()),
            );
            for (label, value) in app.tool_versions() {
                key_value(ui, label, &value);
            }
        });

    ui.add_space(tokens::SPACING_M);
    ui.label(egui::RichText::new("Capability matrix").strong());
    ui.add_space(tokens::SPACING_XS);
    ui.label(
        egui::RichText::new(
            "Capabilities are stated per provider instead of claiming parity. A capability that \
             is missing always has a reason.",
        )
        .size(tokens::TEXT_SMALL)
        .weak(),
    );
    ui.add_space(tokens::SPACING_XS);

    let providers = app.capability_matrix();
    if providers.is_empty() {
        disabled_reason(
            ui,
            "No provider was detected, so there is nothing to report.",
        );
    }
    for (name, capabilities) in providers {
        ui.label(egui::RichText::new(name).strong().size(tokens::TEXT_BODY));
        egui::Grid::new(format!("caps-{name}"))
            .num_columns(3)
            .spacing(egui::vec2(tokens::SPACING_M, tokens::SPACING_XS))
            .show(ui, |ui| {
                for (capability, supported) in capabilities.matrix() {
                    ui.label(egui::RichText::new(capability).size(tokens::TEXT_BODY));
                    if supported {
                        badge(ui, "yes", crate::ui::accent(ui.visuals()));
                    } else {
                        badge(ui, "no", ui.visuals().warn_fg_color);
                    }
                    let reason = capabilities.unsupported_reason(capability);
                    ui.label(
                        egui::RichText::new(reason.unwrap_or_else(|| "supported".to_owned()))
                            .size(tokens::TEXT_SMALL)
                            .weak(),
                    );
                    ui.end_row();
                }
                ui.label(egui::RichText::new("License consent").size(tokens::TEXT_BODY));
                badge(
                    ui,
                    if capabilities.license_boundary.allows_consent_flow() {
                        "reviewable"
                    } else {
                        "blocked"
                    },
                    if capabilities.license_boundary.allows_consent_flow() {
                        crate::ui::accent(ui.visuals())
                    } else {
                        ui.visuals().warn_fg_color
                    },
                );
                ui.label(
                    egui::RichText::new(capabilities.license_boundary.label())
                        .size(tokens::TEXT_SMALL)
                        .weak(),
                );
                ui.end_row();
            });
        ui.add_space(tokens::SPACING_S);
    }

    ui.separator();
    ui.label(egui::RichText::new("Diagnostics").strong());
    ui.add_space(tokens::SPACING_XS);
    ui.label(
        egui::RichText::new(
            "The exported report is redacted by default: your home directory is replaced with \
             $HOME. Inspect it before sharing it.",
        )
        .size(tokens::TEXT_SMALL)
        .weak(),
    );
    ui.add_space(tokens::SPACING_XS);
    ui.horizontal_wrapped(|ui| {
        if ui.button("Copy redacted report").clicked() {
            let text = app.redacted_diagnostic();
            ui.ctx().copy_text(text);
            app.status = "Redacted diagnostic report copied to the clipboard.".to_owned();
        }
        if ui.button("Copy unredacted report").clicked() {
            let text = app.diagnostic_text();
            ui.ctx().copy_text(text);
            app.status = "Unredacted report copied. It contains local paths.".to_owned();
        }
        if ui.button("Copy emulator log location").clicked() {
            let text = app.dirs.logs_dir().display().to_string();
            ui.ctx().copy_text(text);
            app.status = "Log directory copied.".to_owned();
        }
    });

    ui.add_space(tokens::SPACING_S);
    let report = app.redacted_diagnostic();
    crate::ui::raw_block(ui, "diagnostics", "Redacted report preview", &report);

    ui.add_space(tokens::SPACING_S);
    ui.label(
        egui::RichText::new(
            "Avdium is an independent project. It is not affiliated with, endorsed by, or \
             connected to Google. Android is a trademark of Google LLC.",
        )
        .size(tokens::TEXT_SMALL)
        .weak(),
    );
    ui.add_space(tokens::SPACING_XS);
    ui.label(
        egui::RichText::new("Application identifier and desktop entry")
            .size(tokens::TEXT_SMALL)
            .weak(),
    );
    copyable(
        ui,
        &format!("{} / {}.desktop", identity::APP_ID, identity::APP_ID),
    );
}
