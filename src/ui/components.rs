use crate::logic::diff::{
    DiffRenderCommand, DiffRenderFragment, Directory, FileState, FilesystemItem, FolderDiff,
};
use egui::{text::LayoutJob, Color32, Label, Margin, Stroke, TextFormat, TextStyle};

// Catppuccin Mocha
pub const BASE: Color32 = Color32::from_rgb(30, 30, 46);
pub const GREEN: Color32 = Color32::from_rgb(166, 227, 161);
pub const YELLOW: Color32 = Color32::from_rgb(249, 226, 175);
pub const RED: Color32 = Color32::from_rgb(243, 139, 168);

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
                let full_path = if let Some(ref root) = root {
                    format!("{}/{}", root, name)
                } else {
                    name.clone()
                };
                let selected = if let Some(current_file) = current_file {
                    *current_file == *full_path
                } else {
                    false
                };

                let state_color = match state {
                    FileState::Added => GREEN,
                    FileState::Modified => YELLOW,
                    FileState::Removed => RED,
                }
                .gamma_multiply(if selected { 0.5 } else { 0.25 });

                ui.push_id(full_path.clone(), |ui| {
                    let old_wrap_mode = ui.style().wrap_mode;
                    let old_bg_fill = ui.style().visuals.selection.bg_fill;
                    let old_weak_bg_fill = ui.style().visuals.widgets.hovered.weak_bg_fill;

                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    ui.style_mut().visuals.selection.bg_fill = state_color;
                    ui.style_mut().visuals.widgets.hovered.weak_bg_fill = state_color;

                    if ui.selectable_label(selected, name).highlight().clicked() {
                        *current_file = Some(full_path);
                        modified = true;
                    }

                    ui.style_mut().wrap_mode = old_wrap_mode;
                    ui.style_mut().visuals.selection.bg_fill = old_bg_fill;
                    ui.style_mut().visuals.widgets.hovered.weak_bg_fill = old_weak_bg_fill;
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
    diff: &FolderDiff,
) -> bool {
    let mut modified = false;
    ui.vertical(|ui| {
        modified = draw_dir(ui, current_file, None, &diff.dir);
    });
    modified
}

pub fn diff(ui: &mut egui::Ui, diff: &Vec<DiffRenderFragment>, new: bool) {
    let mut layout_job = LayoutJob::default();
    layout_job.break_on_newline = true;

    let mut fmt = TextFormat::default();
    fmt.font_id = TextStyle::Monospace.resolve(ui.style());

    for fragment in diff {
        match &fragment.1 {
            DiffRenderCommand::SetHighlight(highlight) => {
                fmt.background = if *highlight {
                    if new {
                        GREEN
                    } else {
                        RED
                    }
                } else {
                    Color32::TRANSPARENT
                }
                .gamma_multiply(0.25);
            }

            DiffRenderCommand::SetItalic(italic) => {
                fmt.italics = *italic;
            }
            DiffRenderCommand::SetUnderline(underline) => {
                fmt.underline = if *underline {
                    Stroke::new(0., Color32::TRANSPARENT)
                } else {
                    Stroke::new(1., fmt.color)
                };
            }
            DiffRenderCommand::SetColor(color) => {
                fmt.color = *color;
                if !fmt.underline.is_empty() {
                    fmt.underline = Stroke::new(1., fmt.color);
                }
            }

            DiffRenderCommand::Text(text) => {
                layout_job.append(&text, 0., fmt.clone());
            }

            _ => {}
        }
    }

    egui::Frame::default()
        .fill(BASE)
        .inner_margin(Margin::same(8.))
        .show(ui, |ui| {
            ui.add(Label::new(layout_job));
        });
    //ui.add(Label::new(layout_job));
}
