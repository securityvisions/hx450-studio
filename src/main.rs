//! HX450 Studio — a small desktop studio for the HISKA HX-KEG450 panel.

mod font;
mod hid;
mod live;
mod model;
mod theme;

use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Frame, Layout, Margin, Pos2, Rect, Sense,
    Stroke, StrokeKind, TextureHandle, TextureOptions, Vec2,
};

use hid::{Command, DeviceInfo, Event, Worker};
use live::Provider;
use model::{Frame as PixFrame, Screen, H, W};
use theme::*;

const APP_TITLE: &str = "HX450 Studio";
const THUMB_W: usize = 32;
const THUMB_H: usize = 19;
const THUMB_COLS: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Screen,
    Lighting,
    Live,
    Settings,
    Device,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Paint,
    Erase,
    Fill,
    Pick,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tone {
    Info,
    Good,
    Bad,
}

struct App {
    worker: Worker,
    device: Option<DeviceInfo>,
    view: View,

    screen: Screen,
    undo: Vec<Screen>,
    tool: Tool,
    colour: [u8; 3],
    brush: u8,
    show_grid: bool,

    canvas_tex: Option<TextureHandle>,
    atlas_tex: Option<TextureHandle>,
    texture_dirty: bool,

    progress: Option<(String, usize, usize)>,
    status: String,
    tone: Tone,

    light_mode: u8,
    light_colour: [u8; 3],
    light_brightness: u8,
    light_speed: u8,
    light_colourful: bool,
    per_key_colour: [u8; 3],
    per_key_brightness: u8,
    set_win_lock: bool,
    set_alt_tab: bool,
    set_alt_f4: bool,
    set_fn_toggle: bool,
    set_sleep: u8,
    set_debounce: u8,

    live_provider: Provider,
    live_interval: u32,
    live_running: bool,
    live_state: live::LiveState,
    live_preview: PixFrame,
    live_preview_at: Option<Instant>,
    last_push: Option<Instant>,
    metrics: Option<live::Metrics>,
    metrics_at: Option<Instant>,
    log: Vec<(String, String, Tone)>,
    push_failures: u32,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, initial_view: View) -> Self {
        theme::apply(&cc.egui_ctx);
        let mut app = Self {
            worker: Worker::spawn(),
            device: None,
            view: initial_view,
            screen: Screen::single(PixFrame::testcard()),
            undo: Vec::new(),
            tool: Tool::Paint,
            colour: [240, 163, 60],
            brush: 1,
            show_grid: false,
            canvas_tex: None,
            atlas_tex: None,
            texture_dirty: true,
            progress: None,
            status: "looking for the keyboard…".into(),
            tone: Tone::Info,
            light_mode: 1,
            light_colour: [255, 0, 255],
            light_brightness: 5,
            light_speed: 3,
            light_colourful: false,
            per_key_colour: [0, 200, 255],
            per_key_brightness: 5,
            set_win_lock: false,
            set_alt_tab: false,
            set_alt_f4: false,
            set_fn_toggle: false,
            set_sleep: 0,
            set_debounce: 3,
            live_provider: Provider::Clock,
            live_interval: 60,
            live_running: false,
            live_state: live::LiveState::new(),
            live_preview: PixFrame::blank(),
            live_preview_at: None,
            last_push: None,
            metrics: None,
            metrics_at: None,
            log: Vec::new(),
            push_failures: 0,
        };
        app.worker.send(Command::Probe);
        app.live_preview = app.live_state.render(app.live_provider);
        app
    }

    // -- helpers -----------------------------------------------------------
    fn set_status(&mut self, text: impl Into<String>, tone: Tone) {
        let message = text.into();
        if self.status == message {
            return;
        }
        self.status = message.clone();
        self.tone = tone;
        self.log
            .insert(0, (chrono::Local::now().format("%H:%M:%S").to_string(), message, tone));
        self.log.truncate(40);
    }

    /// Append a line to the activity log even when it repeats.
    fn note(&mut self, message: &str, tone: Tone) {
        self.log.insert(
            0,
            (
                chrono::Local::now().format("%H:%M:%S").to_string(),
                message.to_string(),
                tone,
            ),
        );
        self.log.truncate(40);
    }

    fn snapshot(&mut self) {
        self.undo.push(self.screen.clone());
        if self.undo.len() > 30 {
            self.undo.remove(0);
        }
        self.texture_dirty = true;
    }

    fn undo(&mut self) {
        if let Some(previous) = self.undo.pop() {
            self.screen = previous;
            self.texture_dirty = true;
            self.set_status("undo", Tone::Info);
        }
    }

    fn refresh_textures(&mut self, ctx: &egui::Context) {
        if !self.texture_dirty {
            return;
        }
        let frame = self.screen.current();
        let image = egui::ColorImage::from_rgba_unmultiplied([W, H], &frame.to_rgba());
        match self.canvas_tex.as_mut() {
            Some(texture) => texture.set(image, TextureOptions::NEAREST),
            None => {
                self.canvas_tex = Some(ctx.load_texture("canvas", image, TextureOptions::NEAREST))
            }
        }

        let rows = self.screen.frames.len().div_ceil(THUMB_COLS).max(1);
        let atlas_w = THUMB_COLS * THUMB_W;
        let atlas_h = rows * THUMB_H;
        let mut data = vec![0u8; atlas_w * atlas_h * 4];
        for (index, source) in self.screen.frames.iter().enumerate() {
            let origin_x = (index % THUMB_COLS) * THUMB_W;
            let origin_y = (index / THUMB_COLS) * THUMB_H;
            for ty in 0..THUMB_H {
                let sy = ty * H / THUMB_H;
                for tx in 0..THUMB_W {
                    let sx = tx * W / THUMB_W;
                    let pixel = source.px[sy * W + sx];
                    let offset = ((origin_y + ty) * atlas_w + origin_x + tx) * 4;
                    data[offset] = pixel[0];
                    data[offset + 1] = pixel[1];
                    data[offset + 2] = pixel[2];
                    data[offset + 3] = 255;
                }
            }
        }
        let image = egui::ColorImage::from_rgba_unmultiplied([atlas_w, atlas_h], &data);
        match self.atlas_tex.as_mut() {
            Some(texture) => texture.set(image, TextureOptions::NEAREST),
            None => {
                self.atlas_tex = Some(ctx.load_texture("atlas", image, TextureOptions::NEAREST))
            }
        }

        self.texture_dirty = false;
    }

    fn thumbnail_uv(&self, index: usize) -> Rect {
        let rows = self.screen.frames.len().div_ceil(THUMB_COLS).max(1);
        let atlas_w = (THUMB_COLS * THUMB_W) as f32;
        let atlas_h = (rows * THUMB_H) as f32;
        let x = ((index % THUMB_COLS) * THUMB_W) as f32 / atlas_w;
        let y = ((index / THUMB_COLS) * THUMB_H) as f32 / atlas_h;
        Rect::from_min_size(
            Pos2::new(x, y),
            Vec2::new(THUMB_W as f32 / atlas_w, THUMB_H as f32 / atlas_h),
        )
    }

    // -- top bar -----------------------------------------------------------
    fn top_bar(&mut self, ctx: &egui::Context) {
        let frame = Frame::new()
            .fill(PANEL)
            .inner_margin(Margin::symmetric(14, 8))
            .stroke(Stroke::new(1.0_f32, BORDER));
        egui::TopBottomPanel::top("top")
            .frame(frame)
            .exact_height(46.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(22.0, 22.0), Sense::hover());
                    accent_mark(ui.painter(), rect);
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(APP_TITLE).strong().size(15.0).color(TEXT));
                    ui.add_space(16.0);

                    for (view, label) in [
                        (View::Screen, "Screen"),
                        (View::Lighting, "Lighting"),
                        (View::Live, "Live"),
                        (View::Settings, "Settings"),
                        (View::Device, "Device"),
                    ] {
                        let selected = self.view == view;
                        let text = egui::RichText::new(label)
                            .size(13.0)
                            .color(if selected { TEXT } else { TEXT_DIM });
                        let button = egui::Button::new(text)
                            .fill(if selected { ACTIVE } else { Color32::TRANSPARENT })
                            .stroke(if selected {
                                Stroke::new(1.0_f32, BORDER_HI)
                            } else {
                                Stroke::NONE
                            })
                            .corner_radius(CornerRadius::same(7))
                            .min_size(Vec2::new(0.0, 26.0));
                        if ui.add(button).clicked() {
                            self.view = view;
                        }
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        self.device_chip(ui);
                    });
                });
            });
    }

    fn device_chip(&mut self, ui: &mut egui::Ui) {
        let connected = self.device.is_some();
        let (dot, label) = if connected {
            (GOOD, self.device.as_ref().map(|d| d.product.clone()).unwrap_or_default())
        } else {
            (BAD, "no device".to_string())
        };
        let text =
            egui::RichText::new(label).size(12.0).color(if connected { TEXT } else { TEXT_DIM });
        let button = egui::Button::new(text)
            .fill(CARD)
            .stroke(Stroke::new(1.0_f32, BORDER))
            .corner_radius(CornerRadius::same(20))
            .min_size(Vec2::new(0.0, 26.0));
        let response = ui.add(button);
        let rect = response.rect;
        ui.painter()
            .circle_filled(Pos2::new(rect.left() + 12.0, rect.center().y), 3.5, dot);
        if response.clicked() {
            self.worker.send(Command::Probe);
        }
        ui.add_space(4.0);
    }

    // -- left rail ---------------------------------------------------------
    fn rail(&mut self, ctx: &egui::Context) {
        let frame = Frame::new()
            .fill(BG)
            .inner_margin(Margin::symmetric(8, 12))
            .stroke(Stroke::new(1.0_f32, BORDER));
        egui::SidePanel::left("rail")
            .frame(frame)
            .exact_width(58.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    for (view, kind) in [
                        (View::Screen, IconKind::Monitor),
                        (View::Lighting, IconKind::Bulb),
                        (View::Live, IconKind::Pulse),
                        (View::Settings, IconKind::Sliders),
                        (View::Device, IconKind::Chip),
                    ] {
                        let selected = self.view == view;
                        let (rect, response) =
                            ui.allocate_exact_size(Vec2::new(38.0, 38.0), Sense::click());
                        let painter = ui.painter();
                        if selected {
                            painter.rect_filled(rect, CornerRadius::same(9), ACTIVE);
                            painter.rect_stroke(
                                rect,
                                CornerRadius::same(9),
                                Stroke::new(1.0_f32, BORDER_HI),
                                StrokeKind::Inside,
                            );
                        } else if response.hovered() {
                            painter.rect_filled(rect, CornerRadius::same(9), HOVER);
                        }
                        draw_icon(painter, rect.center(), kind, if selected { ACCENT } else { TEXT_DIM });
                        if response.clicked() {
                            self.view = view;
                        }
                        ui.add_space(6.0);
                    }
                });
            });
    }

    // -- status bar --------------------------------------------------------
    fn status_bar(&mut self, ctx: &egui::Context) {
        let frame = Frame::new()
            .fill(BG)
            .inner_margin(Margin::symmetric(14, 4))
            .stroke(Stroke::new(1.0_f32, BORDER));
        egui::TopBottomPanel::bottom("status")
            .frame(frame)
            .exact_height(30.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    let colour = match self.tone {
                        Tone::Info => TEXT_DIM,
                        Tone::Good => GOOD,
                        Tone::Bad => BAD,
                    };
                    ui.label(egui::RichText::new("●").size(9.0).color(colour));
                    ui.add_space(2.0);
                    ui.label(egui::RichText::new(&self.status).size(12.0).color(colour));

                    if let Some((label, done, total)) = self.progress.clone() {
                        ui.add_space(12.0);
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(180.0, 6.0), Sense::hover());
                        let fraction = if total == 0 { 0.0 } else { done as f32 / total as f32 };
                        let painter = ui.painter();
                        painter.rect_filled(rect, CornerRadius::same(3), INPUT);
                        let mut filled = rect;
                        filled.set_width(rect.width() * fraction);
                        painter.rect_filled(filled, CornerRadius::same(3), ACCENT);
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(format!("{label} {done}/{total}"))
                                .size(11.0)
                                .color(TEXT_DIM),
                        );
                    }

                    if self.live_running {
                        ui.add_space(10.0);
                        let stale = self
                            .last_push
                            .map(|at| at.elapsed().as_secs() > self.live_interval as u64 * 2)
                            .unwrap_or(true);
                        let (colour, text) = match self.last_push {
                            Some(at) if !stale => (
                                GOOD,
                                format!("live · pushed {}s ago", at.elapsed().as_secs()),
                            ),
                            Some(at) => (
                                WARN,
                                format!("live · stale {}s", at.elapsed().as_secs()),
                            ),
                            None => (WARN, "live · waiting for first push".to_string()),
                        };
                        ui.label(egui::RichText::new("●").size(8.0).color(colour));
                        ui.label(egui::RichText::new(text).size(11.0).color(colour));
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "{} pages    {} frame(s)    {} ms",
                                self.screen.page_count(),
                                self.screen.frames.len(),
                                self.screen.total_ms()
                            ))
                            .size(11.0)
                            .color(TEXT_FAINT),
                        );
                    });
                });
            });
    }

    // -- central workspace -------------------------------------------------
    fn workspace(&mut self, ctx: &egui::Context) {
        let frame = Frame::new().fill(BG).inner_margin(Margin::same(16));
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| match self.view {
            View::Screen => self.screen_view(ui),
            View::Lighting => self.lighting_view(ui),
            View::Live => self.live_view(ui),
            View::Settings => self.settings_view(ui),
            View::Device => self.device_view(ui),
        });
    }

    fn screen_view(&mut self, ui: &mut egui::Ui) {
        Frame::new()
            .fill(CARD)
            .stroke(Stroke::new(1.0_f32, BORDER))
            .corner_radius(CornerRadius::same(9))
            .inner_margin(Margin::symmetric(12, 9))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Screen").size(14.0).strong().color(TEXT));
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "{W} x {H}   frame {:02}/{:02}   {} ms",
                            self.screen.selected + 1,
                            self.screen.frames.len(),
                            self.screen.current().delay_ms
                        ))
                        .size(11.0)
                        .color(TEXT_DIM),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if primary_button(ui, "Upload to keyboard").clicked() {
                            let payload = self.screen.encode();
                            let pages = payload.len() / model::PAGE;
                            self.progress = Some(("uploading".into(), 0, pages));
                            self.worker.send(Command::Upload(payload));
                            self.set_status("uploading…", Tone::Info);
                        }
                        if ui.button("Import image / GIF").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("images", &["png", "jpg", "jpeg", "gif", "webp", "bmp"])
                                .pick_file()
                            {
                                match self.screen.import(&path) {
                                    Ok(count) => {
                                        self.undo.clear();
                                        self.texture_dirty = true;
                                        self.set_status(
                                            format!(
                                                "imported {count} frame(s) from {}",
                                                path.display()
                                            ),
                                            Tone::Good,
                                        );
                                    }
                                    Err(message) => self.set_status(message, Tone::Bad),
                                }
                            }
                        }
                        if ui.button("Test card").clicked() {
                            self.snapshot();
                            self.screen = Screen::single(PixFrame::testcard());
                            self.set_status("test card loaded", Tone::Info);
                        }
                    });
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("TOOL").size(10.0).color(TEXT_FAINT));
                    for (tool, label) in [
                        (Tool::Paint, "Paint"),
                        (Tool::Erase, "Erase"),
                        (Tool::Fill, "Fill"),
                        (Tool::Pick, "Pick"),
                    ] {
                        let selected = self.tool == tool;
                        let text = egui::RichText::new(label)
                            .size(12.0)
                            .color(if selected { TEXT } else { TEXT_DIM });
                        let button = egui::Button::new(text)
                            .fill(if selected { ACTIVE } else { INPUT })
                            .stroke(Stroke::new(1.0_f32, if selected { ACCENT } else { BORDER }))
                            .corner_radius(CornerRadius::same(7));
                        if ui.add(button).clicked() {
                            self.tool = tool;
                        }
                    }
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new("COLOUR").size(10.0).color(TEXT_FAINT));
                    ui.color_edit_button_srgb(&mut self.colour);
                    ui.add(egui::DragValue::new(&mut self.brush).range(1..=8).prefix("brush "));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Undo").clicked() {
                            self.undo();
                        }
                        ui.checkbox(&mut self.show_grid, "grid");
                    });
                });
            });

        ui.add_space(12.0);

        let footer_height = 108.0;
        let available = ui.available_size();
        let canvas_area = Vec2::new(available.x, (available.y - footer_height).max(80.0));

        let scale = ((canvas_area.x - 32.0) / W as f32)
            .min((canvas_area.y - 32.0) / H as f32)
            .floor()
            .clamp(1.0, 7.0);

        let (area_rect, _) = ui.allocate_exact_size(canvas_area, Sense::hover());
        let size = Vec2::new(W as f32 * scale, H as f32 * scale);
        let canvas_rect = Rect::from_center_size(area_rect.center(), size);

        let painter = ui.painter();
        painter.rect_filled(
            area_rect.shrink(1.0),
            CornerRadius::same(10),
            Color32::from_rgb(0x0c, 0x0d, 0x10),
        );
        painter.rect_filled(canvas_rect.translate(Vec2::new(0.0, 6.0)), CornerRadius::same(4), Color32::from_black_alpha(120));
        if let Some(texture) = &self.canvas_tex {
            painter.image(
                texture.id(),
                canvas_rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        painter.rect_stroke(
            canvas_rect,
            CornerRadius::same(3),
            Stroke::new(1.0_f32, BORDER_HI),
            StrokeKind::Outside,
        );

        if self.show_grid && scale >= 4.0 {
            let grid = Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 16));
            for x in (0..=W).step_by(16) {
                let px = canvas_rect.left() + x as f32 * scale;
                painter.line_segment(
                    [Pos2::new(px, canvas_rect.top()), Pos2::new(px, canvas_rect.bottom())],
                    grid,
                );
            }
            for y in (0..=H).step_by(16) {
                let py = canvas_rect.top() + y as f32 * scale;
                painter.line_segment(
                    [Pos2::new(canvas_rect.left(), py), Pos2::new(canvas_rect.right(), py)],
                    grid,
                );
            }
        }

        let response = ui.interact(canvas_rect, ui.id().with("canvas"), Sense::click_and_drag());
        if let Some(pos) = response.interact_pointer_pos() {
            if canvas_rect.contains(pos)
                && (response.clicked() || response.dragged() || response.drag_started())
            {
                if response.drag_started() || response.clicked() {
                    self.snapshot();
                }
                let x = ((pos.x - canvas_rect.left()) / scale) as isize;
                let y = ((pos.y - canvas_rect.top()) / scale) as isize;
                let colour = self.colour;
                let radius = self.brush as isize - 1;
                match self.tool {
                    Tool::Paint => self.screen.current_mut().brush(x, y, radius, colour),
                    Tool::Erase => self.screen.current_mut().brush(x, y, radius, [0, 0, 0]),
                    Tool::Fill => self.screen.current_mut().fill(colour),
                    Tool::Pick => {
                        let picked =
                            self.screen.current().get(x.max(0) as usize, y.max(0) as usize);
                        self.colour = picked;
                    }
                }
                self.texture_dirty = true;
            }
        }

        ui.add_space(10.0);
        self.timeline(ui);
    }

    fn timeline(&mut self, ui: &mut egui::Ui) {
        Frame::new()
            .fill(CARD)
            .stroke(Stroke::new(1.0_f32, BORDER))
            .corner_radius(CornerRadius::same(9))
            .inner_margin(Margin::same(8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("+").on_hover_text("duplicate frame").clicked() {
                        self.snapshot();
                        self.screen.duplicate();
                    }
                    if ui.button("−").on_hover_text("delete frame").clicked() {
                        self.snapshot();
                        self.screen.delete_frame();
                    }
                    ui.separator();
                    ui.label(egui::RichText::new("DELAY").size(10.0).color(TEXT_FAINT));
                    let mut delay = self.screen.current().delay_ms;
                    if ui
                        .add(
                            egui::DragValue::new(&mut delay)
                                .range(20..=2000)
                                .speed(5)
                                .suffix(" ms"),
                        )
                        .changed()
                    {
                        self.screen.current_mut().delay_ms = delay;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "{} frames  ·  {} ms total",
                                self.screen.frames.len(),
                                self.screen.total_ms()
                            ))
                            .size(11.0)
                            .color(TEXT_FAINT),
                        );
                    });
                });

                ui.add_space(6.0);
                let atlas = self.atlas_tex.clone();
                let mut select: Option<usize> = None;
                egui::ScrollArea::horizontal()
                    .max_height(52.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for index in 0..self.screen.frames.len() {
                                let (rect, response) =
                                    ui.allocate_exact_size(Vec2::new(48.0, 42.0), Sense::click());
                                let painter = ui.painter();
                                let selected = index == self.screen.selected;
                                painter.rect_filled(rect, CornerRadius::same(6), INPUT);
                                let thumb = Rect::from_min_size(
                                    rect.min + Vec2::new(4.0, 4.0),
                                    Vec2::new(40.0, 24.0),
                                );
                                if let Some(atlas) = &atlas {
                                    painter.image(
                                        atlas.id(),
                                        thumb,
                                        self.thumbnail_uv(index),
                                        Color32::WHITE,
                                    );
                                }
                                painter.text(
                                    Pos2::new(rect.left() + 4.0, rect.bottom() - 10.0),
                                    Align2::LEFT_CENTER,
                                    format!("{:02}", index + 1),
                                    FontId::proportional(10.0),
                                    if selected { ACCENT } else { TEXT_FAINT },
                                );
                                painter.text(
                                    Pos2::new(rect.right() - 4.0, rect.bottom() - 10.0),
                                    Align2::RIGHT_CENTER,
                                    format!("{}", self.screen.frames[index].delay_ms),
                                    FontId::proportional(10.0),
                                    TEXT_FAINT,
                                );
                                painter.rect_stroke(
                                    rect,
                                    CornerRadius::same(6),
                                    Stroke::new(
                                        if selected { 1.5_f32 } else { 1.0_f32 },
                                        if selected { ACCENT } else { BORDER },
                                    ),
                                    StrokeKind::Inside,
                                );
                                if response.clicked() {
                                    select = Some(index);
                                }
                            }
                        });
                    });
                if let Some(index) = select {
                    self.screen.selected = index;
                    self.texture_dirty = true;
                }
            });
    }

    // -- lighting ----------------------------------------------------------
    fn lighting_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_top(|ui| {
            let total = ui.available_width();
            let left_width = (total * 0.58).max(420.0);
            ui.allocate_ui_with_layout(
                Vec2::new(left_width, ui.available_height()),
                Layout::top_down(Align::Min),
                |ui| {
                ui.label(egui::RichText::new("Lighting").size(15.0).strong().color(TEXT));
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new("Twenty onboard effects. Pick one, then shape the colour.")
                        .size(12.0)
                        .color(TEXT_DIM),
                );
                ui.add_space(10.0);

                Frame::new()
                    .fill(CARD)
                    .stroke(Stroke::new(1.0_f32, BORDER))
                    .corner_radius(CornerRadius::same(9))
                    .inner_margin(Margin::same(12))
                    .show(ui, |ui| {
                        let names = [
                            "Static",
                            "Single On",
                            "Single Off",
                            "Glittering",
                            "Fluttering",
                            "Colourful",
                            "Breath",
                            "Spectrum",
                            "Outward",
                            "Scrolling",
                            "Rolling",
                            "Rotating",
                            "Explode",
                            "Launch",
                            "Ripples",
                            "Flowing",
                            "Pulsating",
                            "Tilt",
                            "Shuttle",
                            "Off",
                        ];
                        egui::Grid::new("modes")
                            .num_columns(4)
                            .spacing([8.0, 8.0])
                            .show(ui, |ui| {
                                for (index, name) in names.iter().enumerate() {
                                    let mode = (index + 1) as u8;
                                    let selected = self.light_mode == mode;
                                    let text = egui::RichText::new(*name).size(12.0).color(
                                        if selected { TEXT } else { TEXT_DIM },
                                    );
                                    let button = egui::Button::new(text)
                                        .fill(if selected { ACTIVE } else { INPUT })
                                        .stroke(Stroke::new(
                                            1.0_f32,
                                            if selected { ACCENT } else { BORDER },
                                        ))
                                        .corner_radius(CornerRadius::same(7))
                                        .min_size(Vec2::new(124.0, 30.0));
                                    if ui.add(button).clicked() {
                                        self.light_mode = mode;
                                    }
                                    if (index + 1) % 4 == 0 {
                                        ui.end_row();
                                    }
                                }
                            });
                    });
                ui.add_space(12.0);
                self.lighting_preview(ui);
            });

            ui.add_space(14.0);
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), ui.available_height()),
                Layout::top_down(Align::Min),
                |ui| {
                Frame::new()
                    .fill(CARD)
                    .stroke(Stroke::new(1.0_f32, BORDER))
                    .corner_radius(CornerRadius::same(9))
                    .inner_margin(Margin::same(14))
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("COLOUR").size(10.0).color(TEXT_FAINT));
                        ui.add_space(8.0);
                        let (swatch, _) =
                            ui.allocate_exact_size(Vec2::new(ui.available_width(), 54.0), Sense::hover());
                        let colour = Color32::from_rgb(
                            self.light_colour[0],
                            self.light_colour[1],
                            self.light_colour[2],
                        );
                        ui.painter().rect_filled(swatch, CornerRadius::same(7), colour);
                        ui.painter().rect_stroke(
                            swatch,
                            CornerRadius::same(7),
                            Stroke::new(1.0_f32, BORDER_HI),
                            StrokeKind::Inside,
                        );

                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            ui.color_edit_button_srgb(&mut self.light_colour);
                            ui.label(
                                egui::RichText::new(format!(
                                    "#{:02X}{:02X}{:02X}",
                                    self.light_colour[0],
                                    self.light_colour[1],
                                    self.light_colour[2]
                                ))
                                .monospace()
                                .size(12.0)
                                .color(TEXT_DIM),
                            );
                        });

                        ui.add_space(8.0);
                        ui.horizontal_wrapped(|ui| {
                            for preset in [
                                [255u8, 0u8, 0u8],
                                [255, 120, 0],
                                [255, 220, 0],
                                [80, 220, 120],
                                [80, 190, 255],
                                [120, 120, 255],
                                [255, 0, 180],
                                [255, 255, 255],
                            ] {
                                let (rect, response) =
                                    ui.allocate_exact_size(Vec2::new(26.0, 18.0), Sense::click());
                                let colour = Color32::from_rgb(preset[0], preset[1], preset[2]);
                                ui.painter().rect_filled(rect, CornerRadius::same(4), colour);
                                ui.painter().rect_stroke(
                                    rect,
                                    CornerRadius::same(4),
                                    Stroke::new(
                                        1.0_f32,
                                        if self.light_colour == preset { ACCENT } else { BORDER },
                                    ),
                                    StrokeKind::Inside,
                                );
                                if response.clicked() {
                                    self.light_colour = preset;
                                }
                            }
                        });

                        ui.add_space(12.0);
                        ui.label(egui::RichText::new("MOTION").size(10.0).color(TEXT_FAINT));
                        ui.add_space(6.0);
                        slider_row(ui, "brightness", &mut self.light_brightness, 5);
                        ui.add_space(2.0);
                        slider_row(ui, "speed", &mut self.light_speed, 5);
                        ui.add_space(6.0);
                        ui.checkbox(&mut self.light_colourful, "rainbow palette");

                        ui.add_space(12.0);
                        let full_width = ui.available_width();
                        if ui
                            .add_sized(
                                [full_width, 30.0],
                                egui::Button::new(
                                    egui::RichText::new("Apply lighting")
                                        .size(13.0)
                                        .color(Color32::from_rgb(0x14, 0x10, 0x08)),
                                )
                                .fill(ACCENT)
                                .stroke(Stroke::NONE)
                                .corner_radius(CornerRadius::same(7)),
                            )
                            .clicked()
                        {
                            self.worker.send(Command::Light {
                                mode: self.light_mode,
                                colour: (
                                    self.light_colour[0],
                                    self.light_colour[1],
                                    self.light_colour[2],
                                ),
                                brightness: self.light_brightness,
                                speed: self.light_speed,
                                colourful: self.light_colourful,
                            });
                            self.set_status("applying lighting…", Tone::Info);
                        }
                        ui.add_space(4.0);
                        if ui
                            .add_sized([ui.available_width(), 26.0], egui::Button::new("Clock sync"))
                            .clicked()
                        {
                            self.worker.send(Command::ClockSync);
                            self.set_status("syncing clock…", Tone::Info);
                        }
                    });
            });
        });
        ui.add_space(12.0);
        self.activity_card(ui);
    }

    // -- live --------------------------------------------------------------
    fn live_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_top(|ui| {
            let preview_scale = 3.0;
            let preview_size = Vec2::new(W as f32 * preview_scale, H as f32 * preview_scale);
            ui.allocate_ui_with_layout(
                Vec2::new(preview_size.x + 28.0, ui.available_height()),
                Layout::top_down(Align::Min),
                |ui| {
                Frame::new()
                    .fill(CARD)
                    .stroke(Stroke::new(1.0_f32, BORDER))
                    .corner_radius(CornerRadius::same(9))
                    .inner_margin(Margin::same(14))
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("PREVIEW").size(10.0).color(TEXT_FAINT));
                        ui.add_space(8.0);
                        let image = egui::ColorImage::from_rgba_unmultiplied(
                            [W, H],
                            &self.live_preview.to_rgba(),
                        );
                        let texture =
                            ui.ctx().load_texture("live-preview", image, TextureOptions::NEAREST);
                        let (rect, _) = ui.allocate_exact_size(preview_size, Sense::hover());
                        let painter = ui.painter();
                        painter.rect_filled(rect, CornerRadius::same(4), Color32::BLACK);
                        painter.image(
                            texture.id(),
                            rect,
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                            Color32::WHITE,
                        );
                        painter.rect_stroke(
                            rect,
                            CornerRadius::same(4),
                            Stroke::new(1.0_f32, BORDER_HI),
                            StrokeKind::Outside,
                        );
                        ui.add_space(10.0);
                        match self.last_push {
                            Some(last) => ui.label(
                                egui::RichText::new(format!(
                                    "last push {}s ago",
                                    last.elapsed().as_secs()
                                ))
                                .size(11.0)
                                .color(TEXT_FAINT),
                            ),
                            None => ui.label(
                                egui::RichText::new("nothing pushed yet").size(11.0).color(TEXT_FAINT),
                            ),
                        };
                    });
            });

            ui.add_space(14.0);
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), ui.available_height()),
                Layout::top_down(Align::Min),
                |ui| {
                Frame::new()
                    .fill(CARD)
                    .stroke(Stroke::new(1.0_f32, BORDER))
                    .corner_radius(CornerRadius::same(9))
                    .inner_margin(Margin::same(14))
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("PROVIDER").size(10.0).color(TEXT_FAINT));
                        ui.add_space(8.0);
                        for provider in Provider::ALL {
                            let selected = self.live_provider == provider;
                            let response = egui::Frame::new()
                                .fill(if selected { ACTIVE } else { INPUT })
                                .stroke(Stroke::new(1.0_f32, if selected { ACCENT } else { BORDER }))
                                .corner_radius(CornerRadius::same(8))
                                .inner_margin(Margin::same(10))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.vertical(|ui| {
                                        ui.label(
                                            egui::RichText::new(provider.label())
                                                .size(13.0)
                                                .strong()
                                                .color(if selected { TEXT } else { TEXT_DIM }),
                                        );
                                        ui.label(
                                            egui::RichText::new(provider.blurb())
                                                .size(11.0)
                                                .color(TEXT_FAINT),
                                        );
                                    });
                                })
                                .response
                                .interact(Sense::click());
                            if response.clicked() {
                                self.live_provider = provider;
                                self.live_preview = self.live_state.render(provider);
                                self.live_preview_at = Some(Instant::now());
                            }
                            ui.add_space(6.0);
                        }

                        ui.add_space(10.0);
                        ui.label(egui::RichText::new("INTERVAL").size(10.0).color(TEXT_FAINT));
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::DragValue::new(&mut self.live_interval)
                                    .range(10..=3600)
                                    .speed(1)
                                    .suffix(" s"),
                            );
                            if self.live_running {
                                if ui.button("Stop").clicked() {
                                    self.worker.send(Command::StopLive);
                                    self.live_running = false;
                                    self.set_status("live stopped", Tone::Info);
                                }
                            } else if primary_button(ui, "Start").clicked() {
                                if self.live_interval < 30 {
                                    self.set_status(
                                        "use 30 s or more — every push rewrites flash",
                                        Tone::Bad,
                                    );
                                } else {
                                    self.worker.send(Command::StartLive {
                                        provider: self.live_provider,
                                        interval_secs: self.live_interval,
                                    });
                                    self.live_running = true;
                                    self.set_status("live running", Tone::Good);
                                }
                            }
                            if ui.button("Sync clock").clicked() {
                                self.worker.send(Command::ClockSync);
                                self.set_status("syncing clock…", Tone::Info);
                            }
                            if ui.button("Push once").clicked() {
                                let payload =
                                    Screen::single(self.live_state.render(self.live_provider))
                                        .encode();
                                let pages = payload.len() / model::PAGE;
                                self.progress = Some(("uploading".into(), 0, pages));
                                self.worker.send(Command::Upload(payload));
                                self.set_status("pushing one frame…", Tone::Info);
                            }
                        });
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(
                                "Each push rewrites the stored animation in flash; keep it at 30 s or longer.",
                            )
                            .size(11.0)
                            .color(TEXT_FAINT),
                        );
                    });

                ui.add_space(12.0);
                Frame::new()
                    .fill(CARD)
                    .stroke(Stroke::new(1.0_f32, BORDER))
                    .corner_radius(CornerRadius::same(9))
                    .inner_margin(Margin::same(14))
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("READOUT").size(10.0).color(TEXT_FAINT));
                        ui.add_space(8.0);
                        match self.metrics {
                            Some(metrics) => {
                                egui::Grid::new("metrics")
                                    .num_columns(2)
                                    .spacing([16.0, 6.0])
                                    .show(ui, |ui| {
                                        metric_row(ui, "cpu", &format!("{:.0} %", metrics.cpu));
                                        metric_row(ui, "ram", &format!("{:.0} %", metrics.ram));
                                        if let Some(temp) = metrics.temp {
                                            metric_row(ui, "temp", &format!("{temp:.0} C"));
                                        }
                                        let context = (metrics.context_tokens as f32
                                            / 200_000.0
                                            * 100.0)
                                            .min(100.0);
                                        metric_row(ui, "context", &format!("{context:.0} %"));
                                    });
                            }
                            None => {
                                ui.label(
                                    egui::RichText::new("sampling…").size(12.0).color(TEXT_FAINT),
                                );
                            }
                        }
                    });
            });
        });
        ui.add_space(12.0);
        self.activity_card(ui);
    }


    // -- settings ----------------------------------------------------------
    fn settings_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                Vec2::new((ui.available_width() * 0.55).max(360.0), ui.available_height()),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.label(egui::RichText::new("Settings").size(15.0).strong().color(TEXT));
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new(
                            "The 04 17 system-settings block. Field offsets are verified in the \
                             vendor binary; the value meanings below are inferred.",
                        )
                        .size(12.0)
                        .color(TEXT_DIM),
                    );
                    ui.add_space(10.0);

                    Frame::new()
                        .fill(CARD)
                        .stroke(Stroke::new(1.0_f32, BORDER))
                        .corner_radius(CornerRadius::same(9))
                        .inner_margin(Margin::same(14))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width() - 8.0);
                            ui.label(egui::RichText::new("KEY LOCKS").size(10.0).color(TEXT_FAINT));
                            ui.add_space(8.0);
                            ui.checkbox(&mut self.set_win_lock, "Disable the Windows key");
                            ui.checkbox(&mut self.set_alt_f4, "Disable Alt + F4");
                            ui.checkbox(&mut self.set_alt_tab, "Disable Alt + Tab");

                            ui.add_space(12.0);
                            ui.label(egui::RichText::new("BEHAVIOUR").size(10.0).color(TEXT_FAINT));
                            ui.add_space(8.0);
                            ui.checkbox(&mut self.set_fn_toggle, "Fn switch is a toggle (off = momentary)");

                            ui.add_space(10.0);
                            ui.label(egui::RichText::new("sleep when idle").size(11.0).color(TEXT_DIM));
                            ui.horizontal(|ui| {
                                for (value, label) in [(0u8, "off"), (1, "1 min"), (2, "5 min"), (3, "30 min")] {
                                    let selected = self.set_sleep == value;
                                    let text = egui::RichText::new(label)
                                        .size(12.0)
                                        .color(if selected { TEXT } else { TEXT_DIM });
                                    let button = egui::Button::new(text)
                                        .fill(if selected { ACTIVE } else { INPUT })
                                        .stroke(Stroke::new(
                                            1.0_f32,
                                            if selected { ACCENT } else { BORDER },
                                        ))
                                        .corner_radius(CornerRadius::same(7));
                                    if ui.add(button).clicked() {
                                        self.set_sleep = value;
                                    }
                                }
                            });

                            ui.add_space(10.0);
                            ui.label(
                                egui::RichText::new(format!("key response (debounce level {})", self.set_debounce))
                                    .size(11.0)
                                    .color(TEXT_DIM),
                            );
                            slider_row(ui, "level", &mut self.set_debounce, 5);

                            ui.add_space(14.0);
                            if primary_button(ui, "Apply settings").clicked() {
                                self.worker.send(Command::Settings(hid::Settings {
                                    win_lock: self.set_win_lock,
                                    alt_tab: self.set_alt_tab,
                                    alt_f4: self.set_alt_f4,
                                    fn_toggle: self.set_fn_toggle,
                                    sleep: self.set_sleep,
                                    debounce: self.set_debounce,
                                }));
                                self.set_status("applying settings…", Tone::Info);
                            }
                        });

                    ui.add_space(12.0);
                    Frame::new()
                        .fill(CARD)
                        .stroke(Stroke::new(1.0_f32, BORDER))
                        .corner_radius(CornerRadius::same(9))
                        .inner_margin(Margin::same(14))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width() - 8.0);
                            ui.label(egui::RichText::new("CLOCK").size(10.0).color(TEXT_FAINT));
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new(
                                    "Push the host clock to the keyboard RTC (04 28). The panel shows \
                                     whatever frame we upload, but the keyboard's own clock mode uses this.",
                                )
                                .size(11.0)
                                .color(TEXT_DIM),
                            );
                            ui.add_space(8.0);
                            if ui.button("Sync clock now").clicked() {
                                self.worker.send(Command::ClockSync);
                                self.set_status("syncing clock…", Tone::Info);
                            }
                        });
                },
            );

            ui.add_space(14.0);
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), ui.available_height()),
                Layout::top_down(Align::Min),
                |ui| {
                    Frame::new()
                        .fill(CARD)
                        .stroke(Stroke::new(1.0_f32, BORDER))
                        .corner_radius(CornerRadius::same(9))
                        .inner_margin(Margin::same(14))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width() - 8.0);
                            ui.label(egui::RichText::new("PER-KEY COLOURS").size(10.0).color(TEXT_FAINT));
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new(
                                    "Paint every key one colour with 04 23. Enters user-lighting mode \
                                     first, then writes 144 colour records.",
                                )
                                .size(11.0)
                                .color(TEXT_DIM),
                            );
                            ui.add_space(10.0);
                            let (swatch, _) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 44.0),
                                Sense::hover(),
                            );
                            ui.painter().rect_filled(
                                swatch,
                                CornerRadius::same(7),
                                Color32::from_rgb(
                                    self.per_key_colour[0],
                                    self.per_key_colour[1],
                                    self.per_key_colour[2],
                                ),
                            );
                            ui.painter().rect_stroke(
                                swatch,
                                CornerRadius::same(7),
                                Stroke::new(1.0_f32, BORDER_HI),
                                StrokeKind::Inside,
                            );
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.color_edit_button_srgb(&mut self.per_key_colour);
                                ui.label(
                                    egui::RichText::new(format!(
                                        "#{:02X}{:02X}{:02X}",
                                        self.per_key_colour[0],
                                        self.per_key_colour[1],
                                        self.per_key_colour[2]
                                    ))
                                    .monospace()
                                    .size(12.0)
                                    .color(TEXT_DIM),
                                );
                            });
                            slider_row(ui, "brightness", &mut self.per_key_brightness, 5);
                            ui.add_space(10.0);
                            if ui.button("Fill every key").clicked() {
                                self.worker.send(Command::PerKeyFill {
                                    colour: (
                                        self.per_key_colour[0],
                                        self.per_key_colour[1],
                                        self.per_key_colour[2],
                                    ),
                                    brightness: self.per_key_brightness,
                                });
                                self.set_status("writing per-key colours…", Tone::Info);
                            }
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new(
                                    "This path is documented from the binary and was replayed here, \
                                     but it is not yet confirmed on hardware — the effect mode is \
                                     still the safe default.",
                                )
                                .size(10.0)
                                .color(TEXT_FAINT),
                            );
                        });

                    ui.add_space(12.0);
                    self.activity_card(ui);
                },
            );
        });
    }

    // -- device ------------------------------------------------------------
    fn device_view(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("Device").size(15.0).strong().color(TEXT));
        ui.add_space(12.0);

        Frame::new()
            .fill(CARD)
            .stroke(Stroke::new(1.0_f32, BORDER))
            .corner_radius(CornerRadius::same(9))
            .inner_margin(Margin::same(14))
            .show(ui, |ui| match &self.device {
                Some(info) => {
                    ui.set_min_width(ui.available_width() - 40.0);
                    ui.label(egui::RichText::new(&info.product).size(13.0).strong());
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(format!("image   {}", info.image))
                            .monospace()
                            .size(11.0)
                            .color(TEXT_DIM),
                    );
                    ui.label(
                        egui::RichText::new(format!("control {}", info.control))
                            .monospace()
                            .size(11.0)
                            .color(TEXT_DIM),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "firmware {}",
                            hid::firmware_version().unwrap_or_else(|| "unknown".into())
                        ))
                        .monospace()
                        .size(11.0)
                        .color(TEXT_DIM),
                    );
                }
                None => {
                    ui.label(egui::RichText::new("Not connected").size(13.0).color(BAD));
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(
                            "Plug in the USB-C cable. The panel is disabled over 2.4 GHz and \
                             Bluetooth, and this tool needs the udev rule installed.",
                        )
                        .size(12.0)
                        .color(TEXT_DIM),
                    );
                }
            });

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Rescan").clicked() {
                self.worker.send(Command::Probe);
                self.set_status("scanning…", Tone::Info);
            }
            if ui.button("Sync clock").clicked() {
                self.worker.send(Command::ClockSync);
            }
        });

        ui.add_space(12.0);
        Frame::new()
            .fill(CARD)
            .stroke(Stroke::new(1.0_f32, BORDER))
            .corner_radius(CornerRadius::same(9))
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width() - 40.0);
                ui.label(egui::RichText::new("PANEL").size(10.0).color(TEXT_FAINT));
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!(
                        "{W} x {H} px  ·  RGB565  ·  one slot  ·  max {} frames",
                        model::MAX_FRAMES
                    ))
                    .size(12.0)
                    .color(TEXT_DIM),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(
                        "Vendor protocol: 0xFF13 control (64-byte feature reports) and 0xFF68 \
                         image (4096-byte pages). See docs/PROTOCOL.md in the companion repo.",
                    )
                    .size(12.0)
                    .color(TEXT_DIM),
                );
            });

        ui.add_space(12.0);
        Frame::new()
            .fill(CARD)
            .stroke(Stroke::new(1.0_f32, BORDER))
            .corner_radius(CornerRadius::same(9))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width() - 40.0);
                ui.label(egui::RichText::new("SHORTCUTS").size(10.0).color(TEXT_FAINT));
                ui.add_space(6.0);
                for (keys, action) in [
                    ("1 2 3 4", "switch between Screen, Lighting, Live and Device"),
                    ("U", "upload the current timeline to the keyboard"),
                    ("grid", "tick the grid box to line up 16 px guides while painting"),
                ] {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(keys)
                                .monospace()
                                .size(11.0)
                                .color(ACCENT),
                        );
                        ui.label(egui::RichText::new(action).size(11.0).color(TEXT_DIM));
                    });
                }
            });

        ui.add_space(12.0);
        self.activity_card(ui);
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        ctx.input(|input| {
            if input.modifiers.alt {
                return;
            }
            for (key, view) in [
                (egui::Key::Num1, View::Screen),
                (egui::Key::Num2, View::Lighting),
                (egui::Key::Num3, View::Live),
                (egui::Key::Num4, View::Settings),
                (egui::Key::Num5, View::Device),
            ] {
                if input.key_pressed(key) && !input.modifiers.ctrl {
                    self.view = view;
                }
            }
        });
        if self.view == View::Screen && ctx.input(|i| i.key_pressed(egui::Key::U)) {
            let payload = self.screen.encode();
            let pages = payload.len() / model::PAGE;
            self.progress = Some(("uploading".into(), 0, pages));
            self.worker.send(Command::Upload(payload));
            self.set_status("uploading…", Tone::Info);
        }
    }

    fn pump_events(&mut self) {
        while let Some(event) = self.worker.try_recv() {
            match event {
                Event::Device(info) => {
                    if info.is_some() {
                        self.set_status("device ready", Tone::Good);
                    } else {
                        self.set_status("no device on the cable", Tone::Bad);
                    }
                    self.device = info;
                }
                Event::Stage { label, done, total } => {
                    self.progress = Some((label, done, total));
                }
                Event::Finished { message } => {
                    self.progress = None;
                    self.set_status(message, Tone::Good);
                }
                Event::Failed { message } => {
                    self.progress = None;
                    self.set_status(message, Tone::Bad);
                }
                Event::LivePush { ok, message } => {
                    self.live_preview = self.live_state.render(self.live_provider);
                    self.live_preview_at = Some(Instant::now());
                    self.live_preview = self.live_preview.clone();
                    if ok {
                        self.last_push = Some(Instant::now());
                        self.push_failures = 0;
                        self.note(&format!("live: {message}"), Tone::Good);
                        self.status = format!("live: {message}");
                        self.tone = Tone::Good;
                    } else {
                        self.push_failures = self.push_failures.saturating_add(1);
                        self.note(&message, Tone::Bad);
                        self.status = message;
                        self.tone = Tone::Bad;
                    }
                }
                Event::LiveState { running } => {
                    self.live_running = running;
                }
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump_events();
        self.refresh_textures(ctx);
        self.shortcuts(ctx);

        if self.view == View::Live
            && self.metrics_at.is_none_or(|at| at.elapsed() > Duration::from_secs(1))
        {
            self.metrics = Some(self.live_state.metrics());
            self.metrics_at = Some(Instant::now());
        }
        if self.view == View::Live {
            if self
                .live_preview_at
                .is_none_or(|at| at.elapsed() > Duration::from_millis(900))
            {
                self.live_preview = self.live_state.render(self.live_provider);
                self.live_preview_at = Some(Instant::now());
            }
        }
        if self.live_running || self.view == View::Live {
            ctx.request_repaint_after(Duration::from_millis(300));
        }

        self.top_bar(ctx);
        self.rail(ctx);
        self.status_bar(ctx);
        self.workspace(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.worker.send(Command::Shutdown);
    }
}

impl App {
    fn activity_card(&self, ui: &mut egui::Ui) {
        Frame::new()
            .fill(CARD)
            .stroke(Stroke::new(1.0_f32, BORDER))
            .corner_radius(CornerRadius::same(9))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width() - 40.0);
                ui.label(egui::RichText::new("ACTIVITY").size(10.0).color(TEXT_FAINT));
                ui.add_space(6.0);
                if self.log.is_empty() {
                    ui.label(
                        egui::RichText::new("nothing yet — actions show up here")
                            .size(11.0)
                            .color(TEXT_FAINT),
                    );
                    return;
                }
                for (time, message, tone) in self.log.iter().take(5) {
                    let colour = match tone {
                        Tone::Info => TEXT_DIM,
                        Tone::Good => GOOD,
                        Tone::Bad => BAD,
                    };
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(time)
                                .monospace()
                                .size(10.0)
                                .color(TEXT_FAINT),
                        );
                        ui.label(egui::RichText::new(message).size(11.0).color(colour));
                    });
                }
            });
    }

    fn lighting_preview(&self, ui: &mut egui::Ui) {
        Frame::new()
            .fill(CARD)
            .stroke(Stroke::new(1.0_f32, BORDER))
            .corner_radius(CornerRadius::same(9))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width() - 40.0);
                ui.label(egui::RichText::new("HOW IT LOOKS").size(10.0).color(TEXT_FAINT));
                ui.add_space(8.0);
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
                let painter = ui.painter();
                let steps = 6;
                let width = rect.width() / steps as f32;
                for step in 0..steps {
                    let scale = 0.25 + step as f32 * 0.15;
                    let colour = if self.light_colourful {
                        rainbow(step as f32 / steps as f32)
                    } else {
                        Color32::from_rgb(
                            (self.light_colour[0] as f32 * scale) as u8,
                            (self.light_colour[1] as f32 * scale) as u8,
                            (self.light_colour[2] as f32 * scale) as u8,
                        )
                    };
                    let bar = Rect::from_min_size(
                        Pos2::new(rect.left() + step as f32 * width, rect.top()),
                        Vec2::new(width - 2.0, rect.height()),
                    );
                    painter.rect_filled(bar, CornerRadius::same(4), colour);
                }
                painter.rect_stroke(
                    rect,
                    CornerRadius::same(4),
                    Stroke::new(1.0_f32, BORDER),
                    StrokeKind::Inside,
                );
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("left = dim, right = the brightness you picked")
                        .size(10.0)
                        .color(TEXT_FAINT),
                );
            });
    }
}

fn rainbow(t: f32) -> Color32 {
    let (r, g, b) = hsv_to_rgb(t.fract(), 0.85, 1.0);
    Color32::from_rgb(r, g, b)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    let (r, g, b) = match i as i32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let button = egui::Button::new(
        egui::RichText::new(text)
            .size(13.0)
            .color(Color32::from_rgb(0x14, 0x10, 0x08)),
    )
    .fill(ACCENT)
    .stroke(Stroke::NONE)
    .corner_radius(CornerRadius::same(7))
    .min_size(Vec2::new(0.0, 28.0));
    ui.add(button)
}

fn slider_row(ui: &mut egui::Ui, label: &str, value: &mut u8, max: u8) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(label)
                .size(11.0)
                .color(TEXT_DIM),
        );
    });
    let width = (ui.available_width() - 8.0).max(80.0);
    let previous = ui.spacing().slider_width;
    ui.spacing_mut().slider_width = width - 46.0;
    ui.add(egui::Slider::new(value, 0..=max).trailing_fill(true));
    ui.spacing_mut().slider_width = previous;
}

fn metric_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(egui::RichText::new(label).size(12.0).color(TEXT_DIM));
    ui.label(
        egui::RichText::new(value)
            .monospace()
            .size(12.0)
            .color(TEXT),
    );
    ui.end_row();
}

// ------------------------------------------------------------------- icons ---

#[derive(Clone, Copy)]
enum IconKind {
    Monitor,
    Bulb,
    Pulse,
    Sliders,
    Chip,
}

fn draw_icon(painter: &egui::Painter, center: Pos2, kind: IconKind, colour: Color32) {
    let stroke = Stroke::new(1.6_f32, colour);
    match kind {
        IconKind::Monitor => {
            let rect = Rect::from_center_size(center + Vec2::new(0.0, -1.0), Vec2::new(17.0, 12.0));
            painter.rect_stroke(rect, CornerRadius::same(2), stroke, StrokeKind::Middle);
            painter.line_segment(
                [
                    Pos2::new(center.x - 4.0, center.y + 8.5),
                    Pos2::new(center.x + 4.0, center.y + 8.5),
                ],
                stroke,
            );
        }
        IconKind::Bulb => {
            painter.circle_stroke(center + Vec2::new(0.0, -2.0), 5.2, stroke);
            painter.line_segment(
                [
                    Pos2::new(center.x - 2.5, center.y + 6.0),
                    Pos2::new(center.x + 2.5, center.y + 6.0),
                ],
                stroke,
            );
            for angle in [0.0f32, 0.8, -0.8] {
                let dir = Vec2::new(angle.sin(), -angle.cos());
                painter.line_segment(
                    [
                        center + Vec2::new(0.0, -2.0) + dir * 8.0,
                        center + Vec2::new(0.0, -2.0) + dir * 10.0,
                    ],
                    stroke,
                );
            }
        }
        IconKind::Pulse => {
            let points = [
                Pos2::new(center.x - 9.0, center.y),
                Pos2::new(center.x - 4.0, center.y),
                Pos2::new(center.x - 1.5, center.y - 5.5),
                Pos2::new(center.x + 1.5, center.y + 5.5),
                Pos2::new(center.x + 4.5, center.y),
                Pos2::new(center.x + 9.0, center.y),
            ];
            for pair in points.windows(2) {
                painter.line_segment([pair[0], pair[1]], stroke);
            }
        }
        IconKind::Sliders => {
            for (index, offset) in [-5.0f32, 0.0, 5.0].iter().enumerate() {
                let y = center.y + offset;
                painter.line_segment(
                    [Pos2::new(center.x - 8.0, y), Pos2::new(center.x + 8.0, y)],
                    stroke,
                );
                let knob = match index {
                    0 => center.x + 3.0,
                    1 => center.x - 4.0,
                    _ => center.x + 1.0,
                };
                painter.circle_filled(Pos2::new(knob, y), 2.0, colour);
            }
        }
        IconKind::Chip => {
            let rect = Rect::from_center_size(center, Vec2::new(14.0, 14.0));
            painter.rect_stroke(rect, CornerRadius::same(2), stroke, StrokeKind::Middle);
            for i in 0..3 {
                let offset = -4.0 + i as f32 * 4.0;
                painter.line_segment(
                    [
                        Pos2::new(center.x + offset, rect.top() - 3.0),
                        Pos2::new(center.x + offset, rect.top()),
                    ],
                    stroke,
                );
                painter.line_segment(
                    [
                        Pos2::new(center.x + offset, rect.bottom()),
                        Pos2::new(center.x + offset, rect.bottom() + 3.0),
                    ],
                    stroke,
                );
            }
        }
    }
}

fn accent_mark(painter: &egui::Painter, rect: Rect) {
    painter.rect_filled(rect, CornerRadius::same(6), ACCENT);
    let inner = rect.shrink(6.0);
    painter.rect_filled(inner, CornerRadius::same(3), Color32::BLACK);
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let initial_view = args
        .iter()
        .position(|arg| arg == "--view")
        .and_then(|index| args.get(index + 1))
        .map(|name| match name.as_str() {
            "lighting" => View::Lighting,
            "live" => View::Live,
            "settings" => View::Settings,
            "device" => View::Device,
            _ => View::Screen,
        })
        .unwrap_or(View::Screen);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([940.0, 620.0])
            .with_title("HX450 Studio"),
        ..Default::default()
    };
    eframe::run_native(
        "HX450 Studio",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, initial_view)))),
    )
}
