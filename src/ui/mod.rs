//! The egui front end.
//!
//! Layout is a restrained desktop utility: a small token set, clear hierarchy,
//! keyboard-focusable controls, and accessible labels. Every screen renders
//! from state the application already holds, so no screen can block or do I/O
//! while a frame is being drawn.
//!
//! Disabled capabilities always carry a reason. There are no buttons that
//! pretend to work.

pub mod about;
pub mod activity;
pub mod devices;
pub mod doctor;
pub mod screenshot;
pub mod settings;

use egui::Color32;

use crate::doctor::CheckStatus;
use crate::runtime::DeviceState;

/// The main navigation surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Screen {
    /// Devices: the M1 primary surface.
    Devices,
    /// Doctor: evidence-backed checks.
    Doctor,
    /// Activity: jobs, progress, logs and cancellation.
    Activity,
    /// Settings: paths, theme, logging, provider preference.
    Settings,
    /// About: versions, capability matrix and diagnostics.
    About,
}

impl Screen {
    /// Every screen, in navigation order.
    pub const ALL: [Self; 5] = [
        Self::Devices,
        Self::Doctor,
        Self::Activity,
        Self::Settings,
        Self::About,
    ];

    /// Navigation label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Devices => "Devices",
            Self::Doctor => "Doctor",
            Self::Activity => "Activity",
            Self::Settings => "Settings",
            Self::About => "About",
        }
    }

    /// A short description used as an accessible tooltip.
    pub fn description(self) -> &'static str {
        match self {
            Self::Devices => "Virtual devices found in the selected AVD home.",
            Self::Doctor => "Checks with the evidence they were based on.",
            Self::Activity => "Running and finished operations, with their output.",
            Self::Settings => "SDK and AVD locations, theme, logging and provider.",
            Self::About => "Versions, capability matrix and diagnostic export.",
        }
    }
}

/// Spacing, type and sizing tokens.
///
/// A small fixed set keeps the layout consistent and makes density changes a
/// single edit instead of a hunt.
pub mod tokens {
    /// Extra-small gap.
    pub const SPACING_XS: f32 = 4.0;
    /// Small gap.
    pub const SPACING_S: f32 = 8.0;
    /// Medium gap.
    pub const SPACING_M: f32 = 12.0;
    /// Large gap.
    pub const SPACING_L: f32 = 20.0;

    /// Small text, for secondary information.
    pub const TEXT_SMALL: f32 = 11.0;
    /// Body text.
    pub const TEXT_BODY: f32 = 13.0;
    /// Section heading.
    pub const TEXT_HEADING: f32 = 16.0;
    /// Screen title.
    pub const TEXT_TITLE: f32 = 20.0;

    /// Standard row height for dense lists.
    pub const ROW_HEIGHT: f32 = 20.0;
}

/// The accent used for the primary action.
pub fn accent(visuals: &egui::Visuals) -> Color32 {
    if visuals.dark_mode {
        Color32::from_rgb(127, 209, 193)
    } else {
        Color32::from_rgb(23, 122, 106)
    }
}

/// A color for a doctor status.
pub fn status_color(status: CheckStatus, visuals: &egui::Visuals) -> Color32 {
    let dark = visuals.dark_mode;
    match status {
        CheckStatus::Pass => {
            if dark {
                Color32::from_rgb(120, 200, 130)
            } else {
                Color32::from_rgb(28, 120, 44)
            }
        }
        CheckStatus::Warn => {
            if dark {
                Color32::from_rgb(235, 190, 110)
            } else {
                Color32::from_rgb(150, 96, 0)
            }
        }
        CheckStatus::Fail => {
            if dark {
                Color32::from_rgb(240, 130, 130)
            } else {
                Color32::from_rgb(170, 30, 30)
            }
        }
        CheckStatus::Unknown => {
            if dark {
                Color32::from_rgb(170, 170, 180)
            } else {
                Color32::from_rgb(90, 90, 100)
            }
        }
    }
}

/// A color for a device state.
pub fn state_color(state: DeviceState, visuals: &egui::Visuals) -> Color32 {
    let dark = visuals.dark_mode;
    match state {
        DeviceState::BootComplete => {
            if dark {
                Color32::from_rgb(120, 200, 130)
            } else {
                Color32::from_rgb(28, 120, 44)
            }
        }
        DeviceState::Starting | DeviceState::AdbConnected | DeviceState::Stopping => {
            if dark {
                Color32::from_rgb(150, 190, 240)
            } else {
                Color32::from_rgb(30, 90, 170)
            }
        }
        DeviceState::Offline | DeviceState::Failed => {
            if dark {
                Color32::from_rgb(240, 130, 130)
            } else {
                Color32::from_rgb(170, 30, 30)
            }
        }
        DeviceState::Stopped | DeviceState::Unknown => visuals.weak_text_color(),
    }
}

/// Apply the theme preference.
pub fn apply_theme(context: &egui::Context, theme: crate::settings::Theme) {
    let preference = match theme {
        crate::settings::Theme::System => egui::ThemePreference::System,
        crate::settings::Theme::Light => egui::ThemePreference::Light,
        crate::settings::Theme::Dark => egui::ThemePreference::Dark,
    };
    context.set_theme(preference);

    // Applied to both light and dark styles so a theme change never loses the
    // density and type scale.
    context.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(tokens::SPACING_S, tokens::SPACING_S);
        style.spacing.button_padding = egui::vec2(tokens::SPACING_S, tokens::SPACING_XS);
        style.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::proportional(tokens::TEXT_BODY),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::proportional(tokens::TEXT_BODY),
        );
        style.text_styles.insert(
            egui::TextStyle::Small,
            egui::FontId::proportional(tokens::TEXT_SMALL),
        );
        style.text_styles.insert(
            egui::TextStyle::Heading,
            egui::FontId::proportional(tokens::TEXT_HEADING),
        );
    });
}

/// A small colored status chip.
pub fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    let padding = egui::vec2(tokens::SPACING_XS, 1.0);
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        egui::FontId::proportional(tokens::TEXT_SMALL),
        color,
    );
    let (rect, _response) =
        ui.allocate_exact_size(galley.size() + padding * 2.0, egui::Sense::hover());
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(3),
        color.gamma_multiply(0.16),
    );
    ui.painter().galley(rect.min + padding, galley, color);
}

/// A screen title with an optional subtitle.
pub fn title(ui: &mut egui::Ui, heading: &str, subtitle: &str) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(heading)
                .size(tokens::TEXT_TITLE)
                .strong(),
        );
    });
    if !subtitle.is_empty() {
        ui.label(
            egui::RichText::new(subtitle)
                .size(tokens::TEXT_SMALL)
                .weak(),
        );
    }
    ui.add_space(tokens::SPACING_S);
}

/// A labeled key/value row, aligned to the same grid as its neighbours.
pub fn key_value(ui: &mut egui::Ui, key: &str, value: &str) {
    ui.label(egui::RichText::new(key).size(tokens::TEXT_SMALL).weak());
    ui.label(egui::RichText::new(value).size(tokens::TEXT_BODY));
    ui.end_row();
}

/// A monospace value that can be selected and copied.
pub fn copyable(ui: &mut egui::Ui, value: &str) {
    let response = ui.add(
        egui::Label::new(
            egui::RichText::new(value)
                .monospace()
                .size(tokens::TEXT_SMALL),
        )
        .selectable(true),
    );
    response.on_hover_text("Select and copy");
}

/// An explanation for a control that cannot be used.
pub fn disabled_reason(ui: &mut egui::Ui, reason: &str) {
    ui.label(
        egui::RichText::new(reason)
            .size(tokens::TEXT_SMALL)
            .weak()
            .italics(),
    );
}

/// A colored message banner.
pub fn banner(ui: &mut egui::Ui, color: Color32, heading: &str, body: &str) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.12))
        .stroke(egui::Stroke::new(1.0, color.gamma_multiply(0.4)))
        .inner_margin(egui::Margin::same(tokens::SPACING_S as i8))
        .corner_radius(4.0)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                if !heading.is_empty() {
                    ui.label(egui::RichText::new(heading).strong().color(color));
                }
                if !body.is_empty() {
                    ui.label(egui::RichText::new(body).size(tokens::TEXT_BODY));
                }
            });
        });
}

/// A collapsible block of raw text, for command output.
pub fn raw_block(ui: &mut egui::Ui, id: &str, label: &str, text: &str) {
    egui::CollapsingHeader::new(label)
        .id_salt(id)
        .show(ui, |ui| {
            egui::ScrollArea::both()
                .id_salt(format!("{id}-scroll"))
                .max_height(240.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(text)
                                .monospace()
                                .size(tokens::TEXT_SMALL),
                        )
                        .selectable(true),
                    );
                });
        });
}

/// Render a device state chip with a stable width so rows do not jitter.
pub fn device_state_badge(ui: &mut egui::Ui, state: DeviceState) {
    badge(ui, state.label(), state_color(state, ui.visuals()));
}

/// Format a byte count for display.
pub fn bytes_label(bytes: Option<u64>) -> String {
    match bytes {
        Some(bytes) => crate::download::human_bytes(bytes),
        None => "unknown".to_owned(),
    }
}
