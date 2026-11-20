// SPDX-License-Identifier: Apache-2.0
//! Native map canvas and editing controls.

use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

use crate::{
    appearance::{self, BUTTON_TEXT, INDIGO, LOW_IMPACT, PRIMARY, ZONE, display, muted},
    place::{self, Place},
    tile_map::{self, Basemap},
};
use eframe::egui::{
    self, Align, Align2, Color32, Context, FontId, Frame, Layout, Margin, Pos2, Rect, RichText,
    Sense, Stroke, StrokeKind, Theme, Vec2, pos2, vec2,
};
use silent_planner::sensitivity::{self, Sensitivity};
use silent_planner::{
    export::{export, export_satellite, open_project},
    map::{Analysis, Bounds, MAX_ZONES, Point, Project, Scene, Zone, analyze, cell_color},
    satellite::Origin,
};

type ExportResult = Result<(PathBuf, Option<String>), String>;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Inspect,
    Add,
    Move,
    Draw,
}

struct View {
    bounds: Bounds,
    plot: Rect,
    pixels_per_m: f32,
}

impl View {
    fn new(canvas: Rect, bounds: Bounds) -> Self {
        let space = canvas.shrink2(vec2(42.0, 34.0));
        let pixels_per_m = (space.width() / bounds.width() as f32)
            .min(space.height() / bounds.height() as f32)
            .max(0.01);
        let plot = Rect::from_center_size(
            space.center(),
            vec2(
                bounds.width() as f32 * pixels_per_m,
                bounds.height() as f32 * pixels_per_m,
            ),
        );
        Self {
            bounds,
            plot,
            pixels_per_m,
        }
    }

    fn projected(plot: Rect, projector: &walkers::Projector, origin: walkers::Position) -> Self {
        let reference = projector.project(origin);
        let pixels_per_m = projector.scale_pixel_per_meter(origin);
        let local = |position: Pos2| {
            Point::new(
                f64::from((position.x - reference.x) / pixels_per_m),
                f64::from((reference.y - position.y) / pixels_per_m),
            )
        };
        Self {
            bounds: Bounds::spanning(local(plot.left_bottom()), local(plot.right_top())),
            plot,
            pixels_per_m,
        }
    }

    fn position(&self, point: Point) -> Pos2 {
        pos2(
            self.plot.left() + (point.x - self.bounds.min.x) as f32 * self.pixels_per_m,
            self.plot.top() + (self.bounds.max.y - point.y) as f32 * self.pixels_per_m,
        )
    }

    fn point(&self, point: Pos2) -> Point {
        Point::new(
            self.bounds.min.x + f64::from((point.x - self.plot.left()) / self.pixels_per_m),
            self.bounds.max.y - f64::from((point.y - self.plot.top()) / self.pixels_per_m),
        )
    }

    fn rectangle(&self, bounds: Bounds) -> Rect {
        Rect::from_two_pos(self.position(bounds.min), self.position(bounds.max))
    }
}

pub struct Planner {
    project: Project,
    analysis: Result<Analysis, String>,
    mode: Mode,
    zone: Option<usize>,
    drag_start: Option<Point>,
    show_heat: bool,
    show_stability: bool,
    stability: Option<Sensitivity>,
    basemap: Basemap,
    vworld_key: Option<String>,
    tiles: walkers::HttpTiles,
    visible_tiles: usize,
    tile_wait: Instant,
    map_memory: walkers::MapMemory,
    fit_geographic: bool,
    query: String,
    search_task: Option<Receiver<Result<Vec<Place>, String>>>,
    search_results: Vec<Place>,
    pending_place: Option<Place>,
    dirty: bool,
    camera_offset: Point,
    zoom: f64,
    export_task: Option<Receiver<ExportResult>>,
    export_snapshot: Option<Project>,
    export_path: PathBuf,
    message: Option<(String, bool)>,
}

impl Planner {
    pub fn new(ctx: &Context) -> Self {
        appearance::install(ctx);
        let project = Project::demo();
        let analysis = analyze(&project);
        let vworld_key = tile_map::vworld_key();
        let basemap = if vworld_key.is_some() {
            Basemap::KoreaAerial
        } else {
            Basemap::Streets
        };
        Self {
            project,
            analysis,
            mode: Mode::Inspect,
            zone: None,
            drag_start: None,
            show_heat: true,
            show_stability: false,
            stability: None,
            basemap,
            tiles: tile_map::tiles(basemap, vworld_key.as_deref(), ctx),
            visible_tiles: 0,
            tile_wait: Instant::now(),
            vworld_key,
            map_memory: walkers::MapMemory::default(),
            fit_geographic: true,
            query: String::new(),
            search_task: None,
            search_results: Vec::new(),
            pending_place: None,
            dirty: false,
            camera_offset: Point::default(),
            zoom: 1.0,
            export_task: None,
            export_snapshot: None,
            export_path: PathBuf::from("planner-exports"),
            message: None,
        }
    }

    fn refresh(&mut self) {
        self.analysis = analyze(&self.project);
        self.stability = None;
        self.dirty = true;
    }

    fn set_project(&mut self, project: Project) {
        self.project = project;
        self.fit_map();
        self.zone = None;
        self.drag_start = None;
        self.mode = Mode::Inspect;
        self.refresh();
        self.dirty = false;
        self.search_results.clear();
        self.pending_place = None;
        self.search_task = None;
    }

    fn map_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if self.project.origin.is_some() {
                let mut selected = self.basemap;
                ui.selectable_value(&mut selected, Basemap::Streets, "Streets");
                ui.selectable_value(&mut selected, Basemap::Satellite, "Satellite");
                if self.vworld_key.is_some() {
                    ui.selectable_value(&mut selected, Basemap::KoreaAerial, "Korea HD");
                }
                if selected != self.basemap || ui.small_button("Reload").clicked() {
                    self.set_basemap(selected, ui.ctx());
                }
                ui.separator();
                if ui.small_button("−").on_hover_text("Zoom out").clicked() {
                    let _ = self.map_memory.zoom_out();
                }
                if ui.small_button("+").on_hover_text("Zoom in").clicked() {
                    let _ = self.map_memory.zoom_in();
                }
            }
            if ui.small_button("Fit area").clicked() {
                self.fit_map();
            }
            ui.checkbox(&mut self.show_heat, "Noise overlay");
            ui.checkbox(&mut self.show_stability, "Stability");
            if self.mode != Mode::Inspect && ui.button("Cancel").clicked() {
                self.mode = Mode::Inspect;
                self.drag_start = None;
            }
        });
    }

    fn fit_map(&mut self) {
        self.camera_offset = Point::default();
        self.zoom = 1.0;
        self.fit_geographic = true;
    }

    fn set_basemap(&mut self, kind: Basemap, ctx: &Context) {
        self.basemap = kind;
        self.tiles = tile_map::tiles(kind, self.vworld_key.as_deref(), ctx);
        self.tile_wait = Instant::now();
    }

    fn find_place(&mut self, ctx: &Context) {
        let query = self.query.trim().to_owned();
        if query.is_empty() || self.search_task.is_some() {
            return;
        }
        self.search_results.clear();
        self.pending_place = None;
        if let Some(origin) = place::coordinates(&query) {
            self.search_results.push(Place {
                label: query,
                origin,
            });
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let wake = ctx.clone();
        thread::spawn(move || {
            let _ = sender.send(place::search(&query));
            wake.request_repaint();
        });
        self.search_task = Some(receiver);
    }

    fn start_site(&mut self, site: Place) {
        self.set_project(Project {
            name: site.label.chars().take(100).collect(),
            origin: Some(site.origin),
            ..Project::default()
        });
        self.mode = Mode::Add;
        self.message = None;
    }

    fn choose_site(&mut self, site: Place) {
        if self.dirty {
            self.pending_place = Some(site);
        } else {
            self.start_site(site);
        }
    }

    fn site_controls(&mut self, ui: &mut egui::Ui) {
        section(ui, "Find your site");
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 64.0).max(80.0);
            let response = ui.add_sized(
                [width, 34.0],
                egui::TextEdit::singleline(&mut self.query)
                    .char_limit(150)
                    .margin(Margin::symmetric(10, 7))
                    .hint_text(RichText::new("Place or coordinates").color(muted(ui.visuals()))),
            );
            let shortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::K);
            if ui.input_mut(|input| input.consume_shortcut(&shortcut)) {
                response.request_focus();
            }
            if response.has_focus() {
                ui.painter().rect_stroke(
                    response.rect.expand(1.5),
                    7.0,
                    Stroke::new(3.0_f32, INDIGO.gamma_multiply(0.20)),
                    StrokeKind::Inside,
                );
            }
            let submitted = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui
                .add_enabled(
                    self.search_task.is_none(),
                    egui::Button::new("Find").min_size(vec2(0.0, 34.0)),
                )
                .on_hover_text("Search for a place (Ctrl/⌘+K to focus)")
                .clicked()
                || submitted
            {
                self.find_place(ui.ctx());
            }
        });
        if self.search_task.is_some() {
            ui.label(
                RichText::new("Finding places…")
                    .small()
                    .color(muted(ui.visuals())),
            );
        }
        let mut chosen = None;
        for site in &self.search_results {
            if ui
                .add(
                    egui::Button::new(&site.label)
                        .wrap()
                        .min_size(vec2(ui.available_width(), 32.0)),
                )
                .clicked()
            {
                chosen = Some(site.clone());
            }
        }
        if let Some(site) = chosen {
            self.choose_site(site);
        }
        if let Some(site) = self.pending_place.clone() {
            ui.label("Save the current map before starting a new site.");
            ui.horizontal(|ui| {
                if ui.button("Start new site").clicked() {
                    self.start_site(site);
                }
                if ui.button("Keep current").clicked() {
                    self.pending_place = None;
                }
            });
        }
    }

    fn begin_export(&mut self, ctx: &Context) {
        let (sender, receiver) = mpsc::channel();
        let project = self.project.clone();
        self.export_snapshot = Some(project.clone());
        let directory = self.export_path.clone();
        let wake = ctx.clone();
        let imagery = self.basemap == Basemap::Satellite;
        thread::spawn(move || {
            let result = if imagery {
                export_satellite(&project, &directory)
            } else {
                export(&project, &directory).map(|directory| (directory, None))
            };
            let _ = sender.send(result);
            wake.request_repaint();
        });
        self.export_task = Some(receiver);
        self.message = Some(("Preparing map export…".into(), true));
    }

    fn header(&mut self, ctx: &Context) {
        let visuals = ctx.style().visuals.clone();
        egui::TopBottomPanel::top("header")
            .frame(
                Frame::new()
                    .fill(visuals.window_fill)
                    .inner_margin(Margin::symmetric(24, 8))
                    .stroke(visuals.widgets.noninteractive.bg_stroke),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(display("Silent Planner", 23.0));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let export = ui.scope(|ui| {
                            let widgets = &mut ui.visuals_mut().widgets;
                            widgets.inactive.weak_bg_fill = PRIMARY;
                            widgets.hovered.weak_bg_fill = Color32::from_rgb(67, 56, 202);
                            widgets.active.weak_bg_fill = Color32::from_rgb(55, 48, 163);
                            widgets.inactive.bg_stroke = Stroke::NONE;
                            widgets.hovered.bg_stroke = Stroke::NONE;
                            ui.add_enabled(
                                self.export_task.is_none(),
                                egui::Button::new(RichText::new("Export map").color(BUTTON_TEXT))
                                    .min_size(vec2(112.0, 38.0)),
                            )
                        });
                        if export.inner.clicked() {
                            self.begin_export(ctx);
                        }
                        ui.menu_button("Project", |ui| {
                            let (label, theme) = if ui.visuals().dark_mode {
                                ("Light appearance", Theme::Light)
                            } else {
                                ("Dark appearance", Theme::Dark)
                            };
                            if ui.button(label).clicked() {
                                ctx.set_theme(theme);
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("Open project…").clicked() {
                                ui.close_menu();
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("Project", &["json"])
                                    .pick_file()
                                {
                                    match open_project(&path) {
                                        Ok(project) => {
                                            self.set_project(project);
                                            self.message = Some(("Project opened.".into(), true));
                                        }
                                        Err(error) => self.message = Some((error, false)),
                                    }
                                }
                            }
                            if ui.button("Choose export folder…").clicked() {
                                ui.close_menu();
                                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                                    self.export_path = folder;
                                    self.message = Some((
                                        format!("Export folder: {}", self.export_path.display()),
                                        true,
                                    ));
                                }
                            }
                        });
                    });
                });
            });
    }

    fn controls(&mut self, ctx: &Context) {
        egui::SidePanel::left("workflow")
            .resizable(false)
            .exact_width(312.0)
            .frame(
                Frame::new()
                    .fill(ctx.style().visuals.window_fill)
                    .inner_margin(Margin::same(24)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.site_controls(ui);
                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(8.0);
                    self.zone_controls(ui);
                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(8.0);
                    section(ui, "Analysis boundary");
                    let automatic = self.project.bounds.is_none();
                    ui.horizontal(|ui| {
                        if ui.selectable_label(automatic, "Auto area").clicked() {
                            self.project.bounds = None;
                            self.mode = Mode::Inspect;
                            self.refresh();
                            self.fit_map();
                        }
                        if ui
                            .selectable_label(self.mode == Mode::Draw, "Draw area")
                            .clicked()
                        {
                            self.mode = Mode::Draw;
                        }
                    });
                    if automatic {
                        ui.label(
                            RichText::new("Fits around the marked zones.")
                                .small()
                                .color(muted(ui.visuals())),
                        );
                    } else {
                        ui.label(
                            RichText::new("Uses the rectangle you drew.")
                                .small()
                                .color(muted(ui.visuals())),
                        );
                    }
                    ui.add_space(10.0);
                    egui::CollapsingHeader::new("Advanced settings")
                        .show(ui, |ui| self.advanced_controls(ui));
                })
            });
    }

    fn advanced_controls(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        field_label(ui, "Project name");
        changed |= ui
            .add(
                egui::TextEdit::singleline(&mut self.project.name)
                    .char_limit(100)
                    .desired_width(f32::INFINITY),
            )
            .changed();
        let before = self.project.scene;
        field_label(ui, "Outdoor scene");
        egui::ComboBox::from_id_salt("scene")
            .width(ui.available_width())
            .selected_text(before.label())
            .show_ui(ui, |ui| {
                for scene in [Scene::Quiet, Scene::Roadside] {
                    ui.selectable_value(&mut self.project.scene, scene, scene.label());
                }
            });
        changed |= before != self.project.scene;
        field_label(ui, "Nominal source height");
        changed |= ui
            .add(
                egui::Slider::new(&mut self.project.source_height_m, 5.0..=30.0)
                    .step_by(1.0)
                    .suffix(" m"),
            )
            .changed();
        field_label(ui, "Target grid spacing");
        changed |= ui
            .add(
                egui::Slider::new(&mut self.project.cell_size_m, 0.5..=15.0)
                    .step_by(0.5)
                    .suffix(" m"),
            )
            .changed();
        field_label(ui, "Lowest-scoring cells to mark");
        changed |= ui
            .add(
                egui::Slider::new(&mut self.project.selected_fraction, 0.01..=0.50)
                    .step_by(0.01)
                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
                    .custom_parser(|text| {
                        text.trim_end_matches("%")
                            .parse::<f64>()
                            .ok()
                            .map(|v| v / 100.0)
                    }),
            )
            .changed();
        if changed {
            self.refresh();
        }
    }

    fn zone_controls(&mut self, ui: &mut egui::Ui) {
        section(ui, "Occupied zones");
        let button = egui::Button::new("+ Add zone")
            .selected(self.mode == Mode::Add)
            .min_size(vec2(ui.available_width(), 33.0));
        if ui
            .add_enabled(self.project.zones.len() < MAX_ZONES, button)
            .clicked()
        {
            self.mode = if self.mode == Mode::Add {
                Mode::Inspect
            } else {
                Mode::Add
            };
        }
        if self.project.zones.is_empty() {
            ui.label(
                RichText::new("Click the map to mark the first area.")
                    .small()
                    .color(muted(ui.visuals())),
            );
        }
        for (index, zone) in self.project.zones.iter().enumerate() {
            let label = format!("{}  ·  {:.0} m", zone.label, zone.radius_m);
            if ui
                .add(
                    egui::Button::new(label)
                        .selected(self.zone == Some(index))
                        .min_size(vec2(ui.available_width(), 29.0)),
                )
                .clicked()
            {
                self.zone = Some(index);
                self.mode = Mode::Inspect;
            }
        }
        if let Some(index) = self.zone {
            let zone = &mut self.project.zones[index];
            field_label(ui, "Zone name");
            let mut changed = ui
                .add(
                    egui::TextEdit::singleline(&mut zone.label)
                        .char_limit(60)
                        .desired_width(f32::INFINITY),
                )
                .changed();
            field_label(ui, "Radius");
            changed |= ui
                .add(
                    egui::Slider::new(&mut zone.radius_m, 0.0..=30.0)
                        .step_by(0.5)
                        .suffix(" m"),
                )
                .changed();
            field_label(ui, "Sensitivity");
            changed |= ui
                .add(egui::Slider::new(&mut zone.weight, 0.1..=1.0).step_by(0.05))
                .changed();
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(self.mode == Mode::Move, "Move")
                    .clicked()
                {
                    self.mode = Mode::Move;
                }
                if ui
                    .button(RichText::new("Remove").color(ui.visuals().error_fg_color))
                    .clicked()
                {
                    self.project.zones.remove(index);
                    self.zone = None;
                    self.mode = Mode::Inspect;
                    changed = true;
                }
            });
            if changed {
                self.refresh();
            }
        }
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        if self.show_stability
            && self.stability.is_none()
            && let Ok(analysis) = &self.analysis
        {
            self.stability = Some(sensitivity::assess(&self.project, analysis));
        }
        let has_basemap = self.project.origin.is_some();
        let (response, painter, view) = if let Some(origin) = self.project.origin {
            self.geographic_canvas(ui, origin)
        } else {
            self.local_canvas(ui)
        };
        let background = ui.visuals().window_fill;
        let border = ui.visuals().widgets.noninteractive.bg_stroke;
        let secondary = muted(ui.visuals());
        let map_painter = painter.with_clip_rect(view.plot);
        let pointer = response
            .interact_pointer_pos()
            .or_else(|| response.hover_pos());
        if let Ok(analysis) = &self.analysis {
            if self.show_heat {
                let size = vec2(analysis.cell_width_m as f32, analysis.cell_height_m as f32)
                    * view.pixels_per_m;
                for (index, cell) in analysis.cells.iter().enumerate() {
                    let paint = if self.show_stability {
                        self.stability
                            .as_ref()
                            .and_then(|check| sensitivity::color(check.cells[index].class))
                    } else {
                        cell.score.map(|_| cell_color(cell, analysis))
                    };
                    let Some([r, g, b]) = paint else { continue };
                    let tile = Rect::from_center_size(view.position(cell.center), size);
                    let alpha = if !has_basemap {
                        255
                    } else if cell.selected {
                        190
                    } else {
                        88
                    };
                    map_painter.rect_filled(
                        tile,
                        0.0,
                        Color32::from_rgba_unmultiplied(r, g, b, alpha),
                    );
                }
            }
            map_painter.rect_stroke(
                view.rectangle(analysis.bounds),
                0.0,
                Stroke::new(1.5_f32, secondary),
                StrokeKind::Middle,
            );
        }
        if !has_basemap || self.visible_tiles == 0 {
            draw_grid(&map_painter, &view, border.color);
        }
        for (index, zone) in self.project.zones.iter().enumerate() {
            let center = view.position(zone.center);
            let radius = (zone.radius_m as f32 * view.pixels_per_m).max(4.0);
            let selected = self.zone == Some(index);
            let color = if selected { INDIGO } else { ZONE };
            map_painter.circle(
                center,
                radius,
                Color32::from_rgba_unmultiplied(113, 83, 153, 58),
                Stroke::new(if selected { 2.7_f32 } else { 1.8_f32 }, color),
            );
            map_painter.circle_filled(center, 3.0, color);
            let text = map_painter.layout_no_wrap(
                zone.label.clone(),
                FontId::proportional(13.0),
                ui.visuals().text_color(),
            );
            let label = pos2(
                center.x - text.size().x / 2.0 - 6.0,
                center.y - radius - 9.0 - text.size().y,
            );
            let pill = Rect::from_min_size(label, text.size() + vec2(12.0, 6.0));
            map_painter.rect(pill, 4.0, background, border, StrokeKind::Inside);
            map_painter.galley(label + vec2(6.0, 3.0), text, ui.visuals().text_color());
        }
        draw_compass(&painter, response.rect, &view, background, secondary);
        if has_basemap && self.visible_tiles == 0 {
            let loading = self.tile_wait.elapsed() < Duration::from_secs(5);
            if loading {
                ui.ctx().request_repaint_after(Duration::from_millis(400));
            }
            painter.text(
                response.rect.left_top() + vec2(13.0, 14.0),
                Align2::LEFT_TOP,
                if loading {
                    "Loading map…"
                } else {
                    "Map unavailable. Switch layers or reload."
                },
                FontId::proportional(11.0),
                secondary,
            );
        }
        if self.mode == Mode::Draw {
            if response.drag_started() {
                self.drag_start = ui
                    .input(|input| input.pointer.press_origin())
                    .filter(|point| view.plot.contains(*point))
                    .map(|point| view.point(point));
            }
            if let Some((start, end)) = self
                .drag_start
                .zip(pointer.map(|p| view.point(p.clamp(view.plot.min, view.plot.max))))
            {
                let bounds = Bounds::spanning(start, end);
                let rect = view.rectangle(bounds);
                painter.rect_filled(rect, 0.0, INDIGO.gamma_multiply(0.16));
                painter.rect_stroke(rect, 0.0, Stroke::new(2.0_f32, INDIGO), StrokeKind::Inside);
                if response.drag_stopped() {
                    if bounds.width() >= self.project.cell_size_m
                        && bounds.height() >= self.project.cell_size_m
                    {
                        let previous = self.project.bounds.replace(bounds);
                        if let Err(error) = self.project.validate() {
                            self.project.bounds = previous;
                            self.message = Some((error, false));
                        } else {
                            self.mode = Mode::Inspect;
                            self.refresh();
                        }
                    } else {
                        self.message = Some((
                            "Draw a boundary at least one cell wide and high.".into(),
                            false,
                        ));
                    }
                    self.drag_start = None;
                }
            }
            if response.drag_stopped() {
                self.drag_start = None;
            }
        } else if response.clicked()
            && let Some(point) = pointer
                .filter(|p| view.plot.contains(*p))
                .map(|p| view.point(p))
        {
            if self.mode != Mode::Inspect && (point.x.abs() > 10_000.0 || point.y.abs() > 10_000.0)
            {
                self.message = Some((
                    "Set a closer project origin before marking this area.".into(),
                    false,
                ));
                return;
            }
            match self.mode {
                Mode::Inspect => {
                    self.zone = self
                        .project
                        .zones
                        .iter()
                        .enumerate()
                        .filter_map(|(index, zone)| {
                            let distance = zone.center.distance(point);
                            (distance <= zone.radius_m.max(8.0 / f64::from(view.pixels_per_m)))
                                .then_some((index, distance))
                        })
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(index, _)| index);
                }
                Mode::Add if self.project.zones.len() < MAX_ZONES => {
                    self.zone = Some(self.project.zones.len());
                    self.project.zones.push(Zone {
                        label: format!("Zone {}", self.project.zones.len() + 1),
                        center: point,
                        radius_m: 4.0,
                        weight: 1.0,
                    });
                    self.mode = Mode::Inspect;
                    self.refresh();
                }
                Mode::Move => {
                    if let Some(index) = self.zone {
                        self.project.zones[index].center = point;
                        self.refresh();
                    }
                    self.mode = Mode::Inspect;
                }
                _ => {}
            }
        }
        if self.mode == Mode::Inspect
            && let Some(point) = response.hover_pos().filter(|p| view.plot.contains(*p))
        {
            let position = view.point(point);
            let mut label = format!("X {:.1} m   Y {:.1} m", position.x, position.y);
            if let Some(origin) = self.project.origin {
                let [lon, lat] = origin.lon_lat(position);
                label.push_str(&format!("   •   {lat:.5}, {lon:.5}"));
            }
            if let Ok(result) = &self.analysis
                && let Some(cell) = result.cell_at(position)
            {
                label.push_str(&match cell.score {
                    Some(value) => format!("   •   Impact {value:.3}"),
                    None => "   •   Outside model support".into(),
                });
            }
            if self.show_stability
                && let (Ok(result), Some(stability)) = (&self.analysis, &self.stability)
                && let Some(index) = result.cell_index(position)
            {
                let cell = &stability.cells[index];
                label.push_str(&format!(
                    "   •   Selected {}/{} · scored {}",
                    cell.selected,
                    stability.completed.len(),
                    cell.supported
                ));
            }
            let text_color = ui.visuals().text_color();
            let text = painter.layout_no_wrap(label, FontId::monospace(11.0), text_color);
            let position = response.rect.left_bottom() + vec2(12.0, -13.0 - text.size().y);
            let pill = Rect::from_min_size(position, text.size() + vec2(12.0, 6.0));
            painter.rect(pill, 4.0, background, border, StrokeKind::Inside);
            painter.galley(position + vec2(6.0, 3.0), text, text_color);
        }
    }

    fn local_canvas(&mut self, ui: &mut egui::Ui) -> (egui::Response, egui::Painter, View) {
        let (response, painter) = ui.allocate_painter(
            vec2(ui.available_width(), ui.available_height().max(245.0)),
            Sense::click_and_drag(),
        );
        let base = self.project.view_bounds().expanded(8.0);
        let mut view = View::new(response.rect, self.camera_bounds(base));
        if self.mode == Mode::Inspect {
            let drag = response.drag_delta();
            self.camera_offset.x = (self.camera_offset.x - f64::from(drag.x / view.pixels_per_m))
                .clamp(-6000.0, 6000.0);
            self.camera_offset.y = (self.camera_offset.y + f64::from(drag.y / view.pixels_per_m))
                .clamp(-6000.0, 6000.0);
            if response.hovered() {
                let scroll = ui.input(|input| input.smooth_scroll_delta.y);
                self.zoom = (self.zoom * (f64::from(scroll) / 320.0).exp()).clamp(0.4, 12.0);
            }
            view = View::new(response.rect, self.camera_bounds(base));
        }
        painter.rect_filled(response.rect, 6.0, ui.visuals().panel_fill);
        painter.rect_filled(view.plot, 0.0, ui.visuals().window_fill);
        (response, painter, view)
    }

    fn geographic_canvas(
        &mut self,
        ui: &mut egui::Ui,
        origin: Origin,
    ) -> (egui::Response, egui::Painter, View) {
        let position = walkers::lat_lon(origin.latitude, origin.longitude);
        if self.fit_geographic {
            let range = self.project.view_bounds().expanded(55.0);
            let scale = (f64::from(ui.available_width()) / range.width())
                .min(f64::from(ui.available_height()) / range.height());
            let zoom = (scale * 40_075_016.686 * origin.latitude.to_radians().cos() / 256.0)
                .log2()
                .clamp(2.0, 20.0);
            let [lon, lat] = origin.lon_lat(Point::new(
                (range.min.x + range.max.x) / 2.0,
                (range.min.y + range.max.y) / 2.0,
            ));
            self.map_memory.center_at(walkers::lat_lon(lat, lon));
            let _ = self.map_memory.set_zoom(zoom);
            self.fit_geographic = false;
        }
        ui.painter().rect_filled(
            ui.available_rect_before_wrap(),
            0.0,
            ui.visuals().faint_bg_color,
        );
        let inspect = self.mode == Mode::Inspect;
        let mut tiles = tile_map::VisibleTiles::new(&mut self.tiles);
        let response = ui.add(
            walkers::Map::new(Some(&mut tiles), &mut self.map_memory, position)
                .drag_gesture(inspect)
                .zoom_gesture(inspect)
                .double_click_to_zoom(inspect)
                .panning(inspect),
        );
        self.visible_tiles = tiles.ready;
        let painter = ui.painter_at(response.rect);
        let projector = walkers::Projector::new(response.rect, &self.map_memory, position);
        let view = View::projected(response.rect, &projector, position);
        (response, painter, view)
    }

    fn camera_bounds(&self, base: Bounds) -> Bounds {
        let center = Point::new(
            (base.min.x + base.max.x) / 2.0 + self.camera_offset.x,
            (base.min.y + base.max.y) / 2.0 + self.camera_offset.y,
        );
        let half_width = base.width() / (2.0 * self.zoom);
        let half_height = base.height() / (2.0 * self.zoom);
        Bounds::spanning(
            Point::new(center.x - half_width, center.y - half_height),
            Point::new(center.x + half_width, center.y + half_height),
        )
    }
}

impl eframe::App for Planner {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        if let Some(task) = &self.search_task {
            match task.try_recv() {
                Ok(result) => {
                    self.search_task = None;
                    match result {
                        Ok(places) => self.search_results = places,
                        Err(error) => self.message = Some((error, false)),
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.search_task = None;
                    self.message = Some(("The place search stopped. Try again.".into(), false));
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(task) = &self.export_task {
            match task.try_recv() {
                Ok(report) => {
                    self.export_task = None;
                    if report.is_ok() && self.export_snapshot.as_ref() == Some(&self.project) {
                        self.dirty = false;
                    }
                    self.export_snapshot = None;
                    self.message = Some(match report {
                        Ok((path, Some(_))) => (
                            format!("Exported with grid fallback to {}", path.display()),
                            false,
                        ),
                        Ok((path, None)) => (format!("Exported to {}", path.display()), true),
                        Err(error) => (error, false),
                    });
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.export_task = None;
                    self.message = Some(("Export worker stopped.".into(), false));
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.mode = Mode::Inspect;
            self.drag_start = None;
        }
        self.header(ctx);
        let visuals = ctx.style().visuals.clone();
        egui::TopBottomPanel::bottom("status")
            .frame(
                Frame::new()
                    .fill(visuals.window_fill)
                    .inner_margin(Margin::symmetric(24, 8))
                    .stroke(visuals.widgets.noninteractive.bg_stroke),
            )
            .show(ctx, |ui| {
                if let Some((message, success)) = &self.message {
                    let color = if *success {
                        ui.visuals().text_color()
                    } else {
                        ui.visuals().error_fg_color
                    };
                    let mut dismiss = false;
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(message).small().color(color));
                        dismiss = ui.small_button("Dismiss").clicked();
                    });
                    if dismiss {
                        self.message = None;
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new("Relative acoustic estimates · dBFS model")
                            .small()
                            .color(muted(ui.visuals())),
                    );
                    if self.project.origin.is_some() {
                        let (credit, link) = tile_map::attribution(self.basemap);
                        ui.separator();
                        ui.hyperlink_to(RichText::new(credit).small(), link);
                    }
                });
            });

        self.controls(ctx);
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(visuals.panel_fill)
                    .inner_margin(Margin::same(24)),
            )
            .show(ctx, |ui| self.workspace(ui));
    }
}

impl Planner {
    fn workspace(&mut self, ui: &mut egui::Ui) {
        let visuals = ui.visuals();
        let detail_color = muted(visuals);
        let frame = Frame::new()
            .fill(visuals.window_fill)
            .stroke(visuals.widgets.noninteractive.bg_stroke)
            .corner_radius(12)
            .inner_margin(Margin::same(12));
        ui.label(display(&self.project.name, 40.0));
        let instruction = match self.mode {
            Mode::Inspect => self.map_summary(),
            Mode::Add => "Click the map to add an occupied zone.".into(),
            Mode::Move => "Click the new center of the selected zone.".into(),
            Mode::Draw => "Drag across the map to set the analysis boundary.".into(),
        };
        ui.label(RichText::new(instruction).color(detail_color).size(13.0));
        ui.add_space(8.0);

        frame.show(ui, |ui| {
            self.map_toolbar(ui);
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if self.show_heat && self.show_stability {
                    legend(ui, LOW_IMPACT, "Consistently low");
                    legend(ui, Color32::from_rgb(244, 169, 74), "Uncertain");
                    legend(
                        ui,
                        Color32::from_rgb(81, 135, 169),
                        "Consistently unselected",
                    );
                } else if self.show_heat {
                    legend(ui, LOW_IMPACT, "Selected low impact");
                    legend(ui, Color32::from_rgb(221, 112, 89), "Higher impact");
                }
                legend(ui, ZONE, "Occupied zone");
            });
            self.canvas(ui);
        });
    }

    fn map_summary(&self) -> String {
        match &self.analysis {
            Err(error) => error.clone(),
            Ok(_) if self.project.zones.is_empty() => "Add a zone to start the analysis.".into(),
            Ok(analysis) if analysis.scored_count == 0 => {
                "No cells meet the distance limits for every occupied zone.".into()
            }
            Ok(analysis) => format!(
                "{} scored cells  ·  {} selected  ·  {:.0} × {:.0} m boundary",
                analysis.scored_count,
                analysis.selected_count,
                analysis.bounds.width(),
                analysis.bounds.height()
            ),
        }
    }
}

fn section(ui: &mut egui::Ui, text: &str) {
    ui.label(display(text, 19.0));
}

fn field_label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).color(muted(ui.visuals())).size(12.0));
}

fn legend(ui: &mut egui::Ui, color: Color32, label: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
    ui.painter().rect_filled(rect, 2.0, color);
    ui.label(RichText::new(label).small().color(muted(ui.visuals())));
    ui.add_space(6.0);
}

fn draw_grid(painter: &egui::Painter, view: &View, line_color: Color32) {
    let stroke = Stroke::new(1.0_f32, line_color);
    let span = view.bounds.width().max(view.bounds.height());
    let spacing = 10_f64.powf((span / 18.0).log10().ceil().max(1.0));
    let x1 = (view.bounds.min.x / spacing).ceil() as i32;
    let x2 = (view.bounds.max.x / spacing).floor() as i32;
    for x in x1..=x2 {
        let value = f64::from(x) * spacing;
        let sx = view.position(Point::new(value, 0.0)).x;
        painter.line_segment(
            [pos2(sx, view.plot.top()), pos2(sx, view.plot.bottom())],
            stroke,
        );
    }
    let y1 = (view.bounds.min.y / spacing).ceil() as i32;
    let y2 = (view.bounds.max.y / spacing).floor() as i32;
    for y in y1..=y2 {
        let value = f64::from(y) * spacing;
        let sy = view.position(Point::new(0.0, value)).y;
        painter.line_segment(
            [pos2(view.plot.left(), sy), pos2(view.plot.right(), sy)],
            stroke,
        );
    }
}

fn draw_compass(
    painter: &egui::Painter,
    canvas: Rect,
    view: &View,
    background: Color32,
    foreground: Color32,
) {
    let n = canvas.right_top() + vec2(-22.0, 23.0);
    painter.rect_filled(
        Rect::from_center_size(n + vec2(0.0, 10.0), vec2(26.0, 46.0)),
        4.0,
        background,
    );
    painter.text(
        n,
        Align2::CENTER_CENTER,
        "N",
        FontId::monospace(12.0),
        foreground,
    );
    painter.arrow(
        n + vec2(0.0, 26.0),
        vec2(0.0, -17.0),
        Stroke::new(1.5_f32, foreground),
    );
    let metres = 10_f32.powf((85.0 / view.pixels_per_m).log10().floor());
    let p = canvas.right_bottom() + vec2(-23.0 - metres * view.pixels_per_m, -15.0);
    painter.rect_filled(
        Rect::from_min_size(
            p + vec2(-6.0, -22.0),
            vec2((metres * view.pixels_per_m + 12.0).max(44.0), 28.0),
        ),
        4.0,
        background,
    );
    painter.line_segment(
        [p, p + vec2(metres * view.pixels_per_m, 0.0)],
        Stroke::new(2.0_f32, foreground),
    );
    painter.text(
        p + vec2(0.0, -4.0),
        Align2::LEFT_BOTTOM,
        format!("{metres:.0} m"),
        FontId::monospace(11.0),
        foreground,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selecting_a_new_place_preserves_occupied_zones_until_confirmation() {
        let ctx = Context::default();
        let mut planner = Planner::new(&ctx);
        planner.dirty = true;
        let next = Place {
            label: "New site".to_owned(),
            origin: Origin {
                latitude: 36.35,
                longitude: 127.38,
            },
        };
        planner.choose_site(next.clone());
        assert_eq!(planner.project.zones.len(), 3);
        assert!(planner.pending_place.is_some());
        planner.start_site(next.clone());
        assert!(planner.project.zones.is_empty());
        assert_eq!(planner.project.origin, Some(next.origin));
        assert!(planner.mode == Mode::Add);
    }

    #[test]
    fn geographic_overlay_aligns_with_walkers_projection() {
        let origin = Origin::EXAMPLE;
        let position = walkers::lat_lon(origin.latitude, origin.longitude);
        let mut memory = walkers::MapMemory::default();
        memory.set_zoom(18.0).unwrap();
        memory.center_at(walkers::lat_lon(
            origin.latitude + 0.0004,
            origin.longitude - 0.0005,
        ));
        let bounds = Rect::from_min_size(pos2(10.0, 35.0), vec2(700.0, 550.0));
        let projector = walkers::Projector::new(bounds, &memory, position);
        let view = View::projected(bounds, &projector, position);
        let point = Point::new(35.0, -25.0);
        let [lon, lat] = origin.lon_lat(point);
        let expected = projector.project(walkers::lat_lon(lat, lon)).to_pos2();
        assert!(view.position(point).distance(expected) < 0.01);
        assert!(view.point(expected).distance(point) < 0.01);
    }
}
