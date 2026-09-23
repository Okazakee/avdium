//! Repeatable UI screenshots.
//!
//! **Implemented renderer:** a windowed renderer that requires a display
//! server. It drives the real application, captures each screen through the
//! egui viewport screenshot request, and writes PNG files. It works on Wayland
//! or X11, and under a virtual display such as `Xvfb` when no real display is
//! attached. A truly headless renderer that needs no display server is not
//! implemented.
//!
//! The mode is behind the `screenshot` feature so the default build does not
//! carry a PNG encoder.

#[cfg(feature = "screenshot")]
mod implementation {
    use std::io::BufWriter;
    use std::path::{Path, PathBuf};

    use crate::ui::Screen;

    /// Frames to render before capturing, so layout and fonts have settled.
    const SETTLE_FRAMES: u32 = 8;

    /// Drives a sequence of screen captures.
    #[derive(Debug)]
    pub struct ScreenshotState {
        directory: PathBuf,
        targets: Vec<Screen>,
        index: usize,
        settle: u32,
        /// The target an outstanding screenshot request belongs to.
        ///
        /// egui can run several passes per frame and the same input events are
        /// visible to each pass. Tracking the outstanding request by index
        /// means a repeated event rewrites the same file instead of advancing
        /// the sequence twice.
        pending: Option<usize>,
        finished: bool,
        error: Option<String>,
        written: Vec<PathBuf>,
    }

    impl ScreenshotState {
        /// Capture every screen, in navigation order.
        pub fn new(directory: PathBuf) -> Self {
            Self {
                directory,
                targets: Screen::ALL.to_vec(),
                index: 0,
                settle: 0,
                pending: None,
                finished: false,
                error: None,
                written: Vec::new(),
            }
        }

        /// Whether every screen has been captured.
        pub fn is_finished(&self) -> bool {
            self.finished
        }

        /// A failure to report, if one happened.
        pub fn error(&self) -> Option<&str> {
            self.error.as_deref()
        }

        /// Files that were written.
        pub fn written(&self) -> &[PathBuf] {
            &self.written
        }

        /// The screen that should be visible right now.
        pub fn current_screen(&self) -> Screen {
            self.targets
                .get(self.index)
                .copied()
                .unwrap_or(Screen::Devices)
        }

        /// Advance the capture sequence by one frame.
        pub fn drive(&mut self, context: &egui::Context) {
            if self.finished {
                return;
            }
            let Some(target) = self.targets.get(self.index).copied() else {
                self.finished = true;
                return;
            };

            if self.pending.is_none() {
                if self.settle < SETTLE_FRAMES {
                    self.settle += 1;
                    context.request_repaint();
                    return;
                }
                self.pending = Some(self.index);
                context.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                    egui::UserData::default(),
                ));
                context.request_repaint();
                return;
            }

            let image = context.input(|input| {
                input.events.iter().find_map(|event| match event {
                    egui::Event::Screenshot { image, .. } => Some((**image).clone()),
                    _ => None,
                })
            });

            if let Some(image) = image {
                match write_png(&self.directory, target, &image) {
                    Ok(path) => {
                        // A repeated event in the same frame writes the same
                        // file again rather than advancing the sequence.
                        if !self.written.contains(&path) {
                            self.written.push(path);
                        }
                    }
                    Err(error) => {
                        self.error = Some(format!("could not write the screenshot: {error}"));
                        self.finished = true;
                        return;
                    }
                }
                self.index += 1;
                self.settle = 0;
                self.pending = None;
                if self.index >= self.targets.len() {
                    self.finished = true;
                }
            }
            context.request_repaint();
        }
    }

    fn write_png(
        directory: &Path,
        screen: Screen,
        image: &egui::ColorImage,
    ) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(directory)?;
        let name = screen.label().to_ascii_lowercase();
        let path = directory.join(format!("{name}.png"));

        let width = image.size[0] as u32;
        let height = image.size[1] as u32;
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for pixel in &image.pixels {
            pixels.extend_from_slice(&pixel.to_array());
        }

        let file = std::fs::File::create(&path)?;
        let mut encoder = png::Encoder::new(BufWriter::new(file), width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(std::io::Error::other)?;
        writer
            .write_image_data(&pixels)
            .map_err(std::io::Error::other)?;
        writer.finish().map_err(std::io::Error::other)?;
        Ok(path)
    }

    #[test]
    fn the_capture_sequence_covers_every_screen() {
        let state = ScreenshotState::new(PathBuf::from("/tmp/shots"));
        assert_eq!(state.current_screen(), Screen::Devices);
        assert!(!state.is_finished());
        assert_eq!(state.targets.len(), Screen::ALL.len());
    }
}

#[cfg(feature = "screenshot")]
pub use implementation::ScreenshotState;

/// Placeholder used when the feature is off.
///
/// It exposes the same surface as the real implementation so the application
/// compiles in both configurations, and it never captures anything.
#[cfg(not(feature = "screenshot"))]
#[derive(Debug, Default)]
pub struct ScreenshotState;

#[cfg(not(feature = "screenshot"))]
impl ScreenshotState {
    /// Screenshot mode is not compiled into this build, so this is never used.
    pub fn new(_directory: std::path::PathBuf) -> Self {
        Self
    }

    /// Always finished.
    pub fn is_finished(&self) -> bool {
        true
    }

    /// No failure to report.
    pub fn error(&self) -> Option<&str> {
        None
    }

    /// Nothing was written.
    pub fn written(&self) -> &[std::path::PathBuf] {
        &[]
    }

    /// The screen that would be captured.
    pub fn current_screen(&self) -> crate::ui::Screen {
        crate::ui::Screen::Devices
    }

    /// Does nothing.
    pub fn drive(&mut self, _context: &egui::Context) {}
}
