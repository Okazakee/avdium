//! The Activity screen: jobs, progress, output and cancellation.

use crate::app::AvdiumApp;
use crate::operations::{JobState, LogStream};
use crate::ui::{badge, copyable, disabled_reason, title, tokens};

/// Render the screen.
pub fn show(ui: &mut egui::Ui, app: &mut AvdiumApp) {
    title(
        ui,
        "Activity",
        "Background work. The interface stays responsive because nothing here runs on the UI thread.",
    );

    let jobs = app.coordinator.jobs();
    let running = jobs.iter().filter(|job| !job.state.is_finished()).count();

    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new(format!("{} job(s), {running} active", jobs.len()))
                .size(tokens::TEXT_SMALL)
                .weak(),
        );
        if ui
            .add_enabled(running > 0, egui::Button::new("Cancel all"))
            .clicked()
        {
            app.coordinator.cancel_all();
        }
        if ui
            .add_enabled(!jobs.is_empty(), egui::Button::new("Clear finished"))
            .clicked()
        {
            app.coordinator.clear_finished();
        }
    });
    ui.add_space(tokens::SPACING_S);

    if jobs.is_empty() {
        disabled_reason(ui, "No operations have been submitted in this session.");
        return;
    }

    egui::ScrollArea::vertical()
        .id_salt("activity-scroll")
        .show(ui, |ui| {
            for job in jobs {
                let color = match &job.state {
                    JobState::Succeeded => ui.visuals().weak_text_color(),
                    JobState::Failed(_) => ui.visuals().error_fg_color,
                    JobState::Cancelled => ui.visuals().warn_fg_color,
                    JobState::Queued | JobState::Running | JobState::Cancelling => {
                        crate::ui::accent(ui.visuals())
                    }
                };

                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(0, 6))
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            badge(ui, job.state.label(), color);
                            ui.label(egui::RichText::new(&job.title).strong());
                            ui.label(
                                egui::RichText::new(format!(
                                    "{}  {:.1}s",
                                    job.id,
                                    job.duration().as_secs_f32()
                                ))
                                .size(tokens::TEXT_SMALL)
                                .weak(),
                            );
                            if job.is_cancellable() && ui.button("Cancel").clicked() {
                                app.coordinator.cancel(job.id);
                            }
                        });

                        if let JobState::Failed(reason) = &job.state {
                            ui.label(
                                egui::RichText::new(reason)
                                    .size(tokens::TEXT_BODY)
                                    .color(ui.visuals().error_fg_color),
                            );
                        }

                        if let Some(progress) = &job.progress {
                            match progress.fraction {
                                Some(fraction) => {
                                    ui.add(
                                        egui::ProgressBar::new(fraction.clamp(0.0, 1.0))
                                            .desired_width(320.0)
                                            .text(progress.label.clone()),
                                    );
                                }
                                None => {
                                    ui.horizontal(|ui| {
                                        ui.spinner();
                                        ui.label(
                                            egui::RichText::new(&progress.label)
                                                .size(tokens::TEXT_SMALL),
                                        );
                                    });
                                }
                            }
                        }

                        if let Some(preview) = &job.command_preview {
                            ui.label(
                                egui::RichText::new("Command")
                                    .size(tokens::TEXT_SMALL)
                                    .weak(),
                            );
                            copyable(ui, preview);
                        }

                        if job.log.dropped_lines() > 0 {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} earlier line(s) were dropped to keep the log bounded.",
                                    job.log.dropped_lines()
                                ))
                                .size(tokens::TEXT_SMALL)
                                .weak(),
                            );
                        }

                        if !job.log.is_empty() {
                            let text = job.log.text();
                            let errors: usize = job
                                .log
                                .lines()
                                .filter(|line| {
                                    matches!(line.stream, LogStream::Stderr | LogStream::Error)
                                })
                                .count();
                            let label = if errors > 0 {
                                format!("Output ({} lines, {errors} on stderr)", job.log.len())
                            } else {
                                format!("Output ({} lines)", job.log.len())
                            };
                            crate::ui::raw_block(ui, &format!("log-{}", job.id), &label, &text);
                        }
                    });
            }
        });
}
