//! The window.
//!
//! One screen, one button. The design goal is that someone who has never
//! opened a terminal can download this, double-click it, click the button, and
//! be back in Discord — without reading anything except the button.

use crate::clean;
use crate::discord::{Install, Scope};
use crate::job::{self, Options, Step, Summary};
use crate::platform;
use eframe::egui;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Instant;

const BLURPLE: egui::Color32 = egui::Color32::from_rgb(88, 101, 242);
const WINDOW_SIZE: [f32; 2] = [460.0, 470.0];

/// The window/taskbar icon, stored as raw RGBA so we don't need a PNG decoder
/// in the binary. Regenerate with `python3 assets/make_icons.py assets`.
fn icon() -> egui::IconData {
    const SIZE: u32 = 256;
    egui::IconData {
        rgba: include_bytes!("../assets/icon-256.rgba").to_vec(),
        width: SIZE,
        height: SIZE,
    }
}

pub fn run(options: Options) -> anyhow::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(WINDOW_SIZE)
            .with_min_inner_size(WINDOW_SIZE)
            .with_resizable(false)
            .with_title("Discord Desktop Fixer")
            .with_icon(icon()),
        ..Default::default()
    };

    eframe::run_native(
        "Discord Desktop Fixer",
        native_options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(App::new(options)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("couldn't open the window: {e}"))
}

enum Msg {
    Step(String),
    Done(Box<Summary>),
}

enum State {
    Idle,
    Running {
        rx: Receiver<Msg>,
        steps: Vec<String>,
        started: Instant,
    },
    Done {
        summary: Box<Summary>,
        dry_run: bool,
    },
}

struct App {
    installs: Vec<Install>,
    selected: Vec<bool>,
    deep: bool,
    relaunch: bool,
    dry_run: bool,
    state: State,
}

impl App {
    fn new(options: Options) -> Self {
        let installs = platform::discover_sorted();
        let selected = vec![true; installs.len()];
        Self {
            installs,
            selected,
            deep: options.scope == Scope::Deep,
            relaunch: options.relaunch,
            dry_run: options.dry_run,
            state: State::Idle,
        }
    }

    fn chosen(&self) -> Vec<Install> {
        self.installs
            .iter()
            .zip(&self.selected)
            .filter(|(_, on)| **on)
            .map(|(install, _)| install.clone())
            .collect()
    }

    fn start(&mut self, ctx: &egui::Context) {
        let installs = self.chosen();
        let options = Options {
            scope: if self.deep { Scope::Deep } else { Scope::Safe },
            relaunch: self.relaunch,
            dry_run: self.dry_run,
        };
        let (tx, rx): (Sender<Msg>, Receiver<Msg>) = std::sync::mpsc::channel();
        let ctx = ctx.clone();

        // All the real work happens off the UI thread — killing processes
        // involves multi-second waits, and a frozen window looks like a crash.
        std::thread::spawn(move || {
            let notify = ctx.clone();
            let step_tx = tx.clone();
            let summary = job::run(&installs, options, move |Step(line)| {
                let _ = step_tx.send(Msg::Step(line));
                notify.request_repaint();
            });
            let _ = tx.send(Msg::Done(Box::new(summary)));
            ctx.request_repaint();
        });

        self.state = State::Running {
            rx,
            steps: Vec::new(),
            started: Instant::now(),
        };
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.pump();
        let ctx = ui.ctx().clone();

        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            ui.set_min_size(ui.available_size());

            ui.add_space(18.0);
            ui.vertical_centered(|ui| {
                ui.heading("Discord Desktop Fixer");
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new("Clears the caches that make Discord get stuck.")
                        .color(ui.visuals().weak_text_color()),
                );
            });
            ui.add_space(16.0);

            if self.installs.is_empty() {
                self.show_nothing_found(ui);
                return;
            }

            match &self.state {
                State::Idle => self.show_idle(ui, &ctx),
                State::Running { .. } => self.show_running(ui),
                State::Done { .. } => self.show_done(ui),
            }
        });
    }
}

impl App {
    /// Drain whatever the worker has said since the last frame.
    fn pump(&mut self) {
        let State::Running { rx, steps, .. } = &mut self.state else {
            return;
        };
        let mut finished = None;
        while let Ok(msg) = rx.try_recv() {
            match msg {
                Msg::Step(line) => steps.push(line),
                Msg::Done(summary) => finished = Some(summary),
            }
        }
        if let Some(summary) = finished {
            self.state = State::Done {
                summary,
                dry_run: self.dry_run,
            };
        }
    }

    fn show_nothing_found(&self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.label(
                egui::RichText::new("No Discord installation found")
                    .size(16.0)
                    .strong(),
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(
                    "Install the Discord desktop app, sign in once, then run this again.",
                )
                .color(ui.visuals().weak_text_color()),
            );
        });
    }

    fn show_idle(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // With a single install there's nothing to choose, so we just say what
        // we found and get out of the way.
        if self.installs.len() == 1 {
            ui.vertical_centered(|ui| {
                ui.label(format!("Found {}", self.installs[0].label()));
            });
        } else {
            ui.label(egui::RichText::new("Which ones?").strong());
            ui.add_space(4.0);
            for (i, install) in self.installs.iter().enumerate() {
                ui.checkbox(&mut self.selected[i], install.label());
            }
        }

        ui.add_space(20.0);
        let any = self.selected.iter().any(|&on| on);
        ui.vertical_centered(|ui| {
            let button = egui::Button::new(
                egui::RichText::new("Fix Discord")
                    .size(19.0)
                    .strong()
                    .color(egui::Color32::WHITE),
            )
            .fill(BLURPLE)
            .corner_radius(10.0)
            .min_size(egui::vec2(240.0, 52.0));

            if ui.add_enabled(any, button).clicked() {
                self.start(ctx);
            }
        });

        ui.add_space(20.0);
        ui.checkbox(&mut self.deep, "Deep clean — also fixes stuck updates")
            .on_hover_text(
                "Additionally clears downloaded modules and web storage. Discord \
                 re-downloads them on the next launch. You still stay logged in.",
            );
        ui.checkbox(&mut self.relaunch, "Reopen Discord when finished");

        self.footer(ui);
    }

    fn show_running(&self, ui: &mut egui::Ui) {
        let State::Running { steps, started, .. } = &self.state else {
            return;
        };
        ui.vertical_centered(|ui| {
            ui.add_space(24.0);
            ui.add(egui::Spinner::new().size(30.0));
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(steps.last().map(String::as_str).unwrap_or("Starting…"))
                    .size(15.0),
            );
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(format!("{:.0}s", started.elapsed().as_secs_f32()))
                    .color(ui.visuals().weak_text_color()),
            );
        });
        self.footer(ui);
    }

    fn show_done(&self, ui: &mut egui::Ui) {
        let State::Done { summary, dry_run } = &self.state else {
            return;
        };

        ui.vertical_centered(|ui| {
            ui.add_space(16.0);
            ui.label(
                egui::RichText::new("Done")
                    .size(24.0)
                    .strong()
                    .color(BLURPLE),
            );
            ui.add_space(6.0);
            ui.label(egui::RichText::new(summary.headline(*dry_run)).size(15.0));
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new(format!("in {:.1}s", summary.elapsed.as_secs_f32()))
                    .color(ui.visuals().weak_text_color()),
            );
            if summary.relaunched {
                ui.add_space(6.0);
                ui.label("Discord is reopening.");
            }
        });

        ui.add_space(12.0);

        if !summary.warnings.is_empty() {
            ui.collapsing(
                format!("{} thing(s) needed attention", summary.warnings.len()),
                |ui| {
                    for warning in &summary.warnings {
                        ui.label(egui::RichText::new(warning).size(12.0));
                    }
                },
            );
        }

        ui.collapsing("Details", |ui| {
            egui::ScrollArea::vertical()
                .max_height(120.0)
                .show(ui, |ui| {
                    for item in &summary.report.items {
                        if let clean::Outcome::Removed { bytes } = item.outcome {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{}  —  {}",
                                    display_name(&item.path),
                                    clean::format_bytes(bytes)
                                ))
                                .size(12.0),
                            );
                        }
                    }
                });
            ui.add_space(6.0);
            if ui.button("Copy log").clicked() {
                ui.ctx().copy_text(self.log_text(summary, *dry_run));
            }
        });

        self.footer(ui);
    }

    fn log_text(&self, summary: &Summary, dry_run: bool) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        let _ = writeln!(
            out,
            "Discord Desktop Fixer {} on {}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS
        );
        let _ = writeln!(out, "{}\n", summary.headline(dry_run));
        for item in &summary.report.items {
            let _ = writeln!(out, "{}: {:?}", item.path.display(), item.outcome);
        }
        for warning in &summary.warnings {
            let _ = writeln!(out, "warning: {warning}");
        }
        out
    }

    /// The reassurance, always on screen. It is the question every user has.
    fn footer(&self, ui: &mut egui::Ui) {
        let available = ui.available_height();
        if available > 24.0 {
            ui.add_space(available - 22.0);
        }
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(
                    "You stay logged in. Your messages and settings are untouched.",
                )
                .size(11.5)
                .color(ui.visuals().weak_text_color()),
            );
        });
    }
}

/// `…/discord/Code Cache` reads better as `Code Cache`.
fn display_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
