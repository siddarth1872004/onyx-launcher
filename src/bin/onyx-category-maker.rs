#![windows_subsystem = "windows"]

use std::path::PathBuf;

use eframe::egui;

use onyx_launcher::{config, resource_icon};

fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if "\\/:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

enum Status {
    Idle,
    Error(String),
    Done(PathBuf),
}

struct CategoryMaker {
    name: String,
    icon_path: Option<PathBuf>,
    icon_preview: Option<egui::TextureHandle>,
    status: Status,
}

impl Default for CategoryMaker {
    fn default() -> Self {
        Self {
            name: String::new(),
            icon_path: None,
            icon_preview: None,
            status: Status::Idle,
        }
    }
}

impl CategoryMaker {
    fn create(&mut self, ctx: &egui::Context) {
        let name = sanitize_name(&self.name);
        if name.is_empty() {
            self.status = Status::Error("Enter a category name.".to_string());
            return;
        }
        let Some(icon_path) = &self.icon_path else {
            self.status = Status::Error("Choose an icon image first.".to_string());
            return;
        };

        let result = (|| -> anyhow::Result<PathBuf> {
            let icon_bytes = std::fs::read(icon_path)?;
            let ico_bytes = resource_icon::build_ico(&icon_bytes)?;

            let dest_dir = config::config_dir(Some(&name));
            std::fs::create_dir_all(&dest_dir)?;

            let self_exe = std::env::current_exe()?;
            let src_exe = self_exe
                .parent()
                .ok_or_else(|| anyhow::anyhow!("could not resolve installation folder"))?
                .join("onyx-launcher.exe");
            anyhow::ensure!(
                src_exe.exists(),
                "onyx-launcher.exe not found next to onyx-category-maker.exe"
            );

            let dest_exe = dest_dir.join(format!("{name}.exe"));
            std::fs::copy(&src_exe, &dest_exe)?;
            resource_icon::patch_exe_icon(&dest_exe, &ico_bytes)?;

            Ok(dest_exe)
        })();

        self.status = match result {
            Ok(path) => Status::Done(path),
            Err(e) => Status::Error(e.to_string()),
        };
        ctx.request_repaint();
    }

    fn load_preview(&mut self, ctx: &egui::Context, path: &PathBuf) {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(img) = image::load_from_memory(&bytes) {
                let rgba = img.to_rgba8();
                let (w, h) = rgba.dimensions();
                let color_image =
                    egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                self.icon_preview =
                    Some(ctx.load_texture("icon_preview", color_image, egui::TextureOptions::LINEAR));
            }
        }
    }
}

impl eframe::App for CategoryMaker {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(8.0);
            ui.heading("New Onyx Category");
            ui.label("Creates a standalone, pinnable .exe with its own icon, name, and app list.");
            ui.add_space(16.0);

            ui.label("Category name:");
            ui.text_edit_singleline(&mut self.name);
            ui.add_space(12.0);

            ui.horizontal(|ui| {
                if ui.button("Choose icon...").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Images", &["ico", "png", "jpg", "jpeg", "bmp"])
                        .pick_file()
                    {
                        self.load_preview(&ctx, &path);
                        self.icon_path = Some(path);
                    }
                }
                if let Some(tex) = &self.icon_preview {
                    ui.add(egui::Image::new(tex).max_size(egui::vec2(32.0, 32.0)));
                }
                if let Some(path) = &self.icon_path {
                    if let Some(name) = path.file_name() {
                        ui.label(name.to_string_lossy().to_string());
                    }
                }
            });

            ui.add_space(20.0);

            if ui.button("Create pinnable exe").clicked() {
                self.create(&ctx);
            }

            ui.add_space(16.0);

            match &self.status {
                Status::Idle => {}
                Status::Error(msg) => {
                    ui.colored_label(egui::Color32::from_rgb(220, 80, 80), msg);
                }
                Status::Done(path) => {
                    ui.colored_label(egui::Color32::from_rgb(120, 200, 120), "Created!");
                    ui.label(path.display().to_string());
                    if ui.button("Open containing folder").clicked() {
                        let _ = std::process::Command::new("explorer")
                            .arg(format!("/select,{}", path.display()))
                            .spawn();
                    }
                    ui.label("Right-click the exe and choose \"Pin to taskbar\".");
                }
            }
        });
    }
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::empty();
    fonts.font_data.insert(
        "Ubuntu-Light".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/Ubuntu-Light.ttf"
        ))),
    );
    fonts
        .families
        .insert(egui::FontFamily::Proportional, vec!["Ubuntu-Light".to_owned()]);
    fonts
        .families
        .insert(egui::FontFamily::Monospace, vec!["Ubuntu-Light".to_owned()]);
    ctx.set_fonts(fonts);
}

fn main() {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([420.0, 340.0]),
        ..Default::default()
    };
    let _ = eframe::run_native(
        "Onyx Launcher - New Category",
        native_options,
        Box::new(|cc| {
            setup_fonts(&cc.egui_ctx);
            Ok(Box::new(CategoryMaker::default()))
        }),
    );
}
