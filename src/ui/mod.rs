use crate::logic::{app_logic_thread, LogicCommand, LogicResponse};
use state::{AppState, ViewType};
use std::time::Duration;

mod components;
mod state;

#[derive(Debug)]
pub struct App {
    tx: flume::Sender<LogicCommand>,
    rx: flume::Receiver<LogicResponse>,
    state: AppState,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());

        let (main_tx, logic_rx) = flume::unbounded::<LogicCommand>();
        let (logic_tx, main_rx) = flume::unbounded::<LogicResponse>();
        std::thread::spawn(move || app_logic_thread(logic_rx, logic_tx));

        let state = AppState::default();

        App {
            tx: main_tx,
            rx: main_rx,
            state,
        }
    }

    fn handle_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                LogicResponse::PullRequest(res) => {
                    self.state.pull_request_update.set(res);
                }
                LogicResponse::ExtensionDownloadComplete(res) => {
                    self.state.diffed_extension.set(res);
                }
                LogicResponse::FileDiff(res) => {
                    self.state.diff = res.ok();
                }
            }
        }
    }

    fn draw_pr_select(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Pull request ID:");
            ui.add(egui::DragValue::new(&mut self.state.pull_request_id));

            let fetch_enabled =
                self.state.pull_request_id > 0 && !self.state.pull_request_update.working;

            if ui
                .add_enabled(fetch_enabled, egui::Button::new("Fetch"))
                .clicked()
            {
                self.state.pull_request_update.clear();
                self.tx
                    .send(LogicCommand::GetPullRequest(self.state.pull_request_id))
                    .unwrap();
                self.state.pull_request_update.start();
            }

            if self.state.pull_request_update.working {
                ui.spinner();
            }
        });

        if let Some(update) = &self.state.pull_request_update.value {
            ui.horizontal(|ui| {
                egui::ComboBox::from_label("Extension")
                    .selected_text(
                        self.state
                            .selected_extension
                            .as_deref()
                            .unwrap_or("Select an extension"),
                    )
                    .show_ui(ui, |ui| {
                        for ext in &update.extensions {
                            ui.selectable_value(
                                &mut self.state.selected_extension,
                                Some(ext.id.clone()),
                                ext.id.clone(),
                            );
                        }
                    });

                let download_enabled =
                    self.state.selected_extension.is_some() && !self.state.diffed_extension.working;

                if ui
                    .add_enabled(download_enabled, egui::Button::new("Download"))
                    .clicked()
                {
                    if let Some(ext_id) = &self.state.selected_extension {
                        if let Some(ext) = update.extensions.iter().find(|ext| &ext.id == ext_id) {
                            self.state.diffed_extension.clear();
                            self.tx
                                .send(LogicCommand::DownloadExtension {
                                    extension: ext.clone(),
                                    artifact_url: update.artifact_url.clone(),
                                })
                                .unwrap();
                            self.state.diffed_extension.start();
                        }
                    }
                }

                if self.state.diffed_extension.working {
                    ui.spinner();
                }
            });

            if let Some(ext_id) = &self.state.selected_extension {
                if let Some(ext) = update.extensions.iter().find(|ext| &ext.id == ext_id) {
                    ui.label(format!("Repository: {}", ext.repository));
                    ui.label(format!("Old commit: {}", ext.old_commit));
                    ui.label(format!("New commit: {}", ext.new_commit));
                }
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut delete_diffed_extension = false;
        if let Some(diffed_extension) = &self.state.diffed_extension.value {
            let width = ctx.available_rect().width();
            egui::SidePanel::left("sidebar")
                .resizable(true)
                .max_width(width * 0.3)
                .show(ctx, |ui| {
                    ui.vertical(|ui| {
                        if ui.button("Reset").clicked() {
                            delete_diffed_extension = true;
                        }

                        ui.horizontal(|ui| {
                            let source_clicked = ui
                                .selectable_value(
                                    &mut self.state.view_type,
                                    state::ViewType::Source,
                                    "Source",
                                )
                                .clicked();
                            let asar_clicked = ui
                                .selectable_value(
                                    &mut self.state.view_type,
                                    state::ViewType::Asar,
                                    ".asar",
                                )
                                .clicked();
                            if source_clicked || asar_clicked {
                                self.state.selected_file = None;
                            }
                        });
                    });

                    let diff = if self.state.view_type == ViewType::Source {
                        &diffed_extension.source_diff
                    } else {
                        &diffed_extension.asar_diff
                    };
                    let modified = components::draw_diffed_extension_sidebar(
                        ui,
                        &mut self.state.selected_file,
                        diff,
                    );
                    if modified {
                        if let Some(file) = self.state.selected_file.as_deref() {
                            self.tx
                                .send(LogicCommand::DiffFile(
                                    diff.old.join(file),
                                    diff.new.join(file),
                                ))
                                .unwrap();
                        }
                    }
                });

            egui::CentralPanel::default().show(ctx, |ui| {
                egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
                    if let Some(diff) = &self.state.diff {
                        components::ansi(ui, diff);
                    }
                });
            });
        } else {
            egui::CentralPanel::default().show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        self.draw_pr_select(ui);
                    });
            });
        }

        if delete_diffed_extension {
            self.state.diffed_extension.clear();
        }

        // Since we're receiving messages on the UI thread, we need to be
        // repainting at least sometimes so the UI can update
        self.handle_messages();
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}
