use crate::camera::Camera;
use crate::canvas::{Scene, build_scene, root_world, world_rect};
use crate::panels::{EntryGroupView, group_entries};
use crate::focus::{Focus, FocusMode, History, Mark, UsageIndex, focus};
use crate::search::SearchIndex;
use crate::shot::{Shot, ShotPlan, ShotStep};
use crate::input::{Nav, interpret};
use crate::{minimap, style};
use egui::{Align2, Color32, FontId, Key, Pos2, Sense, Shape, Stroke, StrokeKind};
use srcrelief_format::*;
use srcrelief_index::{ProjectConfig, Progress, SettingsPaths, SettingsStore, index_path, index_project, needs_reindex, project_packs};
use srcrelief_layout::{Direction, LayoutCache, LayoutOptions, Rect};
use srcrelief_rules::VisualRules;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

const RECENT_LIMIT: usize = 8;

enum Msg {
    Progress(Progress),
    Done(Result<PathBuf, String>),
}

enum State {
    Start { error: Option<String> },
    Indexing { dir: PathBuf, visual: VisualRules, rx: Receiver<Msg>, stage: String, started: Instant },
    Open(Box<Project>),
}

pub struct ReliefApp {
    state: State,
    config: Option<PathBuf>,
    recent: Vec<PathBuf>,
    shot: Option<Shot>,
}

fn recent_file() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("srcrelief").join("recent.toml"))
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct RecentFile {
    recent: Vec<PathBuf>,
}

fn load_recent() -> Vec<PathBuf> {
    recent_file().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| toml::from_str::<RecentFile>(&s).ok()).map(|r| r.recent).unwrap_or_default()
}

fn save_recent(list: &[PathBuf]) {
    let Some(path) = recent_file() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(s) = toml::to_string(&RecentFile { recent: list.to_vec() }) {
        let _ = std::fs::write(path, s);
    }
}

impl ReliefApp {
    pub fn new(cc: &eframe::CreationContext<'_>, dir: Option<PathBuf>, config: Option<PathBuf>, shot: Option<ShotPlan>) -> ReliefApp {
        cc.egui_ctx.set_visuals(style::visuals());
        let mut app = ReliefApp { state: State::Start { error: None }, config, recent: load_recent(), shot: shot.map(Shot::new) };
        if let Some(d) = dir {
            app.open(d);
        }
        app
    }

    fn open(&mut self, dir: PathBuf) {
        let dir = dir.canonicalize().unwrap_or(dir);
        self.recent.retain(|r| r != &dir);
        self.recent.insert(0, dir.clone());
        self.recent.truncate(RECENT_LIMIT);
        save_recent(&self.recent);
        let prepared = (|| -> anyhow::Result<(ProjectConfig, srcrelief_rules::Packs, PathBuf)> {
            let cfg = SettingsStore::load(SettingsPaths::new(&dir, self.config.as_deref()))?.config();
            let packs = project_packs(&dir, &cfg)?;
            let path = index_path(&dir, &cfg);
            Ok((cfg, packs, path))
        })();
        let (cfg, packs, path) = match prepared {
            Ok(p) => p,
            Err(e) => {
                self.state = State::Start { error: Some(format!("{e:#}")) };
                return;
            }
        };
        let visual = packs.visual();
        if !needs_reindex(&dir, &path, &cfg, &packs) {
            self.state = match Project::open(dir, &path, visual) {
                Ok(p) => State::Open(Box::new(p)),
                Err(e) => State::Start { error: Some(e) },
            };
            return;
        }
        let (tx, rx) = channel();
        let worker_dir = dir.clone();
        std::thread::spawn(move || {
            let progress_tx = tx.clone();
            let result = index_project(&worker_dir, &cfg, &move |p| {
                let _ = progress_tx.send(Msg::Progress(p));
            });
            let _ = tx.send(Msg::Done(result.map(|o| o.path).map_err(|e| format!("{e:#}"))));
        });
        self.state = State::Indexing { dir, visual, rx, stage: "Starting".into(), started: Instant::now() };
    }

    fn start_screen(&mut self, ui: &mut egui::Ui, error: Option<String>) {
        let mut chosen: Option<PathBuf> = None;
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(80.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("SrcRelief").size(34.0).color(style::ACCENT).strong());
                ui.label(egui::RichText::new("Open a Java (Gradle) or Go project folder to see its map").color(style::MUTED));
                ui.add_space(24.0);
                if ui.button(egui::RichText::new("Open folder…").size(16.0)).clicked() {
                    chosen = rfd::FileDialog::new().pick_folder();
                }
                if let Some(e) = &error {
                    ui.add_space(12.0);
                    ui.colored_label(style::EDGE_UP, e);
                }
                if !self.recent.is_empty() {
                    ui.add_space(24.0);
                    ui.label(egui::RichText::new("Recent").color(style::MUTED));
                    for r in &self.recent {
                        if ui.link(r.display().to_string()).clicked() {
                            chosen = Some(r.clone());
                        }
                    }
                }
            });
        });
        if let Some(d) = chosen {
            self.open(d);
        }
    }
}

impl eframe::App for ReliefApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let state = std::mem::replace(&mut self.state, State::Start { error: None });
        match state {
            State::Start { error } => {
                self.start_screen(ui, error.clone());
                if matches!(self.state, State::Start { .. }) {
                    self.state = State::Start { error };
                }
            }
            State::Indexing { dir, visual, rx, mut stage, started } => {
                let mut done = None;
                while let Ok(msg) = rx.try_recv() {
                    match msg {
                        Msg::Progress(p) => stage = format!("{p:?}"),
                        Msg::Done(r) => done = Some(r),
                    }
                }
                match done {
                    Some(Ok(path)) => {
                        self.state = match Project::open(dir, &path, visual) {
                            Ok(p) => State::Open(Box::new(p)),
                            Err(e) => State::Start { error: Some(e) },
                        }
                    }
                    Some(Err(e)) => self.state = State::Start { error: Some(e) },
                    None => {
                        egui::CentralPanel::default().show(ui, |ui| {
                            ui.add_space(120.0);
                            ui.vertical_centered(|ui| {
                                ui.spinner();
                                ui.label(format!("Indexing {} — {stage}… {:.1}s", dir.display(), started.elapsed().as_secs_f32()));
                            });
                        });
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                        self.state = State::Indexing { dir, visual, rx, stage, started };
                    }
                }
            }
            State::Open(mut project) => {
                let action = project.ui(ui);
                if let Some(shot) = &mut self.shot {
                    match shot.step(project.target.is_some()) {
                        ShotStep::Setup => project.apply_plan(&shot.plan),
                        ShotStep::Capture => shot.request(ui.ctx()),
                        ShotStep::Close => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
                        ShotStep::Wait => {}
                    }
                    ui.ctx().request_repaint();
                }
                self.state = State::Open(project);
                match action {
                    Some(Action::OpenOther) => {
                        if let Some(d) = rfd::FileDialog::new().pick_folder() {
                            self.open(d);
                        }
                    }
                    Some(Action::Reindex(dir)) => {
                        let path = match &self.state {
                            State::Open(p) => p.index_path.clone(),
                            _ => return,
                        };
                        let _ = std::fs::remove_file(&path);
                        self.open(dir);
                    }
                    None => {}
                }
            }
        }
    }
}

enum Action {
    OpenOther,
    Reindex(PathBuf),
}

struct Totals {
    modules: usize,
    packages: usize,
    types: usize,
    lines: u64,
    apps: usize,
    tools: usize,
    tests: usize,
}

struct Project {
    dir: PathBuf,
    index_path: PathBuf,
    file: ReliefFile,
    visual: VisualRules,
    cache: LayoutCache,
    cam: Camera,
    target: Option<Camera>,
    needs_fit: bool,
    view: Rect,
    scene: Scene,
    selected: Option<NodeId>,
    selected_entry: Option<usize>,
    focus: Option<Focus>,
    focus_mode: FocusMode,
    usage: UsageIndex,
    history: History,
    pending_refocus: bool,
    pending_back: bool,
    pending_clear: bool,
    pending_go: Option<NodeId>,
    groups: Vec<EntryGroupView>,
    search: SearchIndex,
    search_text: String,
    search_open: bool,
    search_hits: Vec<NodeId>,
    include_test_edges: bool,
    app_counts: HashMap<NodeId, usize>,
    totals: Totals,
    scene_ms: f32,
    last_frame: Instant,
}

fn erect(r: Rect) -> egui::Rect {
    egui::Rect::from_min_size(Pos2::new(r.x, r.y), egui::vec2(r.w, r.h))
}

fn rect_of(r: egui::Rect) -> Rect {
    Rect::new(r.min.x, r.min.y, r.width(), r.height())
}

fn pos(p: [f32; 2]) -> Pos2 {
    Pos2::new(p[0], p[1])
}

const LIST_LIMIT: usize = 60;

fn chip_rect(r: Rect) -> Rect {
    Rect::new(r.right() - 100.0, r.y + 6.0, 92.0, 18.0)
}

fn short_path(g: &ArchivedGraph, n: NodeId) -> String {
    let path = g.path(n);
    let tail: Vec<&str> = path.iter().skip(1).rev().take(2).map(|&p| g.name(p)).collect();
    tail.into_iter().rev().collect::<Vec<_>>().join(" / ")
}

fn full_path(g: &ArchivedGraph, n: NodeId) -> String {
    g.path(n).iter().skip(1).map(|&p| g.name(p)).collect::<Vec<_>>().join(" / ")
}

fn lifecycle_badge(l: Lifecycle) -> &'static str {
    match l {
        Lifecycle::Once => "once",
        Lifecycle::Repeating => "every frame",
        Lifecycle::OnEvent => "on event",
        Lifecycle::Later => "later",
        Lifecycle::Unknown => "callback",
    }
}

impl Project {
    fn open(dir: PathBuf, path: &Path, visual: VisualRules) -> Result<Project, String> {
        let file = ReliefFile::open(path).map_err(|e| format!("{e}"))?;
        let g = file.graph();
        let groups = group_entries(g, &visual);
        let search = SearchIndex::new(g);
        let usage = UsageIndex::new(g);
        let mut app_counts: HashMap<NodeId, usize> = HashMap::new();
        let mut totals = Totals { modules: 0, packages: 0, types: 0, lines: g.subtree_lines(g.root()), apps: 0, tools: 0, tests: 0 };
        for i in 0..g.entries().len() {
            if g.entry_kind(i) != "main" {
                continue;
            }
            match g.entry_category(i) {
                Category::Application => {
                    totals.apps += 1;
                    for n in g.path(g.entries()[i].node.to_native()) {
                        *app_counts.entry(n).or_default() += 1;
                    }
                }
                Category::DevTool => totals.tools += 1,
                Category::Test => totals.tests += 1,
            }
        }
        for n in 0..g.node_count() as NodeId {
            if g.flags(n).intersects(NodeFlags::EXTERNAL | NodeFlags::MOCK | NodeFlags::TEST) {
                continue;
            }
            match g.kind(n) {
                NodeKind::Module => totals.modules += 1,
                NodeKind::Package => totals.packages += 1,
                NodeKind::Type => totals.types += 1,
                _ => {}
            }
        }
        let cache = LayoutCache::new(visual.frames.clone(), LayoutOptions::default());
        Ok(Project {
            dir,
            index_path: path.to_path_buf(),
            file,
            visual,
            cache,
            cam: Camera::default(),
            target: None,
            needs_fit: true,
            view: Rect::new(0.0, 0.0, 1.0, 1.0),
            scene: Scene::default(),
            selected: None,
            selected_entry: None,
            focus: None,
            focus_mode: FocusMode::Neighbours,
            usage,
            history: History::default(),
            pending_refocus: false,
            pending_back: false,
            pending_clear: false,
            pending_go: None,
            groups,
            search,
            search_text: String::new(),
            search_open: false,
            search_hits: Vec::new(),
            include_test_edges: false,
            app_counts,
            totals,
            scene_ms: 0.0,
            last_frame: Instant::now(),
        })
    }

    fn fly_to(&mut self, node: NodeId) {
        let g = self.file.graph();
        if let Some(r) = world_rect(g, &mut self.cache, node) {
            self.target = Some(Camera::fit(r, self.view));
        }
    }

    fn apply_plan(&mut self, plan: &ShotPlan) {
        let entry = plan.entry.as_ref().and_then(|label| {
            let g = self.file.graph();
            (0..g.entries().len()).find(|&i| g.entry_label(i) == label)
        });
        if let Some(i) = entry {
            self.select_entry(i);
        }
        let focus = plan.focus.as_ref().and_then(|name| self.search.query(self.file.graph(), name, 1).first().copied());
        if let Some(n) = focus {
            self.selected = Some(n);
            self.fly_to(n);
        }
    }

    fn select_entry(&mut self, i: usize) {
        let node = self.file.graph().entries()[i].node.to_native();
        self.set_focus(node, Some(i), true, true);
    }

    fn set_focus(&mut self, node: NodeId, entry: Option<usize>, fly: bool, record: bool) {
        let g = self.file.graph();
        self.focus = Some(focus(g, &self.usage, node, entry, self.focus_mode));
        self.selected = Some(node);
        self.selected_entry = entry;
        if record {
            self.history.push(node);
        }
        if fly {
            let hidden = self.cache.options().effective_hidden();
            let leaf = !g.children(node).any(|c| !g.flags(c).intersects(hidden));
            let frame = if leaf { g.parent(node).filter(|&p| p != g.root()).unwrap_or(node) } else { node };
            self.fly_to(frame);
        }
    }

    fn refocus(&mut self) {
        if let Some(f) = &self.focus {
            let (node, entry) = (f.node, f.entry);
            self.set_focus(node, entry, false, false);
        }
    }

    fn go_back(&mut self) {
        if let Some(n) = self.history.back() {
            self.set_focus(n, None, true, false);
        }
    }

    fn clear_focus(&mut self) {
        self.focus = None;
        self.selected_entry = None;
    }

    fn ui(&mut self, ui: &mut egui::Ui) -> Option<Action> {
        let mut action = None;
        if ui.input(|i| i.modifiers.command && i.key_pressed(Key::K)) {
            self.search_open = true;
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            self.search_open = false;
        }
        if ui.input(|i| i.modifiers.alt && i.key_pressed(Key::ArrowLeft)) {
            self.go_back();
        }
        egui::Panel::top("top").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("SrcRelief").color(style::ACCENT).strong());
                ui.separator();
                self.breadcrumb(ui);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Open…").clicked() {
                        action = Some(Action::OpenOther);
                    }
                    if ui.button("Re-index").clicked() {
                        action = Some(Action::Reindex(self.dir.clone()));
                    }
                    if ui.button("Jump to… (Ctrl+K)").clicked() {
                        self.search_open = true;
                    }
                });
            });
        });
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("entries").resizable(true).default_size(270.0).show(ui, |ui| self.entries_panel(ui));
        egui::Panel::right("inspector").resizable(true).default_size(290.0).show(ui, |ui| self.inspector(ui));
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(style::BG)).show(ui, |ui| self.canvas(ui));
        if self.search_open {
            self.search_window(ui.ctx());
        }
        action
    }

    fn breadcrumb(&mut self, ui: &mut egui::Ui) {
        let g = self.file.graph();
        let deepest = self.scene.deepest_open_at(self.view.center()).unwrap_or(g.root());
        let mut fly = None;
        for (i, n) in g.path(deepest).into_iter().enumerate() {
            if i > 0 {
                ui.label(egui::RichText::new("/").color(style::MUTED));
            }
            if ui.link(g.name(n)).clicked() {
                fly = Some(n);
            }
        }
        if let Some(n) = fly {
            if n == g.root() {
                self.needs_fit = true;
            } else {
                self.fly_to(n);
            }
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let g = self.file.graph();
        let s = g.stats();
        let (r, gu, u) = (s.calls_resolved.to_native(), s.calls_guessed.to_native(), s.calls_unresolved.to_native());
        let total = (r + gu + u).max(1) as f32;
        let level = self.scene.deepest_open_at(self.view.center()).map(|n| format!("{:?}", g.kind(n))).unwrap_or_else(|| "Project".into());
        let up = self.cache.get(g.root()).map_or(0, |l| l.up_edges());
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("zoom {:.0}% · inside {level}", self.cam.zoom * 100.0)).color(style::MUTED));
            ui.separator();
            let t = &self.totals;
            ui.label(egui::RichText::new(format!("{} modules · {} packages · {} types · {:.0}k loc", t.modules, t.packages, t.types, t.lines as f32 / 1000.0)).color(style::MUTED));
            ui.separator();
            ui.label(egui::RichText::new(format!("{} apps · {} dev tools", t.apps, t.tools)).color(style::MUTED));
            ui.separator();
            ui.label(egui::RichText::new(format!("calls {:.0}% resolved · {:.0}% guessed", r as f32 * 100.0 / total, gu as f32 * 100.0 / total)).color(style::MUTED));
            if up > 0 {
                ui.separator();
                ui.colored_label(style::EDGE_UP, format!("{up} upward module dependencies"));
            }
            let skipped: Vec<String> = g.skipped_roots().map(|(_, p)| p.to_string()).collect();
            if !skipped.is_empty() {
                ui.separator();
                ui.label(egui::RichText::new(format!("not indexed: {}", skipped.join(", "))).color(style::MUTED));
            }
            ui.separator();
            let mut tests = self.include_test_edges;
            if ui.checkbox(&mut tests, "test deps").changed() {
                self.include_test_edges = tests;
                let mut opts = *self.cache.options();
                opts.include_test_edges = tests;
                self.cache.set_options(opts);
            }
            let mut details = self.cache.options().show_details;
            if ui.checkbox(&mut details, "details").on_hover_text("show data types, helpers and other non-architectural types").changed() {
                let mut opts = *self.cache.options();
                opts.show_details = details;
                self.cache.set_options(opts);
            }
            let mut errors = self.cache.options().show_noise;
            if ui.checkbox(&mut errors, "errors").on_hover_text("show exception and error types").changed() {
                let mut opts = *self.cache.options();
                opts.show_noise = errors;
                self.cache.set_options(opts);
            }
            let mut wiring = self.cache.options().show_wiring;
            if ui.checkbox(&mut wiring, "wiring").on_hover_text("show dependency-injection code and its arrows").changed() {
                let mut opts = *self.cache.options();
                opts.show_wiring = wiring;
                self.cache.set_options(opts);
            }
            ui.label(egui::RichText::new(format!("{:.1} ms · {} boxes", self.scene_ms, self.scene.stats.drawn)).color(style::MUTED));
        });
    }

    fn entries_panel(&mut self, ui: &mut egui::Ui) {
        let mut select: Option<usize> = None;
        let mut fly: Option<NodeId> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            let g = self.file.graph();
            ui.label(egui::RichText::new("ENTRY POINTS").small().color(style::MUTED));
            for (gi, group) in self.groups.iter().enumerate() {
                egui::CollapsingHeader::new(format!("{} ({})", group.label, group.items.len())).id_salt(("group", gi)).default_open(!group.collapsed).show(ui, |ui| {
                    for item in &group.items {
                        let i = item.entry as usize;
                        let mut text = g.entry_label(i).to_string();
                        if let Some(h) = g.entry_handoff(i) {
                            text.push_str(&format!("  · {h}"));
                        }
                        let selected = self.selected_entry == Some(i);
                        if item.children.is_empty() {
                            if ui.selectable_label(selected, text).clicked() {
                                select = Some(i);
                            }
                            continue;
                        }
                        let id = ui.make_persistent_id(("entry", i));
                        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
                            .show_header(ui, |ui| {
                                if ui.selectable_label(selected, format!("{text}  ({})", item.children.len())).clicked() {
                                    select = Some(i);
                                }
                            })
                            .body(|ui| {
                                for &c in &item.children {
                                    let c = c as usize;
                                    let label = format!("{}  [{}]", g.entry_label(c), lifecycle_badge(g.entry_lifecycle(c)));
                                    if ui.selectable_label(self.selected_entry == Some(c), label).clicked() {
                                        select = Some(c);
                                    }
                                }
                            });
                    }
                });
            }
            ui.add_space(12.0);
            ui.label(egui::RichText::new("MODULES").small().color(style::MUTED));
            let hidden = self.cache.options().hidden;
            for m in g.children(g.root()).filter(|&m| !g.flags(m).intersects(hidden)) {
                egui::CollapsingHeader::new(g.name(m)).id_salt(("mod", m)).default_open(false).show(ui, |ui| {
                    if ui.small_button("show").clicked() {
                        fly = Some(m);
                    }
                    for p in g.children(m).filter(|&p| !g.flags(p).intersects(hidden)).take(300) {
                        if ui.selectable_label(self.selected == Some(p), g.name(p)).clicked() {
                            fly = Some(p);
                        }
                    }
                });
            }
        });
        if let Some(i) = select {
            self.select_entry(i);
        }
        if let Some(n) = fly {
            self.selected = Some(n);
            self.fly_to(n);
        }
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        let g = self.file.graph();
        egui::ScrollArea::vertical().show(ui, |ui| {
            if let Some(i) = self.selected_entry {
                ui.label(egui::RichText::new("ENTRY POINT").small().color(style::MUTED));
                ui.heading(g.entry_label(i));
                egui::Grid::new("entry").num_columns(2).show(ui, |ui| {
                    ui.label("Kind");
                    ui.label(g.entry_kind(i));
                    ui.end_row();
                    ui.label("Category");
                    ui.label(g.entry_category(i).label());
                    ui.end_row();
                    ui.label("Lifecycle");
                    ui.label(g.entry_lifecycle(i).label());
                    ui.end_row();
                    if let Some(h) = g.entry_handoff(i) {
                        ui.label("Hands off to");
                        ui.label(h);
                        ui.end_row();
                    }
                    ui.label("Rule");
                    ui.label(egui::RichText::new(g.entry_rule(i)).color(style::MUTED));
                    ui.end_row();
                });
                ui.separator();
            }
            if self.focus.is_some() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new("SHOW").small().color(style::MUTED));
                    for (mode, label) in [(FocusMode::Neighbours, "neighbours"), (FocusMode::Reaches, "everything it reaches"), (FocusMode::UsedBy, "everywhere it's used")] {
                        if ui.selectable_label(self.focus_mode == mode, label).clicked() && self.focus_mode != mode {
                            self.focus_mode = mode;
                            self.pending_refocus = true;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("Back (Alt+Left)").clicked() {
                        self.pending_back = true;
                    }
                    if ui.button("Clear").clicked() {
                        self.pending_clear = true;
                    }
                });
                ui.separator();
            }
            let Some(n) = self.selected else {
                ui.label(egui::RichText::new("Click a box to inspect it. Double-click to zoom into it.").color(style::MUTED));
                return;
            };
            let kind = match g.sub_kind(n) {
                SubKind::None => format!("{:?}", g.kind(n)),
                sub => format!("{sub:?}"),
            };
            let wiring = if g.flags(n).contains(NodeFlags::WIRING) { " · WIRING (DEPENDENCY INJECTION)" } else { "" };
            ui.label(egui::RichText::new(format!("{}{wiring}", kind.to_uppercase())).small().color(style::MUTED));
            ui.heading(g.name(n));
            if let Some(f) = g.file(n) {
                let (s, e) = g.span(n);
                ui.label(egui::RichText::new(format!("{f}:{s}-{e}")).small().color(style::MUTED));
            }
            let parent = g.parent(n);
            let (mut depends, mut used) = (0, 0);
            if let Some(p) = parent {
                for a in g.aggregates(p) {
                    if a.a.to_native() == n {
                        depends += 1;
                    }
                    if a.b.to_native() == n {
                        used += 1;
                    }
                }
            }
            let hidden = self.cache.options().hidden;
            let mut counts: HashMap<NodeKind, usize> = HashMap::new();
            for c in g.children(n).filter(|&c| !g.flags(c).intersects(hidden)) {
                *counts.entry(g.kind(c)).or_default() += 1;
            }
            egui::Grid::new("node").num_columns(2).show(ui, |ui| {
                for (kind, label) in [(NodeKind::Module, "Modules"), (NodeKind::Package, "Packages"), (NodeKind::Type, "Types"), (NodeKind::Member, "Members")] {
                    if let Some(c) = counts.get(&kind) {
                        ui.label(label);
                        ui.label(c.to_string());
                        ui.end_row();
                    }
                }
                ui.label("Lines");
                ui.label(g.subtree_lines(n).to_string());
                ui.end_row();
                ui.label("Language");
                ui.label(format!("{:?}", g.lang(n)));
                ui.end_row();
                ui.label("Depends on");
                ui.label(format!("{depends} siblings"));
                ui.end_row();
                ui.label("Used by");
                ui.label(format!("{used} siblings"));
                ui.end_row();
                if let Some(c) = self.app_counts.get(&n) {
                    ui.label("Applications inside");
                    ui.label(c.to_string());
                    ui.end_row();
                }

            });
            let tags: Vec<&str> = g.tags(n).collect();
            if !tags.is_empty() {
                ui.label(format!("Tags: {}", tags.join(", ")));
            }
            if let Some(l) = self.cache.get(n) {
                let up = l.up_edges();
                if up > 0 {
                    ui.colored_label(style::EDGE_UP, format!("{up} upward dependencies inside"));
                }
            }
            if let Some(p) = parent {
                if let Some(l) = self.cache.get(p) {
                    for e in l.edges.iter().filter(|e| e.dir == Direction::Up && (e.a == n || e.b == n)) {
                        ui.colored_label(style::EDGE_UP, format!("Upward: {} -> {}", g.name(e.a), g.name(e.b)));
                    }
                }
            }
            for i in (0..g.entries().len()).filter(|&i| g.entries()[i].node.to_native() == n) {
                ui.colored_label(style::ENTRY, format!("Entry point: {}", g.entry_label(i)));
            }
            if let Some(f) = self.focus.as_ref().filter(|f| f.node == n) {
                for (title, list, color) in [("SET UP / CALLED BY", &f.callers, style::CALLER), ("CALLS", &f.calls, style::REACH)] {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(format!("{title} ({})", list.len())).small().color(color));
                    for &m in list.iter().take(LIST_LIMIT) {
                        if ui.selectable_label(false, short_path(g, m)).on_hover_text(full_path(g, m)).clicked() {
                            self.pending_go = Some(m);
                        }
                    }
                    if list.len() > LIST_LIMIT {
                        ui.label(egui::RichText::new(format!("… {} more", list.len() - LIST_LIMIT)).color(style::MUTED));
                    }
                }
            }
        });
        if std::mem::take(&mut self.pending_refocus) {
            self.refocus();
        }
        if std::mem::take(&mut self.pending_back) {
            self.go_back();
        }
        if std::mem::take(&mut self.pending_clear) {
            self.clear_focus();
        }
        if let Some(m) = self.pending_go.take() {
            self.set_focus(m, None, true, true);
        }
    }

    fn search_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut fly = None;
        egui::Window::new("Jump to").open(&mut open).collapsible(false).resizable(false).anchor(Align2::CENTER_TOP, [0.0, 60.0]).show(ctx, |ui| {
            let edit = ui.add(egui::TextEdit::singleline(&mut self.search_text).hint_text("type, package or module name").desired_width(380.0));
            edit.request_focus();
            if edit.changed() {
                self.search_hits = self.search.query(self.file.graph(), &self.search_text, 30);
            }
            let g = self.file.graph();
            if ui.input(|i| i.key_pressed(Key::Enter)) {
                fly = self.search_hits.first().copied();
            }
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for &h in &self.search_hits {
                    let path: Vec<&str> = g.path(h).iter().skip(1).map(|&p| g.name(p)).collect();
                    if ui.selectable_label(false, format!("{}   {}", g.name(h), path[..path.len().saturating_sub(1)].join(" / "))).clicked() {
                        fly = Some(h);
                    }
                }
            });
        });
        if let Some(n) = fly {
            self.selected = Some(n);
            self.fly_to(n);
            self.search_open = false;
        } else {
            self.search_open = open;
        }
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        self.view = rect_of(resp.rect);
        let g = self.file.graph();
        if self.needs_fit {
            self.target = None;
            self.cam = Camera::fit(root_world(&mut self.cache, g), self.view);
            self.needs_fit = false;
        }
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        if resp.dragged() {
            let d = resp.drag_delta();
            self.cam.pan_by([d.x, d.y]);
            self.target = None;
        }
        if let Some(p) = resp.hover_pos() {
            let (scroll, pinch, mods) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers));
            match interpret(mods.command || mods.ctrl, mods.shift, [scroll.x, scroll.y], pinch) {
                Nav::Zoom(f) => {
                    self.cam.zoom_at([p.x, p.y], f, self.view);
                    self.target = None;
                }
                Nav::Pan(d) => {
                    self.cam.pan_by(d);
                    self.target = None;
                }
                Nav::None => {}
            }
        }
        let mut clicked_box = None;
        if resp.clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                let root = self.cache.shared(g, g.root());
                if let Some(w) = minimap::world_at(&root, self.view, [p.x, p.y]) {
                    self.target = Some(Camera { center: w, zoom: self.cam.zoom });
                } else if self.scene.boxes.iter().any(|b| b.hidden_details > 0 && chip_rect(b.rect).contains([p.x, p.y])) {
                    let mut opts = *self.cache.options();
                    opts.show_details = true;
                    self.cache.set_options(opts);
                } else {
                    clicked_box = self.scene.box_at([p.x, p.y]);
                }
            }
        }
        let mut fly = None;
        if resp.double_clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                if let Some(n) = self.scene.box_at([p.x, p.y]) {
                    self.selected = Some(n);
                    fly = Some(n);
                }
            }
        }
        if let Some(t) = self.target {
            if !self.cam.animate_to(&t, dt) {
                self.target = None;
            }
            ui.ctx().request_repaint();
        }
        let started = Instant::now();
        self.scene = build_scene(g, &mut self.cache, &self.cam, self.view, &self.visual, self.focus.as_ref().map(|f| &f.marks));
        self.scene_ms = started.elapsed().as_secs_f32() * 1000.0;
        self.paint(&painter);
        let root = self.cache.shared(g, g.root());
        minimap::draw(&painter, &root, &self.cam, self.view);
        self.paint_legend(&painter);
        if let Some(n) = clicked_box {
            self.set_focus(n, None, false, true);
        }
        if let Some(n) = fly {
            self.set_focus(n, None, true, true);
        }
    }

    fn paint(&self, painter: &egui::Painter) {
        painter.rect_filled(erect(self.view), 0.0, style::BG);
        let painter = painter.with_clip_rect(erect(self.view));
        let max_depth = self.scene.boxes.iter().map(|b| b.depth).max().unwrap_or(0);
        for depth in 1..=max_depth {
            for e in self.scene.edges.iter().filter(|e| e.depth == depth) {
                self.paint_edge(&painter, e);
            }
            for b in self.scene.boxes.iter().filter(|b| b.depth == depth) {
                self.paint_box(&painter, b);
            }
            for f in self.scene.frames.iter().filter(|f| f.depth == depth) {
                let r = erect(f.rect);
                let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                painter.extend(Shape::dashed_line(&pts, Stroke::new(1.0, style::FRAME), 5.0, 3.0));
                painter.text(r.left_top() + egui::vec2(6.0, 3.0), Align2::LEFT_TOP, &f.label, FontId::proportional(11.0), style::MUTED);
            }
            for l in self.scene.labels.iter().filter(|l| l.depth == depth) {
                self.paint_label(&painter, l);
            }
        }
    }

    fn paint_box(&self, painter: &egui::Painter, b: &crate::canvas::SceneBox) {
        let r = erect(b.rect);
        let external = b.flags.contains(NodeFlags::EXTERNAL);
        let wiring = b.flags.contains(NodeFlags::WIRING);
        let accent = if external {
            style::EXTERNAL
        } else if wiring {
            style::WIRING
        } else if b.flags.contains(NodeFlags::NOISE) {
            style::ERROR_TYPE
        } else {
            style::kind_stroke(b.sub)
        };
        let fill = if external { Color32::from_rgb(43, 43, 43) } else { style::tint(style::depth_fill(b.depth, b.open), accent, if b.open { 0.05 } else { 0.12 }) };
        let fill = if b.dimmed { style::tint(fill, style::BG, 0.6) } else { fill };
        painter.rect_filled(r, 3.0, fill);
        let mut stroke = Stroke::new(if b.open { 1.5 } else { 1.0 }, accent);
        if let Some(c) = b.tag_color {
            stroke.color = Color32::from_rgb(c[0], c[1], c[2]);
        }
        match b.mark {
            Mark::Down => stroke = Stroke::new(2.0, style::REACH),
            Mark::Up => stroke = Stroke::new(2.0, style::CALLER),
            Mark::Focus => stroke = Stroke::new(1.5, style::ACCENT),
            Mark::None => {}
        }
        if b.dimmed {
            stroke.color = stroke.color.gamma_multiply(0.3);
        }
        if self.selected == Some(b.node) {
            stroke = Stroke::new(2.5, style::ACCENT);
        }
        painter.rect_stroke(r, 3.0, stroke, StrokeKind::Inside);
        if b.open && b.hidden_details > 0 && b.rect.w > 140.0 {
            let chip = erect(chip_rect(b.rect));
            painter.rect_filled(chip, 8.0, style::BAR);
            painter.rect_stroke(chip, 8.0, Stroke::new(1.0, style::FRAME), StrokeKind::Inside);
            painter.text(chip.center(), Align2::CENTER_CENTER, format!("+ {} details", b.hidden_details), FontId::proportional(10.5), style::MUTED);
        }
        if self.app_counts.contains_key(&b.node) && b.rect.w > 24.0 {
            painter.circle_filled(Pos2::new(b.rect.right() - 9.0, b.rect.y + 9.0), 4.0, style::ENTRY);
        }
    }

    fn paint_edge(&self, painter: &egui::Painter, e: &crate::canvas::SceneEdge) {
        let pts: Vec<Pos2> = e.points.iter().map(|p| pos(*p)).collect();
        let (mut color, mut dashed) = match e.dir {
            Direction::Down => (style::EDGE, false),
            Direction::Up => (style::EDGE_UP, true),
            Direction::Sideways => (style::EDGE_SIDEWAYS, false),
        };
        if e.undeclared {
            color = style::EDGE_SIDEWAYS;
            dashed = true;
        }
        if e.test_only {
            dashed = true;
        }
        let marked = e.mark != Mark::None;
        let width = style::edge_width(e.weight, marked);
        match e.mark {
            Mark::Down | Mark::Focus => color = style::REACH,
            Mark::Up => color = style::CALLER,
            Mark::None => {}
        }
        if e.dimmed {
            color = color.gamma_multiply(0.2);
        }
        let wiring_only = e.weight == 0 && e.implements == 0 && e.wiring > 0;
        let implements_only = e.weight == 0 && e.implements > 0;
        if wiring_only {
            painter.extend(Shape::dashed_line(&pts, Stroke::new(1.0, style::WIRING.gamma_multiply(0.7)), 2.0, 4.0));
            return;
        }
        let stroke = Stroke::new(width, color);
        if dashed || implements_only {
            painter.extend(Shape::dashed_line(&pts, stroke, if implements_only { 3.0 } else { 6.0 }, 4.0));
        } else {
            painter.add(Shape::line(pts.clone(), stroke));
        }
        if pts.len() >= 2 {
            let (a, b) = (pts[pts.len() - 2], pts[pts.len() - 1]);
            let dir = (b - a).normalized();
            let head = if implements_only { 11.0 } else { 8.0 };
            let side = egui::vec2(-dir.y, dir.x) * head * if implements_only { 0.55 } else { 0.45 };
            let tri = vec![b, b - dir * head + side, b - dir * head - side];
            if implements_only {
                painter.add(Shape::convex_polygon(tri, style::BG, Stroke::new(1.3, color)));
            } else {
                painter.add(Shape::convex_polygon(tri, color, Stroke::NONE));
            }
        }
    }

    fn paint_legend(&self, painter: &egui::Painter) {
        let opts = self.cache.options();
        let items = style::legend(opts.show_wiring, opts.show_noise);
        let area = minimap::area(self.view);
        let row = 14.0;
        let half = items.len().div_ceil(2);
        let width = area.w * 2.0 + 8.0;
        let h = half as f32 * row + 10.0;
        let top = egui::Rect::from_min_size(Pos2::new(area.right() - width, area.y - h - 6.0), egui::vec2(width, h));
        painter.rect_filled(top, 4.0, style::BAR);
        for (i, it) in items.iter().enumerate() {
            let col = (i / half) as f32;
            let x = top.min.x + 8.0 + col * (width / 2.0);
            let y = top.min.y + 6.0 + (i % half) as f32 * row;
            let sample = egui::Rect::from_min_size(Pos2::new(x, y + 2.0), egui::vec2(18.0, 9.0));
            match it.sample {
                style::LegendSample::Box { stroke } => {
                    painter.rect_stroke(sample, 1.0, Stroke::new(1.5, stroke), StrokeKind::Inside);
                }
                style::LegendSample::Outline { stroke, width } => {
                    painter.rect_stroke(sample, 1.0, Stroke::new(width, stroke), StrokeKind::Inside);
                }
                style::LegendSample::Dot { color } => {
                    painter.circle_filled(sample.center(), 3.5, color);
                }
                style::LegendSample::Frame => {
                    let r = sample;
                    let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                    painter.extend(Shape::dashed_line(&pts, Stroke::new(1.0, style::FRAME), 3.0, 2.0));
                }
                style::LegendSample::Arrow { color, dashed, dotted, hollow_head } => {
                    let (a, b) = (sample.left_center(), sample.right_center());
                    let stroke = Stroke::new(1.4, color);
                    if dotted {
                        painter.extend(Shape::dashed_line(&[a, b], stroke, 2.0, 3.0));
                    } else if dashed {
                        painter.extend(Shape::dashed_line(&[a, b], stroke, 4.0, 3.0));
                    } else {
                        painter.line_segment([a, b], stroke);
                    }
                    let tri = vec![b, b + egui::vec2(-6.0, -3.5), b + egui::vec2(-6.0, 3.5)];
                    if hollow_head {
                        painter.add(Shape::convex_polygon(tri, style::BAR, Stroke::new(1.2, color)));
                    } else {
                        painter.add(Shape::convex_polygon(tri, color, Stroke::NONE));
                    }
                }
            }
            painter.text(Pos2::new(x + 26.0, y), Align2::LEFT_TOP, it.label, FontId::proportional(10.0), style::MUTED);
        }
    }

    fn paint_label(&self, painter: &egui::Painter, l: &crate::canvas::SceneLabel) {
        let Some(b) = self.scene.boxes.iter().find(|b| b.node == l.node) else { return };
        let clip = painter.with_clip_rect(erect(b.rect).shrink(2.0).intersect(erect(self.view)));
        let color = if b.flags.contains(NodeFlags::EXTERNAL) { style::MUTED } else { style::BOX_TITLE };
        let color = if b.dimmed { color.gamma_multiply(0.35) } else { color };
        clip.text(pos(l.pos), Align2::LEFT_TOP, &l.text, FontId::proportional(l.size), color);
        if let Some(d) = &l.detail {
            let mut detail = d.clone();
            if let Some(c) = self.app_counts.get(&l.node) {
                detail.push_str(&format!(" · {c} app{}", if *c == 1 { "" } else { "s" }));
            }
            clip.text(pos([l.pos[0], l.pos[1] + l.size + 4.0]), Align2::LEFT_TOP, detail, FontId::proportional((l.size * 0.75).max(9.0)), style::MUTED);
        }
    }
}
