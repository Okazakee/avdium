//! The Devices screen: the primary M1 surface.
//!
//! Everything here is read-only except start, cold boot and stop, which are
//! submitted as background jobs. Mutating actions that arrive in later
//! milestones are shown disabled with an explicit reason.

use crate::app::AvdiumApp;
use crate::runtime::{DeviceState, GpuMode, StartOptions};
use crate::ui::{badge, copyable, disabled_reason, key_value, state_color, title, tokens};

/// Render the screen.
pub fn show(ui: &mut egui::Ui, app: &mut AvdiumApp) {
    title(
        ui,
        "Devices",
        "Android virtual devices in the selected AVD home. Nothing is modified unless you ask.",
    );

    if app.demo {
        crate::ui::banner(
            ui,
            crate::ui::accent(ui.visuals()),
            "Demo mode",
            "These devices are synthetic. Demo mode never reads or changes a real SDK.",
        );
        ui.add_space(tokens::SPACING_S);
    }

    if !app.demo && !app.paths.sdk_looks_usable() {
        crate::ui::banner(
            ui,
            ui.visuals().warn_fg_color,
            "No usable SDK",
            "Devices can still be listed and inspected. Starting one needs the emulator \
             and platform-tools. Choose an SDK location in Settings, or use demo mode to \
             explore the interface.",
        );
        ui.add_space(tokens::SPACING_S);
    }

    ui.horizontal(|ui| {
        let refreshing = app.is_busy("discovery") || app.is_busy("runtimes");
        if ui
            .add_enabled(!refreshing, egui::Button::new("Refresh"))
            .clicked()
        {
            app.refresh_all();
        }
        if refreshing {
            ui.spinner();
            ui.label(
                egui::RichText::new("Refreshing")
                    .size(tokens::TEXT_SMALL)
                    .weak(),
            );
        }
        ui.separator();
        ui.label(
            egui::RichText::new(format!("{} device(s)", app.discovery.avds.len()))
                .size(tokens::TEXT_SMALL)
                .weak(),
        );
        if !app.discovery.warnings.is_empty() {
            ui.separator();
            ui.label(
                egui::RichText::new(format!("{} warning(s)", app.discovery.warnings.len()))
                    .size(tokens::TEXT_SMALL)
                    .color(ui.visuals().warn_fg_color),
            );
        }
    });
    ui.add_space(tokens::SPACING_S);

    if app.discovery.avds.is_empty() {
        empty_state(ui, app);
        return;
    }

    egui::ScrollArea::vertical()
        .id_salt("devices-scroll")
        .max_height(260.0)
        .show(ui, |ui| {
            egui::Grid::new("devices-grid")
                .num_columns(5)
                .striped(true)
                .spacing(egui::vec2(tokens::SPACING_M, tokens::SPACING_XS))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("Device")
                            .strong()
                            .size(tokens::TEXT_SMALL),
                    );
                    ui.label(
                        egui::RichText::new("Image")
                            .strong()
                            .size(tokens::TEXT_SMALL),
                    );
                    ui.label(
                        egui::RichText::new("Status")
                            .strong()
                            .size(tokens::TEXT_SMALL),
                    );
                    ui.label(
                        egui::RichText::new("Hardware")
                            .strong()
                            .size(tokens::TEXT_SMALL),
                    );
                    ui.label(
                        egui::RichText::new("Definition")
                            .strong()
                            .size(tokens::TEXT_SMALL),
                    );
                    ui.end_row();

                    for avd in app.discovery.avds.clone() {
                        let selected = app.selected.as_deref() == Some(avd.name.as_str());
                        let label = if avd.is_broken() {
                            format!("{}  (!)", avd.display_name())
                        } else {
                            avd.display_name().to_owned()
                        };
                        if ui.selectable_label(selected, label).clicked() {
                            app.selected = Some(avd.name.clone());
                        }
                        ui.label(egui::RichText::new(avd.image_label()).size(tokens::TEXT_BODY));

                        let runtime = app.runtime_for(&avd.name);
                        badge(
                            ui,
                            runtime.state.label(),
                            state_color(runtime.state, ui.visuals()),
                        );

                        let hardware = format!(
                            "{} MiB, {} cores, GPU {}",
                            avd.ram_mb()
                                .map(|value| value.to_string())
                                .unwrap_or_else(|| "?".to_owned()),
                            avd.cores()
                                .map(|value| value.to_string())
                                .unwrap_or_else(|| "?".to_owned()),
                            avd.gpu_mode().unwrap_or("?")
                        );
                        ui.label(egui::RichText::new(hardware).size(tokens::TEXT_SMALL));

                        if avd.is_broken() {
                            // The table stays readable; the full reason is shown
                            // in the detail panel and by the doctor.
                            ui.label(
                                egui::RichText::new("broken")
                                    .size(tokens::TEXT_SMALL)
                                    .color(ui.visuals().error_fg_color),
                            );
                        } else {
                            ui.label(
                                egui::RichText::new(avd.data_dir_origin.label())
                                    .size(tokens::TEXT_SMALL)
                                    .weak(),
                            );
                        }
                        ui.end_row();
                    }
                });
        });

    ui.add_space(tokens::SPACING_M);

    let Some(name) = app.selected.clone() else {
        ui.label(
            egui::RichText::new("Select a device to see its details and actions.")
                .size(tokens::TEXT_SMALL)
                .weak(),
        );
        return;
    };
    let Some(avd) = app.discovery.get(&name).cloned() else {
        app.selected = None;
        return;
    };

    ui.separator();
    ui.label(
        egui::RichText::new(avd.display_name())
            .strong()
            .size(tokens::TEXT_HEADING),
    );
    ui.add_space(tokens::SPACING_XS);

    egui::Grid::new("device-details")
        .num_columns(2)
        .spacing(egui::vec2(tokens::SPACING_M, tokens::SPACING_XS))
        .show(ui, |ui| {
            key_value(ui, "AVD name", &avd.name);
            key_value(ui, "Target", avd.target().unwrap_or("not recorded"));
            key_value(
                ui,
                "Tag",
                avd.tag_display()
                    .or_else(|| avd.tag_id())
                    .unwrap_or("not recorded"),
            );
            key_value(ui, "ABI", avd.abi().unwrap_or("not recorded"));
            key_value(
                ui,
                "Device profile",
                avd.device_name().unwrap_or("not recorded"),
            );
            key_value(
                ui,
                "Play Store flag",
                match avd.play_store_flag() {
                    Some(true) => "yes (the installed image is authoritative)",
                    Some(false) => "no",
                    None => "not recorded",
                },
            );
            ui.label(
                egui::RichText::new("AVD home file")
                    .size(tokens::TEXT_SMALL)
                    .weak(),
            );
            copyable(ui, &avd.ini_path.to_string_lossy());
            ui.end_row();
            ui.label(
                egui::RichText::new("Data directory")
                    .size(tokens::TEXT_SMALL)
                    .weak(),
            );
            copyable(ui, &avd.data_dir.to_string_lossy());
            ui.end_row();
            ui.label(
                egui::RichText::new("Configuration")
                    .size(tokens::TEXT_SMALL)
                    .weak(),
            );
            copyable(ui, &avd.config_path.to_string_lossy());
            ui.end_row();
            if let Some(image) = avd.image_sysdir() {
                key_value(ui, "System image", image);
            }
        });

    if !avd.broken.is_empty() {
        ui.add_space(tokens::SPACING_S);
        for reason in &avd.broken {
            crate::ui::banner(
                ui,
                ui.visuals().error_fg_color,
                "Problem",
                &reason.summary(),
            );
        }
    }

    ui.add_space(tokens::SPACING_M);
    let runtime = app.runtime_for(&name).clone();
    action_row(ui, app, &name, &avd, &runtime);

    ui.add_space(tokens::SPACING_M);
    ui.label(
        egui::RichText::new("Command preview")
            .strong()
            .size(tokens::TEXT_BODY),
    );
    ui.label(
        egui::RichText::new(
            "Avdium runs argument arrays, never a reconstructed shell command. This is \
             what it would execute, shown so you can inspect it.",
        )
        .size(tokens::TEXT_SMALL)
        .weak(),
    );
    ui.add_space(tokens::SPACING_XS);
    for (label, options) in [
        ("Start", StartOptions::default()),
        ("Cold boot", StartOptions::cold()),
    ] {
        match app.start_preview(&name, &options) {
            Some(preview) => {
                ui.label(egui::RichText::new(label).size(tokens::TEXT_SMALL).weak());
                copyable(ui, &preview);
            }
            None => {
                ui.label(egui::RichText::new(label).size(tokens::TEXT_SMALL).weak());
                disabled_reason(
                    ui,
                    "No emulator executable is available in the selected SDK.",
                );
            }
        }
    }
    if let Some(serial) = &runtime.serial
        && let Some(preview) = app.stop_preview(serial)
    {
        ui.label(egui::RichText::new("Stop").size(tokens::TEXT_SMALL).weak());
        copyable(ui, &preview);
    }

    if let Some(log) = app.emulator_log(&name) {
        ui.add_space(tokens::SPACING_S);
        crate::ui::raw_block(ui, "emulator-log", "Emulator log (most recent lines)", &log);
    }
}

fn empty_state(ui: &mut egui::Ui, app: &mut AvdiumApp) {
    let avd_home = app.paths.avd_home.display().to_string();
    crate::ui::banner(
        ui,
        ui.visuals().warn_fg_color,
        "No devices found",
        &format!(
            "Nothing was found in {avd_home}. Avdium does not create devices in this \
             milestone; creation arrives with the provisioning work. If you already have \
             devices elsewhere, set the AVD home in Settings."
        ),
    );
    ui.add_space(tokens::SPACING_S);
    ui.horizontal(|ui| {
        if ui.button("Open Settings").clicked() {
            app.screen = crate::ui::Screen::Settings;
        }
        if ui.button("Run the doctor").clicked() {
            app.screen = crate::ui::Screen::Doctor;
        }
    });
}

fn action_row(
    ui: &mut egui::Ui,
    app: &mut AvdiumApp,
    name: &str,
    avd: &crate::avd::Avd,
    runtime: &crate::runtime::DeviceRuntime,
) {
    let image_missing = avd
        .broken
        .iter()
        .any(|reason| matches!(reason, crate::avd::BrokenReason::MissingImage { .. }));
    let running = runtime.state.is_running();
    let busy = app.is_busy(&crate::operations::jobs::avd_mutation_key(name));

    ui.horizontal_wrapped(|ui| {
        let can_start = !running && !image_missing && !busy;
        if ui
            .add_enabled(can_start, egui::Button::new("Start"))
            .on_hover_text("Launch the device with snapshots enabled.")
            .clicked()
        {
            app.start_device(name, StartOptions::default());
        }
        if ui
            .add_enabled(can_start, egui::Button::new("Cold boot"))
            .on_hover_text("Launch without loading a snapshot. Snapshots are still saved on exit.")
            .clicked()
        {
            app.start_device(name, StartOptions::cold());
        }
        let can_stop = running && !busy;
        if ui
            .add_enabled(can_stop, egui::Button::new("Stop"))
            .on_hover_text("Ask the guest to shut down, then signal the process group.")
            .clicked()
        {
            app.stop_device(name);
        }
        ui.separator();
        // Later-milestone actions are visible but disabled, each with a reason.
        ui.add_enabled(false, egui::Button::new("Edit"));
        ui.add_enabled(false, egui::Button::new("Wipe"));
        ui.add_enabled(false, egui::Button::new("Delete"));
    });

    if image_missing {
        disabled_reason(
            ui,
            "Start is unavailable because the system image this device references is not installed.",
        );
    }
    if busy {
        disabled_reason(ui, "An operation for this device is already running.");
    }
    disabled_reason(
        ui,
        "Edit, wipe and delete are not implemented yet. They arrive with safe mutation, \
         which requires conflict checks and a recovery route. Nothing is disabled silently.",
    );
    if runtime.state == DeviceState::Failed
        && let Some(detail) = &runtime.detail
    {
        ui.label(
            egui::RichText::new(detail)
                .size(tokens::TEXT_SMALL)
                .color(ui.visuals().error_fg_color),
        );
    }
    if runtime.state == DeviceState::BootComplete
        && avd.gpu_mode().and_then(GpuMode::parse) == Some(GpuMode::SwiftshaderIndirect)
    {
        crate::ui::banner(
            ui,
            ui.visuals().warn_fg_color,
            "Deprecated graphics mode",
            "This device records swiftshader_indirect, which is deprecated from emulator \
             36.4.9. Avdium does not select it as a fallback.",
        );
    }
}
