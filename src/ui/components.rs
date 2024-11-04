use crate::logic::diff::{Diff, Directory, FileState, FilesystemItem};
use anstyle_parse::{DefaultCharAccumulator, Params, Parser, Perform};
use egui::{text::LayoutJob, FontId};

fn draw_dir(
    ui: &mut egui::Ui,
    current_file: &mut Option<String>,
    root: Option<String>,
    folder: &Directory,
) -> bool {
    let mut modified = false;

    for item in folder {
        match item {
            FilesystemItem::File { name, state } => {
                let selected = current_file.as_deref() == Some(&name);
                let state_text = match state {
                    FileState::Added => "+",
                    FileState::Removed => "-",
                    FileState::Modified => "~",
                };
                let state_name = format!("{} {}", state_text, name);

                let full_path = if let Some(ref root) = root {
                    format!("{}/{}", root, name)
                } else {
                    name.clone()
                };

                ui.push_id(full_path.clone(), |ui| {
                    if ui.selectable_label(selected, state_name.clone()).clicked() {
                        *current_file = Some(full_path);
                        modified = true;
                    }
                });
            }

            FilesystemItem::Directory { name, children } => {
                let name = name.as_deref().unwrap_or_default();
                let full_path = if let Some(ref root) = root {
                    format!("{}/{}", root, name)
                } else {
                    name.to_string()
                };

                ui.push_id(full_path.clone(), |ui| {
                    ui.collapsing(format!("{}/", name), |ui| {
                        if draw_dir(ui, current_file, Some(full_path), children) {
                            modified = true;
                        }
                    });
                });
            }
        }
    }

    modified
}

pub fn draw_diffed_extension_sidebar(
    ui: &mut egui::Ui,
    current_file: &mut Option<String>,
    diff: &Diff,
) -> bool {
    let mut modified = false;
    ui.vertical(|ui| {
        modified = draw_dir(ui, current_file, None, &diff.dir);
    });
    modified
}

struct AnsiDrawer {
    layout_job: LayoutJob,
    buf: String,

    current_color: Option<egui::Color32>,
    underline: bool,
}

impl Perform for AnsiDrawer {
    fn print(&mut self, c: char) {
        self.buf.push(c);
    }

    fn execute(&mut self, c: u8) {
        if c == b'\n' {
            self.buf.push(c as char);
            self.draw_text();
        }
    }

    fn csi_dispatch(&mut self, params: &Params, _intermediates: &[u8], _ignore: bool, c: u8) {
        let params = params
            .iter()
            .map(|p| p.iter().map(|&x| x as u16).collect::<Vec<u16>>())
            .collect::<Vec<Vec<u16>>>();

        if c == b'm' {
            self.draw_text();
            let param = params[0][0];
            match param {
                0 => {
                    self.current_color = None;
                    self.underline = false;
                }
                1 => { /* bold */ }
                2 => { /* dimmed */ }
                4 => {
                    self.underline = true;
                }
                22 => { /* bold */ }
                24 => {
                    self.underline = false;
                }
                39 => {
                    self.current_color = None;
                }
                91 => {
                    self.current_color = Some(egui::Color32::RED);
                }
                92 => {
                    self.current_color = Some(egui::Color32::GREEN);
                }
                93 => {
                    self.current_color = Some(egui::Color32::LIGHT_YELLOW);
                }
                94 => {
                    self.current_color = Some(egui::Color32::LIGHT_BLUE);
                }
                95 => {
                    self.current_color = Some(egui::Color32::LIGHT_RED);
                }
                96 => {
                    self.current_color = Some(egui::Color32::BLUE);
                }
                //_ => unimplemented!("CSI {:?} {:?}", params, intermediates),
                _ => {}
            }
        }
    }
}

impl AnsiDrawer {
    fn new() -> Self {
        let mut layout_job = LayoutJob::default();
        layout_job.break_on_newline = true;
        Self {
            layout_job,
            buf: String::new(),

            current_color: None,
            underline: false,
        }
    }

    fn draw_text(&mut self) {
        if self.buf.is_empty() {
            return;
        }

        let font_id = FontId::monospace(14.);
        let mut fmt = egui::TextFormat::default();
        fmt.font_id = font_id;

        if let Some(color) = self.current_color {
            fmt.color = color;
        }
        if self.underline {
            fmt.underline = egui::Stroke::new(1.0, fmt.color);
        }
        // TODO: bold
        self.layout_job.append(self.buf.as_str(), 0., fmt);
        self.buf.clear();
    }

    fn draw(mut self, ui: &mut egui::Ui) -> egui::Response {
        self.draw_text();
        ui.add(egui::Label::new(self.layout_job))
    }
}

pub fn ansi(ui: &mut egui::Ui, text: &str) {
    let mut drawer = AnsiDrawer::new();
    let mut state_machine = Parser::<DefaultCharAccumulator>::new();
    for byte in text.bytes() {
        state_machine.advance(&mut drawer, byte);
    }
    drawer.draw(ui);
}
