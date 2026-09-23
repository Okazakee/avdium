//! The Doctor screen: evidence-backed checks with next actions.

use crate::app::AvdiumApp;
use crate::doctor::CheckStatus;
use crate::ui::{badge, copyable, disabled_reason, status_color, title, tokens};

/// Render the screen.
pub fn show(ui: &mut egui::Ui, app: &mut AvdiumApp) {
    title(
        ui,
        "Doctor",
        "Every result shows what was observed and what to do next. Unknown means unknown.",
    );

    ui.horizontal_wrapped(|ui| {
        let running = app.doctor_running();
        if ui
            .add_enabled(!running, egui::Button::new("Run checks"))
            .clicked()
        {
            app.run_doctor(false);
        }
        if ui
            .add_enabled(!running, egui::Button::new("Run checks with tool probes"))
            .on_hover_text(
                "Also executes the emulator, Android CLI and Java. Running a tool can write \
                 its own caches, so it is never done without asking.",
            )
            .clicked()
        {
            app.run_doctor(true);
        }
        if running {
            ui.spinner();
            ui.label(
                egui::RichText::new("Running")
                    .size(tokens::TEXT_SMALL)
                    .weak(),
            );
        }
    });
    ui.add_space(tokens::SPACING_XS);
    ui.label(
        egui::RichText::new(
            "Checks that need a subprocess stay off until you ask, because a listing command \
             can still create directories or write caches.",
        )
        .size(tokens::TEXT_SMALL)
        .weak(),
    );
    ui.add_space(tokens::SPACING_S);

    let Some(report) = app.doctor.clone() else {
        disabled_reason(ui, "No checks have been run yet.");
        return;
    };

    let (pass, warn, fail, unknown) = report.counts();
    ui.horizontal_wrapped(|ui| {
        badge(
            ui,
            &format!("{pass} pass"),
            status_color(CheckStatus::Pass, ui.visuals()),
        );
        badge(
            ui,
            &format!("{warn} warning"),
            status_color(CheckStatus::Warn, ui.visuals()),
        );
        badge(
            ui,
            &format!("{fail} fail"),
            status_color(CheckStatus::Fail, ui.visuals()),
        );
        badge(
            ui,
            &format!("{unknown} unknown"),
            status_color(CheckStatus::Unknown, ui.visuals()),
        );
    });
    ui.add_space(tokens::SPACING_S);

    egui::ScrollArea::vertical()
        .id_salt("doctor-scroll")
        .show(ui, |ui| {
            for check in &report.checks {
                let color = status_color(check.status, ui.visuals());
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(0, 4))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            badge(ui, check.status.label(), color);
                            ui.label(egui::RichText::new(&check.title).strong());
                        });
                        for line in &check.evidence {
                            ui.horizontal(|ui| {
                                ui.add_space(tokens::SPACING_M);
                                copyable(ui, line);
                            });
                        }
                        if let Some(hint) = &check.hint {
                            ui.horizontal(|ui| {
                                ui.add_space(tokens::SPACING_M);
                                ui.label(
                                    egui::RichText::new(format!("Next: {hint}"))
                                        .size(tokens::TEXT_SMALL)
                                        .color(crate::ui::accent(ui.visuals())),
                                );
                            });
                        }
                    });
            }
        });
}
