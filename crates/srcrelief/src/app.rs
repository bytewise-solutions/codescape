use crate::camera::Camera;
use crate::canvas::{Scene, build_scene_at, fit_label, root_world, world_rect};
use crate::panels::{EntryGroupView, group_entries};
use crate::focus::{Focus, FocusMode, History, Mark, UsageIndex, focus};
use crate::search::{self, SearchIndex};
use crate::shot::{Shot, ShotPlan, ShotStep};
use crate::input::{CHEATSHEET, Nav, Step, interpret, nearest_in};
use crate::inspector;
use crate::review_view::{CommitStep, ItemKind, MarkState, ReviewItem, ReviewMarks, Risk, by_risk, items as review_items, step as review_step};
use srcrelief_index::analysis::{Analysis, Usage, analyse};
use std::sync::Arc;
use srcrelief_index::review::{Change, Counts, Review, Status, base_for, branches, commits, compare, current_branch, has_uncommitted, index_revision, is_git};
use crate::source::{LINE_SPACING, SourceCache, Token, advance, file_font, highlight, hscroll_by, initial_scroll, merge_spans, scroll_at, scroll_by, thumb};
use crate::settings_window::{self, SettingsForm};
use crate::view::{layout_options, toggle, view_items};
use crate::{minimap, style};
use egui::{Align2, Color32, FontId, Key, KeyboardShortcut, Modifiers, Pos2, Sense, Shape, Stroke, StrokeKind};
use srcrelief_format::*;
use srcrelief_index::{Key as SettingKey, ProjectConfig, Progress, SettingsPaths, SettingsStore, View, index_path, index_project, needs_reindex, project_packs};
use srcrelief_layout::{Direction, LayoutCache, Rect};
use srcrelief_rules::{PackSelection, VisualRules, load_packs, user_rules_dir};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

const RECENT_LIMIT: usize = 8;
const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const SETTINGS: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Comma);
const JUMP: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::K);

enum Msg {
    Progress(Progress),
    Done(Result<PathBuf, String>),
}

enum State {
    Start { error: Option<String> },
    Indexing { dir: PathBuf, visual: VisualRules, settings: Box<SettingsStore>, packs: Vec<String>, rx: Receiver<Msg>, stage: String, started: Instant },
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
        let prepared = (|| -> anyhow::Result<(SettingsStore, ProjectConfig, srcrelief_rules::Packs, PathBuf, Vec<String>)> {
            let settings = SettingsStore::load(SettingsPaths::new(&dir, self.config.as_deref()))?;
            let cfg = settings.config();
            let packs = project_packs(&dir, &cfg)?;
            let path = index_path(&dir, &cfg);
            let available = load_packs(Some(&dir), user_rules_dir().as_deref(), &PackSelection { extra_dirs: cfg.packs.extra_dirs.clone(), ..Default::default() })?;
            Ok((settings, cfg, packs, path, available.names().into_iter().map(String::from).collect()))
        })();
        let (settings, cfg, packs, path, available) = match prepared {
            Ok(p) => p,
            Err(e) => {
                self.state = State::Start { error: Some(format!("{e:#}")) };
                return;
            }
        };
        let visual = packs.visual();
        if !needs_reindex(&dir, &path, &cfg, &packs) {
            self.state = match Project::open(dir, &path, visual, settings, available) {
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
        self.state = State::Indexing { dir, visual, settings: Box::new(settings), packs: available, rx, stage: "Starting".into(), started: Instant::now() };
    }

    fn file_menu(&self, ui: &mut egui::Ui, project: bool) -> Option<Action> {
        let mut action = None;
        let shortcut = |s: &KeyboardShortcut| ui.ctx().format_shortcut(s);
        let (open, settings) = (shortcut(&OPEN), shortcut(&SETTINGS));
        ui.menu_button("File", |ui| {
            if ui.add(egui::Button::new("Open Folder…").shortcut_text(open)).clicked() {
                action = Some(Action::OpenFolder);
            }
            ui.add_enabled_ui(!self.recent.is_empty(), |ui| {
                ui.menu_button("Open Recent", |ui| {
                    for r in &self.recent {
                        if ui.button(r.display().to_string()).clicked() {
                            action = Some(Action::OpenRecent(r.clone()));
                        }
                    }
                });
            });
            if project {
                ui.separator();
                if ui.button("Re-index").clicked() {
                    action = Some(Action::Reindex);
                }
                if ui.add(egui::Button::new("Project Settings…").shortcut_text(settings)).clicked() {
                    action = Some(Action::Settings);
                }
                ui.separator();
                if ui.button("Close Project").clicked() {
                    action = Some(Action::Close);
                }
            }
        });
        action
    }

    fn act(&mut self, action: Action) {
        match action {
            Action::OpenFolder => {
                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                    self.open(d);
                }
            }
            Action::OpenRecent(d) => self.open(d),
            Action::Reindex => {
                if let State::Open(p) = &self.state {
                    let dir = p.dir.clone();
                    let _ = std::fs::remove_file(&p.index_path);
                    self.open(dir);
                }
            }
            Action::Close => self.state = State::Start { error: None },
            Action::Settings => {
                if let State::Open(p) = &mut self.state {
                    p.settings_open = true;
                }
            }
        }
    }

    fn start_screen(&mut self, ui: &mut egui::Ui, error: Option<String>) {
        let mut chosen: Option<PathBuf> = None;
        let mut action = None;
        if ui.input_mut(|i| i.consume_shortcut(&OPEN)) {
            action = Some(Action::OpenFolder);
        }
        egui::Panel::top("top").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                if let Some(a) = self.file_menu(ui, false) {
                    action = Some(a);
                }
            });
        });
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
        } else if let Some(a) = action {
            self.act(a);
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
            State::Indexing { dir, visual, settings, packs, rx, mut stage, started } => {
                let mut done = None;
                while let Ok(msg) = rx.try_recv() {
                    match msg {
                        Msg::Progress(p) => stage = format!("{p:?}"),
                        Msg::Done(r) => done = Some(r),
                    }
                }
                match done {
                    Some(Ok(path)) => {
                        self.state = match Project::open(dir, &path, visual, *settings, packs) {
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
                        self.state = State::Indexing { dir, visual, settings, packs, rx, stage, started };
                    }
                }
            }
            State::Open(mut project) => {
                let menu = egui::Panel::top("top").show(ui, |ui| egui::MenuBar::new().ui(ui, |ui| {
                    let file = self.file_menu(ui, true);
                    project.view_menu(ui);
                    project.review_menu(ui);
                    ui.menu_button("Help", |ui| {
                        if ui.add(egui::Button::new("Shortcuts").shortcut_text("? / F1")).clicked() {
                            project.shortcuts_open = true;
                        }
                    });
                    project.mode_switch(ui);
                    file
                }).inner).inner;
                let action = project.ui(ui).or(menu);
                if let Some(shot) = &mut self.shot {
                    match shot.step(project.target.is_some() || project.review_job.is_some()) {
                        ShotStep::Setup => project.apply_plan(&shot.plan),
                        ShotStep::Capture => shot.request(ui.ctx()),
                        ShotStep::Close => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
                        ShotStep::Wait => {}
                    }
                    ui.ctx().request_repaint();
                }
                self.state = State::Open(project);
                if let Some(a) = action {
                    self.act(a);
                }
            }
        }
    }
}

fn review_dim(rv: &ReviewState, scene: &mut Scene) {
    let quiet = |n: NodeId| {
        let c = rv.review.change(n);
        c.status == Status::Unchanged && c.inside == Default::default()
    };
    for b in scene.boxes.iter_mut().filter(|b| matches!(b.kind, NodeKind::Module | NodeKind::Package | NodeKind::Type | NodeKind::Member)) {
        b.dimmed |= quiet(b.node);
    }
    for e in scene.edges.iter_mut() {
        e.dimmed |= !rv.new_arrows.contains(&(e.a, e.b));
    }
}

struct ReviewBar {
    base: Option<String>,
    target: Option<String>,
    by_commit: bool,
    steps: Vec<CommitStep>,
    step: usize,
    branches: Vec<String>,
    current: Option<String>,
    other: Option<(bool, String)>,
}

struct ReviewDone {
    label: String,
    review: Review,
    analysis: Analysis,
    dir: PathBuf,
    index: PathBuf,
    rebuilt: bool,
    version: Option<String>,
}

fn short_rev(rev: &str) -> String {
    let hex = rev.trim_end_matches('^');
    if hex.len() >= 20 && hex.chars().all(|c| c.is_ascii_hexdigit()) { format!("{}{}", &hex[..7], &rev[hex.len()..]) } else { rev.to_string() }
}

struct ReviewState {
    label: String,
    review: Review,
    items: Vec<ReviewItem>,
    cursor: Option<usize>,
    new_arrows: std::collections::HashSet<(NodeId, NodeId)>,
    upward: std::collections::HashSet<(NodeId, NodeId)>,
    marks: ReviewMarks,
    marks_path: PathBuf,
    states: HashMap<NodeId, MarkState>,
    hide_reviewed: bool,
    by_package: bool,
}

impl ReviewState {
    fn visible(&self) -> Vec<usize> {
        (0..self.items.len()).filter(|&i| !self.hide_reviewed || self.marks.state(&self.items[i]) != MarkState::Reviewed).collect()
    }

    fn reviewed(&self) -> usize {
        self.items.iter().filter(|i| self.marks.state(i) == MarkState::Reviewed).count()
    }

    fn refresh(&mut self) {
        self.states = self.items.iter().filter_map(|i| Some((i.node?, self.marks.state(i)))).collect();
    }

    fn reorder(&mut self) {
        let current = self.cursor.map(|c| self.items[c].key.clone());
        self.items.sort_by(|a, b| (&a.group, &a.label).cmp(&(&b.group, &b.label)));
        if !self.by_package {
            by_risk(&mut self.items);
        }
        self.cursor = current.and_then(|k| self.items.iter().position(|i| i.key == k));
    }

    fn toggle(&mut self, i: usize) -> Option<String> {
        self.marks.toggle(&self.items[i]);
        self.refresh();
        self.marks.save(&self.marks_path).err().map(|e| format!("saving review marks: {e}"))
    }
}

enum Action {
    OpenFolder,
    OpenRecent(PathBuf),
    Reindex,
    Settings,
    Close,
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
    search_cursor: usize,
    hovered: Option<NodeId>,
    shortcuts_open: bool,
    settings: SettingsStore,
    packs: Vec<String>,
    settings_open: bool,
    form: SettingsForm,
    app_counts: HashMap<NodeId, usize>,
    entry_nodes: std::collections::HashSet<NodeId>,
    goto_line: Option<(NodeId, u32)>,
    totals: Totals,
    scene_ms: f32,
    last_frame: Instant,
    source: std::cell::RefCell<SourceCache>,
    file_scroll: HashMap<NodeId, f32>,
    file_hscroll: HashMap<NodeId, f32>,
    scripted: bool,
    scroll_drag: Option<NodeId>,
    review: Option<ReviewState>,
    review_job: Option<(String, Receiver<Result<ReviewDone, String>>)>,
    review_bar: Option<ReviewBar>,
    home: Option<(PathBuf, PathBuf)>,
    version: Option<String>,
    git: bool,
    analysis: Option<Arc<Analysis>>,
    analysis_job: Option<Receiver<Analysis>>,
    review_error: Option<String>,
    details_in: Vec<NodeId>,
    pointer: Option<Pos2>,
}

fn chevron(painter: &egui::Painter, center: Pos2, up: bool) {
    let dy = if up { -2.0 } else { 2.0 };
    let pts = vec![center + egui::vec2(-4.0, -dy), center + egui::vec2(0.0, dy), center + egui::vec2(4.0, -dy)];
    painter.add(Shape::line(pts, Stroke::new(1.5, style::MUTED)));
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
const LEGEND_ROW: f32 = 14.0;

fn review_badge(c: &Change, open: bool) -> Option<(String, Color32)> {
    let i = c.inside;
    if (open || c.status != Status::New) && i != Counts::default() {
        let parts: Vec<String> = [(i.new, "new"), (i.changed, "changed"), (i.removed, "removed")].iter().filter(|(n, _)| *n > 0).map(|(n, w)| format!("{n} {w}")).collect();
        let color = if i.new > 0 { style::REACH } else { style::EDGE_SIDEWAYS };
        return Some((parts.join(" · "), color));
    }
    match c.status {
        Status::New => Some(("NEW".into(), style::REACH)),
        Status::Changed => Some((format!("+{} −{}", c.added, c.removed), style::EDGE_SIDEWAYS)),
        Status::Unchanged => None,
    }
}

fn unused_short(u: Usage) -> &'static str {
    match u {
        Usage::Unused => "unused",
        Usage::NoCallerInProject => "no caller",
        Usage::OnlyTests => "tests only",
        Usage::Used => "",
    }
}

fn row_delta(c: &Change) -> Option<(String, Color32)> {
    match c.status {
        Status::New => Some(("new".into(), style::REACH)),
        Status::Changed => {
            let mut d = format!("+{} −{}", c.added, c.removed);
            if let Some((before, after)) = c.complexity.filter(|(b, a)| a > b) {
                d.push_str(&format!(" · complexity {before} to {after}"));
            }
            Some((d, style::EDGE_SIDEWAYS))
        }
        Status::Unchanged => None,
    }
}

fn chip_rect(r: Rect) -> Rect {
    Rect::new(r.right() - 100.0, r.y + 6.0, 92.0, 18.0)
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
    fn open(dir: PathBuf, path: &Path, visual: VisualRules, settings: SettingsStore, packs: Vec<String>) -> Result<Project, String> {
        let git = is_git(&dir);
        let source = std::cell::RefCell::new(SourceCache::new(&dir));
        let file = ReliefFile::open(path).map_err(|e| format!("{e}"))?;
        let g = file.graph();
        let entry_nodes = g.entries().iter().map(|e| e.node.to_native()).collect();
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
        let cache = LayoutCache::new(visual.frames.clone(), layout_options(settings.settings(), &visual));
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
            search_cursor: 0,
            hovered: None,
            shortcuts_open: false,
            settings,
            packs,
            settings_open: false,
            form: SettingsForm::default(),
            app_counts,
            entry_nodes,
            goto_line: None,
            totals,
            scene_ms: 0.0,
            last_frame: Instant::now(),
            source,
            file_scroll: HashMap::new(),
            file_hscroll: HashMap::new(),
            scripted: false,
            scroll_drag: None,
            review: None,
            review_job: None,
            analysis: None,
            analysis_job: None,
            review_bar: None,
            home: None,
            version: None,
            git,
            review_error: None,
            details_in: Vec::new(),
            pointer: None,
        })
    }

    fn fly_to(&mut self, node: NodeId) {
        let g = self.file.graph();
        if let Some(r) = world_rect(g, &mut self.cache, node) {
            self.target = Some(Camera::fit(r, self.view));
        }
    }

    fn apply_plan(&mut self, plan: &ShotPlan) {
        self.scripted = true;
        let entry = plan.entry.as_ref().and_then(|label| {
            let g = self.file.graph();
            (0..g.entries().len()).find(|&i| g.entry_label(i) == label)
        });
        if let Some(i) = entry {
            self.select_entry(i);
        }
        let focus = plan.focus.as_ref().and_then(|name| self.search.query(self.file.graph(), name, 1).first().copied());
        if let Some(n) = focus {
            self.set_focus(n, None, false, true);
            self.fly_to(n);
            if let (Some(z), Some(w)) = (plan.zoom, world_rect(self.file.graph(), &mut self.cache, n)) {
                if let Some(t) = self.target.as_mut() {
                    t.zoom *= z;
                    let half = [self.view.w as f64 / 2.0 / t.zoom, self.view.h as f64 / 2.0 / t.zoom];
                    t.center = [w.x + half[0], w.y + half[1]];
                }
            }
        }
        self.settings_open |= plan.settings;
        self.shortcuts_open |= plan.shortcuts;
        if plan.review.is_some() || plan.review_target.is_some() || plan.review_by_commit {
            self.enter_review(plan.review.clone().filter(|r| r != "main"), plan.review_target.clone());
        }
        if plan.review_by_commit {
            if let Some(bar) = self.review_bar.as_mut() {
                bar.by_commit = true;
            }
            self.refresh_steps();
            self.compare_now();
        }
    }

    fn set_display(&mut self, key: &SettingKey, on: bool) {
        toggle(&mut self.settings, key, on);
        self.apply_display();
    }

    fn apply_display(&mut self) {
        let mut opts = layout_options(self.settings.settings(), &self.visual);
        opts.details_in = self.details_in.clone();
        self.cache.set_options(opts);
        self.form.error = self.settings.save_view().err().map(|e| format!("{e:#}"));
    }

    fn view_menu(&mut self, ui: &mut egui::Ui) {
        let jump = ui.ctx().format_shortcut(&JUMP);
        let mut changed = None;
        ui.menu_button("View", |ui| {
            let items = view_items(self.settings.settings());
            for (i, item) in items.iter().enumerate() {
                let filter = |k: &SettingKey| matches!(k, SettingKey::FilterShown(_));
                if (filter(&item.key) && !filter(&items[i - 1].key)) || item.key == SettingKey::View(View::Legend) {
                    ui.separator();
                }
                let mut on = item.checked;
                if ui.checkbox(&mut on, &item.label).clicked() {
                    changed = Some((item.key.clone(), on));
                }
            }
            ui.separator();
            if ui.add(egui::Button::new("Jump to…").shortcut_text(jump)).clicked() {
                self.search_open = true;
            }
        });
        if let Some((key, on)) = changed {
            self.set_display(&key, on);
        }
    }

    fn review_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Review", |ui| {
            let start = ui.add_enabled(self.git && self.review_bar.is_none(), egui::Button::new("Start review"));
            if start.on_disabled_hover_text(self.review_unavailable()).clicked() {
                self.enter_review(None, None);
            }
            if self.review_bar.is_some() && ui.button("Exit review").clicked() {
                self.exit_review();
            }
        });
    }

    fn review_unavailable(&self) -> &'static str {
        if self.git { "Already reviewing" } else { "Review compares git versions: this folder is not in a git repository" }
    }

    fn mode_switch(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        let reviewing = self.review_bar.is_some();
        if ui.selectable_label(!reviewing, "Overview").on_hover_text("The checkout as it is").clicked() && reviewing {
            self.exit_review();
        }
        let review = ui.add_enabled(self.git, egui::Button::selectable(reviewing, "Review")).on_hover_text("What changed between two versions").on_disabled_hover_text(self.review_unavailable());
        if review.clicked() && !reviewing {
            self.enter_review(None, None);
        }
    }

    fn origin(&self) -> (PathBuf, PathBuf) {
        self.home.clone().unwrap_or_else(|| (self.dir.clone(), self.index_path.clone()))
    }

    fn enter_review(&mut self, base: Option<String>, target: Option<String>) {
        let (dir, _) = self.origin();
        self.review_bar = Some(ReviewBar { base, target, by_commit: false, steps: Vec::new(), step: 0, branches: branches(&dir), current: current_branch(&dir), other: None });
        self.compare_now();
    }

    fn exit_review(&mut self) {
        self.review_bar = None;
        self.review = None;
        self.review_job = None;
        self.review_error = None;
        if let Some((dir, index)) = self.home.clone() {
            if let Err(e) = self.show_version(dir, index) {
                self.review_error = Some(e);
            }
        }
    }

    fn refresh_steps(&mut self) {
        let (dir, _) = self.origin();
        let Some(bar) = &mut self.review_bar else { return };
        let target = bar.target.clone().unwrap_or_else(|| "HEAD".into());
        let base = bar.base.clone().or_else(|| base_for(&dir, bar.target.as_deref()).ok());
        bar.steps = base.and_then(|b| commits(&dir, &b, &target).ok()).unwrap_or_default().into_iter().map(CommitStep::Commit).collect();
        if bar.target.is_none() && has_uncommitted(&dir) {
            bar.steps.push(CommitStep::Uncommitted);
        }
        bar.step = 0;
    }

    fn compare_now(&mut self) {
        let Some(bar) = &self.review_bar else { return };
        let (base, target) = match bar.steps.get(bar.step).filter(|_| bar.by_commit) {
            Some(step) => step.range(),
            None => (bar.base.clone(), bar.target.clone()),
        };
        self.start_review(base, target);
    }

    fn start_review(&mut self, base: Option<String>, target: Option<String>) {
        let (dir, index) = self.origin();
        let cfg = self.settings.config();
        let cache = srcrelief_index::cache_path(&dir).parent().map(Path::to_path_buf).unwrap_or_else(std::env::temp_dir);
        let shown = format!("{} to {}", short_rev(base.as_deref().unwrap_or("main")), target.as_deref().map_or("working copy".into(), short_rev));
        let (tx, rx) = channel();
        let label = shown.clone();
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<ReviewDone> {
                let base_rev = match &base {
                    Some(r) => r.clone(),
                    None => base_for(&dir, target.as_deref())?,
                };
                let b = index_revision(&dir, &base_rev, &cfg, &cache, &|_| {})?;
                let (target_dir, target_index, rebuilt) = match &target {
                    Some(t) => {
                        let r = index_revision(&dir, t, &cfg, &cache, &|_| {})?;
                        (r.dir, r.index, false)
                    }
                    None => {
                        let stale = needs_reindex(&dir, &index, &cfg, &project_packs(&dir, &cfg)?);
                        if stale {
                            index_project(&dir, &cfg, &|_| {})?;
                        }
                        (dir.clone(), index.clone(), stale)
                    }
                };
                let (bf, tf) = (ReliefFile::open(&b.index)?, ReliefFile::open(&target_index)?);
                let review = compare(bf.graph(), &b.dir, tf.graph(), &target_dir);
                let analysis = analyse(tf.graph(), &target_dir);
                Ok(ReviewDone { label: format!("{label} @ {}", &b.sha[..7]), review, analysis, dir: target_dir, index: target_index, rebuilt, version: target.as_deref().map(short_rev) })
            })();
            let _ = tx.send(result.map_err(|e| format!("{e:#}")));
        });
        self.review_error = None;
        self.review_job = Some((shown, rx));
    }

    fn show_version(&mut self, dir: PathBuf, index: PathBuf) -> Result<(), String> {
        let origin = self.origin();
        let fresh = Project::open(dir.clone(), &index, self.visual.clone(), self.settings.clone(), self.packs.clone())?;
        let mut old = std::mem::replace(self, fresh);
        self.home = (dir != origin.0).then_some(origin);
        self.review_bar = old.review_bar.take();
        self.review_job = old.review_job.take();
        self.cam = old.cam;
        self.view = old.view;
        self.needs_fit = false;
        self.scripted = old.scripted;
        Ok(())
    }

    fn poll_review(&mut self, ctx: &egui::Context) {
        let Some((_, rx)) = &self.review_job else { return };
        match rx.try_recv() {
            Ok(Ok(done)) => {
                self.review_job = None;
                if done.rebuilt || (done.dir.as_path(), done.index.as_path()) != (self.dir.as_path(), self.index_path.as_path()) {
                    if let Err(e) = self.show_version(done.dir, done.index) {
                        self.review_error = Some(e);
                        return;
                    }
                }
                self.version = done.version;
                let g = self.file.graph();
                let items = review_items(g, &done.review, &done.analysis);
                let new_arrows = done.review.new_arrows.iter().copied().collect();
                let upward = done
                    .review
                    .new_arrows
                    .iter()
                    .copied()
                    .filter(|&(a, b)| g.parent(a).is_some_and(|p| self.cache.get_or_compute(g, p).edges.iter().any(|e| e.a == a && e.b == b && e.dir == Direction::Up)))
                    .collect();
                let marks_path = srcrelief_index::cache_path(&self.origin().0).with_extension("marks");
                let marks = ReviewMarks::load(&marks_path);
                let (hide_reviewed, by_package) = self.review.as_ref().map_or((false, false), |r| (r.hide_reviewed, r.by_package));
                let mut rv = ReviewState { label: done.label, review: done.review, items, cursor: None, new_arrows, upward, marks, marks_path, states: HashMap::new(), hide_reviewed, by_package };
                rv.reorder();
                rv.refresh();
                self.analysis = Some(Arc::new(done.analysis));
                self.review = Some(rv);
            }
            Ok(Err(e)) => {
                self.review_error = Some(e);
                self.review_job = None;
            }
            Err(_) => ctx.request_repaint_after(std::time::Duration::from_millis(200)),
        }
    }

    fn review_bar(&mut self, ui: &mut egui::Ui) {
        let Some(bar) = &mut self.review_bar else { return };
        let mut changed = false;
        let mut steps_changed = false;
        let mut rerun = false;
        let mut exit = false;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("REVIEW").strong().color(style::BG).background_color(style::ACCENT));
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Base").color(style::MUTED));
            let base_text = bar.base.clone().unwrap_or_else(|| "where it left main".into());
            egui::ComboBox::from_id_salt("review_base").selected_text(base_text).width(170.0).show_ui(ui, |ui| {
                changed |= ui.selectable_value(&mut bar.base, None, "where it left main").clicked();
                for b in &bar.branches {
                    changed |= ui.selectable_value(&mut bar.base, Some(b.clone()), b.as_str()).clicked();
                }
                if ui.button("Other commit or tag…").clicked() {
                    bar.other = Some((false, String::new()));
                }
            });
            ui.label(egui::RichText::new("Target").color(style::MUTED));
            let working = format!("Working copy{}", bar.current.as_ref().map(|c| format!(" · {c}")).unwrap_or_default());
            let target_text = bar.target.clone().unwrap_or_else(|| working.clone());
            egui::ComboBox::from_id_salt("review_target").selected_text(target_text).width(190.0).show_ui(ui, |ui| {
                changed |= ui.selectable_value(&mut bar.target, None, working.as_str()).clicked();
                for b in &bar.branches {
                    changed |= ui.selectable_value(&mut bar.target, Some(b.clone()), b.as_str()).clicked();
                }
                if ui.button("Other commit or tag…").clicked() {
                    bar.other = Some((true, String::new()));
                }
            });
            if let Some((for_target, text)) = &mut bar.other {
                let edit = ui.add(egui::TextEdit::singleline(text).hint_text("tag, commit, origin/x…").desired_width(150.0));
                edit.request_focus();
                if ui.input(|i| i.key_pressed(Key::Enter)) && !text.trim().is_empty() {
                    let rev = Some(text.trim().to_string());
                    if *for_target { bar.target = rev } else { bar.base = rev }
                    bar.other = None;
                    changed = true;
                } else if ui.input(|i| i.key_pressed(Key::Escape)) {
                    bar.other = None;
                }
            }
            ui.separator();
            if ui.selectable_label(!bar.by_commit, "Whole range").clicked() && bar.by_commit {
                bar.by_commit = false;
                rerun = true;
            }
            if ui.selectable_label(bar.by_commit, "Commit by commit").clicked() && !bar.by_commit {
                bar.by_commit = true;
                steps_changed = true;
            }
            if bar.by_commit {
                let n = bar.steps.len();
                if n == 0 {
                    ui.label(egui::RichText::new("no commits in this range").color(style::MUTED));
                } else {
                    if ui.add_enabled(bar.step > 0, egui::Button::new("<")).on_hover_text("Previous commit ([)").clicked() {
                        bar.step -= 1;
                        rerun = true;
                    }
                    let step = &bar.steps[bar.step];
                    let sha = match step {
                        CommitStep::Commit(c) => c.sha[..7].to_string(),
                        CommitStep::Uncommitted => String::new(),
                    };
                    ui.label(egui::RichText::new(format!("{}/{n}", bar.step + 1)).strong());
                    ui.label(egui::RichText::new(sha).monospace().color(style::MUTED));
                    ui.label(fit_label(step.label(), 260.0, 13.0));
                    if ui.add_enabled(bar.step + 1 < n, egui::Button::new(">")).on_hover_text("Next commit (])").clicked() {
                        bar.step += 1;
                        rerun = true;
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                exit = ui.button("Exit review").clicked();
            });
        });
        if exit {
            self.exit_review();
            return;
        }
        if (changed || steps_changed) && self.review_bar.as_ref().is_some_and(|b| b.by_commit) {
            self.refresh_steps();
        }
        if changed || steps_changed || rerun {
            self.compare_now();
        }
    }

    fn poll_analysis(&mut self, ctx: &egui::Context) {
        if self.analysis.is_some() || !self.settings.settings().view(View::Unused).value {
            return;
        }
        let Some(rx) = &self.analysis_job else {
            let (tx, rx) = channel();
            let (dir, index) = (self.dir.clone(), self.index_path.clone());
            std::thread::spawn(move || {
                if let Ok(f) = ReliefFile::open(&index) {
                    let _ = tx.send(analyse(f.graph(), &dir));
                }
            });
            self.analysis_job = Some(rx);
            return;
        };
        match rx.try_recv() {
            Ok(a) => {
                self.analysis = Some(Arc::new(a));
                self.analysis_job = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
            Err(_) => self.analysis_job = None,
        }
    }

    fn unused(&self, n: NodeId) -> Option<Usage> {
        let a = self.analysis.as_ref().filter(|_| self.settings.settings().view(View::Unused).value)?;
        a.usage.get(n as usize).copied().filter(|&u| u != Usage::Used)
    }

    fn goto_review_item(&mut self, i: usize) {
        let Some(rv) = &mut self.review else { return };
        rv.cursor = Some(i);
        let item = &rv.items[i];
        if let Some(n) = item.node.or(item.parent) {
            self.set_focus(n, None, true, true);
        }
    }

    fn keyboard(&mut self, ui: &egui::Ui) {
        let plain = |k: Key| ui.input(|i| i.key_pressed(k) && !i.modifiers.alt && !i.modifiers.command && !i.modifiers.ctrl);
        let typed = |c: &str| ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Text(t) if t == c)));
        if typed("?") || plain(Key::F1) {
            self.shortcuts_open = true;
            return;
        }
        if let Some(rv) = &mut self.review {
            let forward = plain(Key::N);
            if forward || plain(Key::P) {
                let visible = rv.visible();
                let at = rv.cursor.and_then(|c| visible.iter().position(|&v| v == c));
                if let Some(i) = review_step(visible.len(), at, forward) {
                    self.goto_review_item(visible[i]);
                }
                return;
            }
            let (back, ahead) = (typed("["), typed("]"));
            if back || ahead {
                if let Some(bar) = self.review_bar.as_mut().filter(|b| b.by_commit && !b.steps.is_empty()) {
                    let next = if ahead { (bar.step + 1).min(bar.steps.len() - 1) } else { bar.step.saturating_sub(1) };
                    if next != bar.step {
                        bar.step = next;
                        self.compare_now();
                    }
                }
                return;
            }
            if plain(Key::R) {
                if let Some(c) = rv.cursor {
                    self.review_error = rv.toggle(c);
                }
                return;
            }
        }
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let page = pointer.and_then(|p| self.file_page_at(p)).or(self.selected.filter(|&s| self.scene.boxes.iter().any(|b| b.node == s && b.source)));
        if let Some((node, (line_h, total, visible))) = page.and_then(|n| Some((n, self.file_metrics(n)?))) {
            let lines = [(Key::PageDown, -(visible as f32 - 2.0)), (Key::PageUp, visible as f32 - 2.0), (Key::End, -(total as f32)), (Key::Home, total as f32)];
            if let Some(&(_, by)) = lines.iter().find(|(k, _)| plain(*k)) {
                if let Some(v) = self.scrolled(node, by * line_h) {
                    self.file_scroll.insert(node, v);
                }
                return;
            }
        }
        let steps = [(Key::ArrowLeft, Step::Left), (Key::ArrowRight, Step::Right), (Key::ArrowUp, Step::Up), (Key::ArrowDown, Step::Down)];
        if let Some(&(_, step)) = steps.iter().find(|(k, _)| plain(*k)) {
            self.step_selection(step);
        } else if plain(Key::Enter) {
            if let Some(n) = self.selected {
                self.fly_to(n);
            }
        } else if plain(Key::Backspace) {
            self.go_up();
        } else if plain(Key::F) {
            self.needs_fit = true;
        } else if plain(Key::Plus) || plain(Key::Equals) || typed("+") {
            self.zoom_by(1.25);
        } else if plain(Key::Minus) || typed("-") {
            self.zoom_by(0.8);
        } else if self.focus.is_some() {
            let modes = [(Key::Num1, FocusMode::Neighbours), (Key::Num2, FocusMode::Reaches), (Key::Num3, FocusMode::UsedBy)];
            if let Some(&(_, mode)) = modes.iter().find(|(k, _)| plain(*k)) {
                self.focus_mode = mode;
                self.refocus();
            }
        }
    }

    fn zoom_by(&mut self, f: f64) {
        let c = self.view.center();
        self.cam.zoom_at(c, f, self.view);
        self.target = None;
    }

    fn go_up(&mut self) {
        let g = self.file.graph();
        match self.selected.and_then(|n| g.parent(n)).filter(|&p| p != g.root()) {
            Some(p) => self.set_focus(p, None, true, true),
            None => {
                self.clear_focus();
                self.selected = None;
                self.needs_fit = true;
            }
        }
    }

    fn step_selection(&mut self, step: Step) {
        let g = self.file.graph();
        let center = self.view.center();
        let Some(sel) = self.selected.filter(|&s| self.scene.boxes.iter().any(|b| b.node == s)) else {
            if let Some(n) = self.scene.box_at(center).filter(|&n| n != g.root()) {
                self.set_focus(n, None, false, true);
            }
            return;
        };
        let parent = g.parent(sel);
        let siblings: Vec<(NodeId, Rect)> = self.scene.boxes.iter().filter(|b| g.parent(b.node) == parent).map(|b| (b.node, b.rect)).collect();
        let from = siblings.iter().find(|(n, _)| *n == sel).map(|(_, r)| *r).unwrap();
        let Some(next) = nearest_in(step, sel, from, &siblings) else { return };
        self.set_focus(next, None, false, true);
        let g = self.file.graph();
        let r = siblings.iter().find(|(n, _)| *n == next).map(|(_, r)| *r).unwrap();
        if !self.view.contains_rect(&r) {
            if let Some(w) = world_rect(g, &mut self.cache, next) {
                self.target = Some(Camera { center: w.center(), zoom: self.cam.zoom });
            }
        }
    }

    fn select_entry(&mut self, i: usize) {
        let node = self.file.graph().entries()[i].node.to_native();
        self.set_focus(node, Some(i), true, true);
    }

    fn set_focus(&mut self, node: NodeId, entry: Option<usize>, fly: bool, record: bool) {
        let g = self.file.graph();
        let mut opts = self.cache.options().clone();
        if opts.reveal(g, node) {
            self.details_in = opts.details_in.clone();
            self.cache.set_options(opts);
        }
        self.focus = Some(focus(g, &self.usage, node, entry, self.focus_mode));
        self.selected = Some(node);
        self.selected_entry = entry;
        if record {
            self.history.push(node);
        }
        if fly {
            let opts = self.cache.options();
            let leaf = !g.children(node).any(|c| !opts.hides(g, c));
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
        self.poll_review(ui.ctx());
        self.poll_analysis(ui.ctx());
        let (jump, open, settings) = ui.input_mut(|i| (i.consume_shortcut(&JUMP), i.consume_shortcut(&OPEN), i.consume_shortcut(&SETTINGS)));
        self.search_open |= jump;
        self.settings_open |= settings;
        if open {
            action = Some(Action::OpenFolder);
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            if self.search_open || self.shortcuts_open {
                self.search_open = false;
                self.shortcuts_open = false;
            } else {
                self.clear_focus();
                self.selected = None;
            }
        }
        if ui.input(|i| i.modifiers.alt && i.key_pressed(Key::ArrowLeft)) {
            self.go_back();
        } else if !self.search_open && !ui.ctx().egui_wants_keyboard_input() {
            self.keyboard(ui);
        }
        if self.shortcuts_open {
            egui::Window::new("Shortcuts").open(&mut self.shortcuts_open).collapsible(false).resizable(false).anchor(Align2::CENTER_TOP, [0.0, 60.0]).show(ui.ctx(), |ui| {
                ui.horizontal_top(|ui| {
                    for column in CHEATSHEET.chunks(2) {
                        ui.vertical(|ui| {
                            for (section, rows) in column {
                                ui.label(egui::RichText::new(*section).strong().color(style::TEXT));
                                egui::Grid::new(*section).num_columns(2).spacing([14.0, 4.0]).show(ui, |ui| {
                                    for (keys, what) in rows.iter() {
                                        ui.label(egui::RichText::new(*keys).monospace().color(style::ACCENT));
                                        ui.label(egui::RichText::new(*what).color(style::MUTED));
                                        ui.end_row();
                                    }
                                });
                                ui.add_space(10.0);
                            }
                        });
                        ui.add_space(18.0);
                    }
                });
            });
        }
        if self.review_bar.is_some() {
            egui::Panel::top("review_bar").frame(egui::Frame::side_top_panel(ui.style()).fill(style::tint(style::BAR, style::ACCENT, 0.22))).show(ui, |ui| self.review_bar(ui));
        }
        egui::Panel::top("breadcrumb").show(ui, |ui| ui.horizontal(|ui| self.breadcrumb(ui)));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("entries").resizable(true).default_size(270.0).show(ui, |ui| self.entries_panel(ui));
        egui::Panel::right("inspector").resizable(true).default_size(290.0).show(ui, |ui| self.inspector(ui));
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(style::BG)).show(ui, |ui| {
            self.canvas(ui);
            if self.review_bar.is_some() {
                ui.painter().rect_stroke(ui.max_rect(), 0.0, Stroke::new(3.0, style::ACCENT), StrokeKind::Inside);
            }
        });
        if self.search_open {
            self.search_window(ui.ctx());
        }
        if self.settings_open {
            let out = settings_window::show(ui.ctx(), &mut self.settings_open, &mut self.settings, &self.packs, &mut self.form);
            if out.view_changed {
                self.apply_display();
            }
            if out.apply {
                match self.settings.apply() {
                    Ok(()) => action = Some(Action::Reindex),
                    Err(e) => self.form.error = Some(format!("{e:#}")),
                }
            }
        }
        action
    }

    pub(crate) fn breadcrumb(&mut self, ui: &mut egui::Ui) {
        let g = self.file.graph();
        let deepest = self.scene.deepest_open_at(self.view.center()).unwrap_or(g.root());
        let mut fly = None;
        let root = match (&self.home, &self.version) {
            (Some((dir, _)), Some(v)) => format!("{} @ {v}", dir.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned())),
            _ => g.name(g.root()).to_string(),
        };
        for (i, n) in g.path(deepest).into_iter().enumerate() {
            if i > 0 {
                ui.label(egui::RichText::new("/").color(style::MUTED));
            }
            if ui.link(if i == 0 { root.as_str() } else { g.name(n) }).clicked() {
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
            if let Some((shown, _)) = &self.review_job {
                ui.spinner();
                ui.label(egui::RichText::new(format!("Comparing with {shown}…")).color(style::ACCENT));
                ui.separator();
            }
            if let Some(e) = &self.review_error {
                ui.colored_label(style::EDGE_UP, format!("Review failed: {e}"));
                ui.separator();
            }
            if let Some(rv) = &self.review {
                let t = rv.review.totals();
                let removed = if t.removed > 0 { format!(" · {} removed", t.removed) } else { String::new() };
                ui.label(egui::RichText::new(format!("{} · {} new · {} changed{removed} · {} / {} reviewed", rv.label, t.new, t.changed, rv.reviewed(), rv.items.len())).strong().color(style::ACCENT));
                ui.separator();
            }
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
            ui.label(egui::RichText::new(format!("{:.1} ms · {} boxes", self.scene_ms, self.scene.stats.drawn)).color(style::MUTED));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.link(egui::RichText::new("? shortcuts").color(style::MUTED)).clicked() {
                    self.shortcuts_open = true;
                }
            });
        });
    }

    fn entries_panel(&mut self, ui: &mut egui::Ui) {
        let mut select: Option<usize> = None;
        let mut fly: Option<NodeId> = None;
        let mut review_pick: Option<usize> = None;
        let mut review_toggle: Option<usize> = None;
        let mut review_switches: Option<(bool, bool)> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            let g = self.file.graph();
            if let Some(rv) = &self.review {
                let visible = rv.visible();
                ui.label(egui::RichText::new(format!("CHANGES · {} / {} reviewed", rv.reviewed(), rv.items.len())).small().color(style::ACCENT))
                    .on_hover_text("N / P: next / previous change · R: mark the current one reviewed");
                let (mut hide, mut grouped) = (rv.hide_reviewed, rv.by_package);
                ui.horizontal(|ui| {
                    ui.checkbox(&mut hide, egui::RichText::new("Hide reviewed").small());
                    ui.checkbox(&mut grouped, egui::RichText::new("By package").small()).on_hover_text("Off: riskiest first");
                });
                if (hide, grouped) != (rv.hide_reviewed, rv.by_package) {
                    review_switches = Some((hide, grouped));
                }
                let mut row = |ui: &mut egui::Ui, i: usize, with_group: bool| {
                    let item = &rv.items[i];
                    let state = rv.marks.state(item);
                    let color = match (state, item.kind) {
                        (MarkState::Reviewed, _) => style::MUTED,
                        (_, ItemKind::New) => style::REACH,
                        (_, ItemKind::Changed) => style::EDGE_SIDEWAYS,
                        (_, ItemKind::Removed) => style::EDGE_UP,
                    };
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let mut done = state == MarkState::Reviewed;
                        if ui.checkbox(&mut done, "").on_hover_text("Reviewed (R)").changed() {
                            review_toggle = Some(i);
                        }
                        let hover = if with_group { format!("{}\n{}", item.group, item.detail) } else { item.detail.clone() };
                        if ui.selectable_label(rv.cursor == Some(i), egui::RichText::new(&item.label).color(color)).on_hover_text(hover).clicked() {
                            review_pick = Some(i);
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.add_space(22.0);
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let shown = if item.risks.iter().any(|r| matches!(r, Risk::Complexity(..))) { item.detail.split(" · ").next().unwrap_or("") } else { item.detail.as_str() };
                        ui.label(egui::RichText::new(shown).small().color(style::MUTED));
                        if state == MarkState::ChangedSince {
                            ui.label(egui::RichText::new("changed since review").small().color(style::ACCENT));
                        }
                        for r in &item.risks {
                            ui.label(egui::RichText::new(r.label()).small().color(if *r == Risk::Untested { style::MUTED } else { style::EDGE_SIDEWAYS }));
                        }
                    });
                };
                if rv.by_package {
                    let mut groups: Vec<(&str, Vec<usize>)> = Vec::new();
                    for &i in &visible {
                        let item = &rv.items[i];
                        match groups.last_mut() {
                            Some((g0, v)) if *g0 == item.group.as_str() => v.push(i),
                            _ => groups.push((item.group.as_str(), vec![i])),
                        }
                    }
                    for (group, idx) in groups {
                        egui::CollapsingHeader::new(format!("{group} ({})", idx.len())).id_salt(("review", group)).default_open(true).show(ui, |ui| {
                            for i in idx {
                                row(ui, i, false);
                            }
                        });
                    }
                } else {
                    for &i in &visible {
                        row(ui, i, true);
                    }
                }
                if !rv.review.new_arrows.is_empty() {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(format!("NEW DEPENDENCIES ({})", rv.review.new_arrows.len())).small().color(style::ACCENT));
                    let mut arrows = rv.review.new_arrows.clone();
                    arrows.sort_by_key(|a| !rv.upward.contains(a));
                    for (a, b) in arrows {
                        let up = rv.upward.contains(&(a, b));
                        ui.horizontal_wrapped(|ui| {
                            if ui.selectable_label(false, egui::RichText::new(format!("{} uses {}", g.name(a), g.name(b))).color(style::REACH)).on_hover_text(format!("{} uses {}", full_path(g, a), full_path(g, b))).clicked() {
                                fly = Some(a);
                            }
                            if up {
                                ui.label(egui::RichText::new("upward dependency").small().color(style::EDGE_UP));
                            }
                        });
                    }
                }
                if !rv.review.removed_arrows.is_empty() {
                    ui.label(egui::RichText::new(format!("REMOVED DEPENDENCIES ({})", rv.review.removed_arrows.len())).small().color(style::MUTED));
                    for (a, b) in &rv.review.removed_arrows {
                        ui.label(egui::RichText::new(format!("{} uses {}", a.rsplit('/').next().unwrap_or(a), b.rsplit('/').next().unwrap_or(b))).color(style::MUTED)).on_hover_text(format!("{a} uses {b}"));
                    }
                }
                ui.add_space(12.0);
            } else if let Some(a) = self.analysis.as_ref().filter(|_| self.settings.settings().view(View::Unused).value) {
                let mut unused: Vec<(Usage, String, NodeId)> =
                    (0..g.node_count() as NodeId).filter(|&n| a.usage[n as usize] != Usage::Used).map(|n| (a.usage[n as usize], full_path(g, n), n)).collect();
                unused.sort_by(|x, y| (x.0 != Usage::Unused, &x.1).cmp(&(y.0 != Usage::Unused, &y.1)));
                ui.label(egui::RichText::new(format!("UNUSED CODE ({})", unused.len())).small().color(style::EDGE_UP));
                for (u, path, n) in unused.iter().take(400) {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        if ui.selectable_label(self.selected == Some(*n), g.name(*n)).on_hover_text(path).clicked() {
                            fly = Some(*n);
                        }
                        ui.label(egui::RichText::new(u.label().unwrap_or("")).small().color(if *u == Usage::Unused { style::EDGE_UP } else { style::MUTED }));
                    });
                }
                ui.add_space(12.0);
            }
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
        if let Some(i) = review_pick {
            self.goto_review_item(i);
        }
        if let (Some(i), Some(rv)) = (review_toggle, self.review.as_mut()) {
            self.review_error = rv.toggle(i);
        }
        if let (Some((hide, grouped)), Some(rv)) = (review_switches, self.review.as_mut()) {
            rv.hide_reviewed = hide;
            rv.by_package = grouped;
            rv.reorder();
        }
        if let Some(i) = select {
            self.select_entry(i);
        }
        if let Some(n) = fly {
            self.selected = Some(n);
            self.fly_to(n);
        }
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        let unused_label = self.selected.and_then(|n| self.unused(n)).and_then(Usage::label);
        let g = self.file.graph();
        egui::ScrollArea::vertical().show(ui, |ui| {
            if let Some(i) = self.selected_entry {
                ui.label(egui::RichText::new("ENTRY POINT").small().color(style::MUTED));
                ui.label(egui::RichText::new(g.entry_label(i)).size(16.0).color(style::ENTRY)).on_hover_text(format!("found by {}", g.entry_rule(i)));
                let mut line = format!("{} · {}", g.entry_kind(i).replace('-', " "), g.entry_lifecycle(i).label());
                if let Some(h) = g.entry_handoff(i) {
                    line.push_str(&format!(" · hands off to {h}"));
                }
                ui.label(egui::RichText::new(line).color(style::MUTED));
                ui.add_space(6.0);
                ui.separator();
            }
            let Some(n) = self.selected else {
                ui.label(egui::RichText::new("Click a box to inspect it. Double-click to zoom into it.").color(style::MUTED));
                return;
            };
            let h = inspector::header(g, n);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&h.kind).small().color(style::MUTED));
                if self.focus.is_some() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Clear").on_hover_text("Esc").clicked() {
                            self.pending_clear = true;
                        }
                        if ui.small_button("Back").on_hover_text("Alt+Left").clicked() {
                            self.pending_back = true;
                        }
                    });
                }
            });
            ui.label(egui::RichText::new(&h.name).size(18.0).color(style::TEXT));
            if !h.owners.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for (k, (owner, name)) in h.owners.iter().enumerate() {
                        if k > 0 {
                            ui.label(egui::RichText::new("·").color(style::MUTED));
                        }
                        if ui.link(egui::RichText::new(name).small()).clicked() {
                            self.pending_go = Some(*owner);
                        }
                    }
                });
            }
            if let Some(loc) = &h.location {
                ui.label(egui::RichText::new(loc).small().color(style::MUTED)).on_hover_text(g.file(n).unwrap_or(""));
            }
            if let Some(label) = unused_label {
                ui.label(egui::RichText::new(label).small().color(style::EDGE_UP))
                    .on_hover_text("Nothing in main code calls, creates or names it outside its own lines. Code reached by reflection or a framework can show up here.");
            }
            let owns_entry = self.app_counts.contains_key(&n) || (0..g.entries().len()).any(|i| g.entries()[i].node.to_native() == n);
            let chips = inspector::chips(g, n, owns_entry);
            if !chips.is_empty() {
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    for c in chips {
                        let text = egui::RichText::new(&c.label).small().color(style::TEXT);
                        ui.add(egui::Button::new(text).fill(style::BAR).corner_radius(8.0).sense(Sense::hover())).on_hover_text(c.hint);
                    }
                });
            }
            if let Some(l) = self.cache.get(n) {
                let up = l.up_edges();
                if up > 0 {
                    ui.colored_label(style::EDGE_UP, format!("{up} upward dependencies inside"));
                }
            }
            if let Some(p) = g.parent(n) {
                if let Some(l) = self.cache.get(p) {
                    for e in l.edges.iter().filter(|e| e.dir == Direction::Up && (e.a == n || e.b == n)) {
                        ui.colored_label(style::EDGE_UP, format!("Upward: {} -> {}", g.name(e.a), g.name(e.b)));
                    }
                }
            }
            if let Some(f) = self.focus.as_ref().filter(|f| f.node == n) {
                for (title, list, color) in [("USES", &f.calls, style::REACH), ("USED BY", &f.callers, style::CALLER)] {
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(format!("{title} ({})", list.len())).small().strong().color(color));
                        if title == "USES" {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let modes = [(FocusMode::Neighbours, "neighbours"), (FocusMode::Reaches, "everything it reaches"), (FocusMode::UsedBy, "everywhere it's used")];
                                let current = modes.iter().find(|(m, _)| *m == self.focus_mode).map_or("", |(_, l)| *l);
                                egui::ComboBox::from_id_salt("focus-mode").selected_text(egui::RichText::new(current).small()).show_ui(ui, |ui| {
                                    for (mode, label) in modes {
                                        if ui.selectable_label(self.focus_mode == mode, label).clicked() && self.focus_mode != mode {
                                            self.focus_mode = mode;
                                            self.pending_refocus = true;
                                        }
                                    }
                                });
                            });
                        }
                    });
                    if list.is_empty() {
                        ui.label(egui::RichText::new("nothing").small().color(style::MUTED));
                    }
                    for group in inspector::group_links(g, n, list) {
                        egui::CollapsingHeader::new(egui::RichText::new(format!("{}  ({})", group.label, group.items.len())).small().color(style::MUTED))
                            .id_salt((title, &group.label))
                            .default_open(group.items.len() <= LIST_LIMIT)
                            .show(ui, |ui| {
                                for (m, label) in group.items.iter().take(LIST_LIMIT) {
                                    if ui.selectable_label(false, label).on_hover_text(full_path(g, *m)).clicked() {
                                        self.pending_go = Some(*m);
                                    }
                                }
                                if group.items.len() > LIST_LIMIT {
                                    ui.label(egui::RichText::new(format!("… {} more", group.items.len() - LIST_LIMIT)).color(style::MUTED));
                                }
                            });
                    }
                }
            }
            let fields: Vec<_> = g.fields(n).collect();
            if !fields.is_empty() {
                ui.add_space(10.0);
                ui.label(egui::RichText::new(format!("FIELDS ({})", fields.len())).small().strong().color(style::MUTED));
                egui::CollapsingHeader::new(egui::RichText::new("list").small().color(style::MUTED)).id_salt(("fields", n)).default_open(fields.len() <= 12).show(ui, |ui| {
                    for f in fields.iter().take(LIST_LIMIT) {
                        let vis = f.visibility.uml();
                        let ty = if f.ty.is_empty() { String::new() } else { format!(": {}", f.ty) };
                        ui.label(egui::RichText::new(format!("{vis} {}{ty}", f.name).trim().to_string()).small());
                    }
                });
            }
            let opts = self.cache.options().clone();
            let (summary, kids) = inspector::inside(g, n, &|c| opts.hides(g, c));
            if !kids.is_empty() && g.kind(n) != NodeKind::Member {
                ui.add_space(10.0);
                ui.label(egui::RichText::new("INSIDE").small().strong().color(style::MUTED));
                ui.label(egui::RichText::new(summary).small().color(style::MUTED));
                egui::CollapsingHeader::new(egui::RichText::new("list").small().color(style::MUTED)).id_salt(("inside", n)).default_open(kids.len() <= 12).show(ui, |ui| {
                    for (c, name) in kids.iter().take(LIST_LIMIT) {
                        if ui.selectable_label(false, name).clicked() {
                            self.pending_go = Some(*c);
                        }
                    }
                    if kids.len() > LIST_LIMIT {
                        ui.label(egui::RichText::new(format!("… {} more", kids.len() - LIST_LIMIT)).color(style::MUTED));
                    }
                });
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
            let (down, up) = ui.input_mut(|i| (i.consume_key(egui::Modifiers::NONE, Key::ArrowDown), i.consume_key(egui::Modifiers::NONE, Key::ArrowUp)));
            let moved = down || up;
            if moved {
                self.search_cursor = search::step_selection(self.search_cursor, self.search_hits.len(), down);
            }
            let edit = ui.add(egui::TextEdit::singleline(&mut self.search_text).hint_text("type, package or module name").desired_width(380.0));
            edit.request_focus();
            if edit.changed() {
                self.search_hits = self.search.query(self.file.graph(), &self.search_text, 30);
                self.search_cursor = 0;
            }
            let g = self.file.graph();
            if ui.input(|i| i.key_pressed(Key::Enter)) {
                fly = self.search_hits.get(self.search_cursor).copied();
            }
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for (i, &h) in self.search_hits.iter().enumerate() {
                    let path: Vec<&str> = g.path(h).iter().skip(1).map(|&p| g.name(p)).collect();
                    let row = ui.selectable_label(i == self.search_cursor, format!("{}   {}", g.name(h), path[..path.len().saturating_sub(1)].join(" / ")));
                    if moved && i == self.search_cursor {
                        row.scroll_to_me(None);
                    }
                    if row.clicked() {
                        fly = Some(h);
                    }
                }
            });
        });
        if let Some(n) = fly {
            self.set_focus(n, None, true, true);
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
        self.pointer = resp.hover_pos();
        if resp.drag_started() {
            self.scroll_drag = resp.interact_pointer_pos().and_then(|p| self.scrollbar_at(p));
        }
        if let (Some(node), Some(p), true) = (self.scroll_drag, resp.interact_pointer_pos(), resp.dragged()) {
            if let Some(v) = self.pointer_scroll(node, p) {
                self.file_scroll.insert(node, v);
            }
        } else if resp.dragged() {
            let d = resp.drag_delta();
            self.cam.pan_by([d.x, d.y]);
            self.target = None;
        }
        if resp.drag_stopped() {
            self.scroll_drag = None;
        }
        if let Some(p) = resp.hover_pos() {
            let (scroll, pinch, mods) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers));
            let page = self.file_page_at(p).filter(|_| !(mods.command || mods.ctrl) && scroll != egui::Vec2::ZERO && (pinch - 1.0).abs() < 1e-4);
            if let Some(node) = page {
                let (sideways, down) = if mods.shift { (if scroll.x != 0.0 { scroll.x } else { scroll.y }, 0.0) } else { (scroll.x, scroll.y) };
                if let Some(v) = self.scrolled(node, down).filter(|_| down != 0.0) {
                    self.file_scroll.insert(node, v);
                }
                if let Some(v) = self.hscrolled(node, sideways).filter(|_| sideways != 0.0) {
                    self.file_hscroll.insert(node, v);
                }
            } else { match interpret(mods.command || mods.ctrl, mods.shift, [scroll.x, scroll.y], pinch) {
                Nav::Zoom(f) => {
                    self.cam.zoom_at([p.x, p.y], f, self.view);
                    self.target = None;
                }
                Nav::Pan(d) => {
                    self.cam.pan_by(d);
                    self.target = None;
                }
                Nav::None => {}
            } }
        }
        let mut clicked_box = None;
        let mut display = None;
        let mut details_toggle: Option<(NodeId, bool)> = None;
        if resp.clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                let root = self.cache.shared(g, g.root());
                let (legend, legend_toggle) = self.legend_rects();
                if let Some(node) = self.scrollbar_at(p) {
                    if let Some(v) = self.pointer_scroll(node, p) {
                self.file_scroll.insert(node, v);
            }
                } else if legend_toggle.contains(Pos2::new(p.x, p.y)) {
                    let shown = self.settings.settings().view(View::Legend).value;
                    display = Some((View::Legend, !shown));
                } else if legend.contains(Pos2::new(p.x, p.y)) {
                } else if let Some(w) = minimap::world_at(&root, self.view, [p.x, p.y]) {
                    self.target = Some(Camera { center: w, zoom: self.cam.zoom });
                } else if let Some(b) = self.scene.boxes.iter().rev().find(|b| b.open && b.hidden_details + b.shown_details > 0 && chip_rect(b.rect).contains([p.x, p.y])) {
                    details_toggle = Some((b.node, b.hidden_details > 0));
                } else {
                    clicked_box = self.file_page_at(p).or_else(|| self.scene.row_at([p.x, p.y])).or_else(|| self.scene.box_at([p.x, p.y]));
                }
            }
        }
        let mut fly = None;
        let mut open_row: Option<(NodeId, NodeId)> = None;
        if resp.double_clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                if let Some(m) = self.scene.row_at([p.x, p.y]) {
                    open_row = g.parent(m).map(|t| (m, t));
                } else if let Some(n) = self.file_page_at(p).or_else(|| self.scene.box_at([p.x, p.y])) {
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
        self.hovered = resp.hover_pos().and_then(|p| self.scene.box_at([p.x, p.y]));
        let anchor = self.pointer.filter(|_| !self.scripted).map_or(self.view.center(), |p| [p.x, p.y]);
        let open_files: Vec<NodeId> = self.scene.boxes.iter().filter(|b| b.source).map(|b| b.node).collect();
        self.scene = build_scene_at(g, &mut self.cache, &self.cam, self.view, &self.visual, self.focus.as_ref().map(|f| &f.marks), anchor, &open_files);
        if let Some(rv) = &self.review {
            review_dim(rv, &mut self.scene);
        }
        let shown: Vec<(NodeId, f32, f32)> = self.scene.boxes.iter().filter(|b| b.source).filter_map(|b| Some((b.node, b.rect.y, file_font(b.rect.w)? * LINE_SPACING))).collect();
        self.file_scroll.retain(|n, _| shown.iter().any(|(s, _, _)| s == n));
        self.file_hscroll.retain(|n, _| shown.iter().any(|(s, _, _)| s == n));
        for (n, y, line_h) in shown {
            let start = g.span(n).0;
            let view_y = self.view.y;
            let opening = match self.goto_line {
                Some((t, line)) if t == n => line.saturating_sub(4) as f32,
                _ => initial_scroll(start, y, view_y, line_h),
            };
            if self.target.is_some() {
                self.file_scroll.insert(n, opening);
            } else {
                self.file_scroll.entry(n).or_insert(opening);
            }
        }
        if self.target.is_none() && self.goto_line.is_some_and(|(t, _)| self.file_scroll.contains_key(&t)) {
            self.goto_line = None;
        }
        self.scene_ms = started.elapsed().as_secs_f32() * 1000.0;
        self.paint(&painter);
        let root = self.cache.shared(g, g.root());
        minimap::draw(&painter, &root, &self.cam, self.view);
        self.paint_legend(&painter);
        if let Some(n) = clicked_box {
            self.set_focus(n, None, false, true);
        }
        if let Some((v, on)) = display {
            self.set_display(&SettingKey::View(v), on);
        }
        if let Some((node, show)) = details_toggle {
            self.details_in.retain(|&n| n != node);
            if show {
                self.details_in.push(node);
            }
            let mut opts = self.cache.options().clone();
            opts.details_in = self.details_in.clone();
            self.cache.set_options(opts);
        }
        if let Some(n) = fly {
            self.set_focus(n, None, true, true);
        }
        if let Some((m, t)) = open_row {
            self.goto_line = Some((t, self.file.graph().span(m).0));
            self.set_focus(m, None, false, true);
            self.fly_to(t);
        }
    }

    fn paint(&self, painter: &egui::Painter) {
        painter.rect_filled(erect(self.view), 0.0, style::BG);
        let painter = painter.with_clip_rect(erect(self.view));
        let max_depth = self.scene.boxes.iter().map(|b| b.depth).max().unwrap_or(0);
        let g = self.file.graph();
        let mut heaviest: HashMap<Option<NodeId>, u32> = HashMap::new();
        for e in &self.scene.edges {
            let m = heaviest.entry(g.parent(e.a)).or_default();
            *m = (*m).max(e.links);
        }
        for depth in 1..=max_depth {
            for e in self.scene.edges.iter().filter(|e| e.depth == depth) {
                self.paint_edge(&painter, e, heaviest[&g.parent(e.a)]);
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
            for b in self.scene.boxes.iter().filter(|b| b.depth == depth && !(b.rows.is_empty() && b.field_rows.is_empty())) {
                self.paint_rows(&painter, b);
            }
        }
        for b in self.scene.boxes.iter().filter(|b| b.source) {
            self.paint_file(&painter, b);
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
        let change = self.review.as_ref().map(|rv| rv.review.change(b.node));
        let reviewed = self.review.as_ref().is_some_and(|rv| rv.states.get(&b.node) == Some(&MarkState::Reviewed));
        let unused = self.unused(b.node);
        if unused.is_some() {
            stroke = Stroke::new(1.5, style::EDGE_UP.gamma_multiply(0.8));
        }
        if let Some(c) = change.filter(|_| !reviewed) {
            if c.status == Status::New {
                stroke = Stroke::new(2.0, style::REACH);
            } else if c.status == Status::Changed {
                stroke = Stroke::new(2.0, style::EDGE_SIDEWAYS);
            }
        }
        if b.dimmed {
            stroke.color = stroke.color.gamma_multiply(0.3);
        }
        if self.selected == Some(b.node) {
            stroke = Stroke::new(2.5, style::ACCENT);
        }
        painter.rect_stroke(r, 3.0, stroke, StrokeKind::Inside);
        if b.open && b.hidden_details + b.shown_details > 0 && b.rect.w > 140.0 {
            let chip = erect(chip_rect(b.rect));
            painter.rect_filled(chip, 8.0, style::BAR);
            painter.rect_stroke(chip, 8.0, Stroke::new(1.0, style::FRAME), StrokeKind::Inside);
            painter.text(chip.center(), Align2::CENTER_CENTER, if b.hidden_details > 0 { format!("+ {} details", b.hidden_details) } else { format!("hide {} details", b.shown_details) }, FontId::proportional(10.5), style::MUTED);
        }
        if b.kind == NodeKind::Member && !b.source && b.rect.w > 48.0 {
            let size = (b.rect.h * 0.18).clamp(8.0, 12.0);
            self.complexity_pill(painter, Pos2::new(b.rect.right() - 6.0, b.rect.y + size), self.file.graph().complexity(b.node), size);
        }
        if self.app_counts.contains_key(&b.node) && b.rect.w > 24.0 {
            painter.circle_filled(Pos2::new(b.rect.right() - 9.0, b.rect.y + 9.0), 4.0, style::ENTRY);
        }
        let badge = if reviewed { Some(("reviewed".to_string(), style::MUTED)) } else { change.and_then(|c| review_badge(c, b.open)) };
        let badge = badge.or_else(|| unused.map(|u| (unused_short(u).to_string(), style::EDGE_UP)));
        if let Some((text, color)) = badge.filter(|_| b.rect.w > 60.0 && b.rect.h > 22.0) {
            let galley = painter.layout_no_wrap(text, FontId::proportional(10.5), style::BG);
            let size = galley.size() + egui::vec2(10.0, 4.0);
            let card = !(b.rows.is_empty() && b.field_rows.is_empty());
            let chip = if card { egui::Rect::from_min_size(r.left_top() + egui::vec2(5.0, 5.0), size) } else { egui::Rect::from_min_size(r.right_bottom() - size - egui::vec2(5.0, 5.0), size) };
            if chip.left() > r.left() {
                painter.rect_filled(chip, 7.0, color);
                painter.galley(chip.center() - galley.size() / 2.0, galley, style::BG);
            }
        }
    }

    fn paint_edge(&self, painter: &egui::Painter, e: &crate::canvas::SceneEdge, heaviest: u32) {
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
        let hovered = self.hovered.is_some_and(|h| e.a == h || e.b == h);
        let links = if e.weight == 0 { e.implements } else { e.links };
        let (width, alpha) = if self.settings.settings().view(View::QuietArrows).value {
            let look = style::edge_look(links, heaviest, marked || hovered);
            (if marked { look.width.max(2.2) } else { look.width }, look.alpha)
        } else {
            (style::edge_width(links, marked), if marked { 1.0 } else { style::edge_alpha(links) })
        };
        match e.mark {
            Mark::Down | Mark::Focus => color = style::REACH,
            Mark::Up => color = style::CALLER,
            Mark::None => {}
        }
        let introduced = self.review.as_ref().is_some_and(|rv| rv.new_arrows.contains(&(e.a, e.b)));
        let (width, alpha) = if introduced {
            color = style::REACH;
            (width.max(2.5), 1.0)
        } else {
            (width, alpha)
        };
        if e.dimmed && !hovered {
            color = color.gamma_multiply(0.2);
        } else {
            color = color.gamma_multiply(alpha);
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
            let head = if implements_only { 11.0 } else { 5.0 + width * 1.5 };
            let side = egui::vec2(-dir.y, dir.x) * head * if implements_only { 0.55 } else { 0.45 };
            let tri = vec![b, b - dir * head + side, b - dir * head - side];
            if implements_only {
                painter.add(Shape::convex_polygon(tri, style::BG, Stroke::new(1.3, color)));
            } else {
                painter.add(Shape::convex_polygon(tri, color, Stroke::NONE));
            }
        }
    }

    fn legend_rects(&self) -> (egui::Rect, egui::Rect) {
        let corner = Pos2::new(self.view.x + 10.0, self.view.bottom() - 10.0);
        if !self.settings.settings().view(View::Legend).value {
            let chip = egui::Rect::from_min_size(corner - egui::vec2(0.0, 22.0), egui::vec2(78.0, 22.0));
            return (chip, chip);
        }
        let opts = self.cache.options();
        let rows = style::legend(opts.show_wiring, opts.show_noise).len().div_ceil(2);
        let size = egui::vec2(minimap::area(self.view).w * 2.0 + 8.0, rows as f32 * LEGEND_ROW + 10.0);
        let rect = egui::Rect::from_min_size(corner - egui::vec2(0.0, size.y), size);
        (rect, egui::Rect::from_min_size(rect.right_top() - egui::vec2(18.0, 0.0), egui::vec2(18.0, 18.0)))
    }

    fn paint_legend(&self, painter: &egui::Painter) {
        let (top, close) = self.legend_rects();
        if !self.settings.settings().view(View::Legend).value {
            painter.rect_filled(top, 4.0, style::BAR);
            painter.text(top.left_center() + egui::vec2(10.0, 0.0), Align2::LEFT_CENTER, "Legend", FontId::proportional(11.0), style::MUTED);
            chevron(painter, top.right_center() - egui::vec2(14.0, 0.0), true);
            return;
        }
        let opts = self.cache.options();
        let items = style::legend(opts.show_wiring, opts.show_noise);
        let row = LEGEND_ROW;
        let half = items.len().div_ceil(2);
        let width = top.width();
        painter.rect_filled(top, 4.0, style::BAR);
        chevron(painter, close.center(), false);
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

    fn file_page(&self, b: &crate::canvas::SceneBox) -> egui::Rect {
        egui::Rect::from_min_max(Pos2::new(b.rect.x, self.view.y), Pos2::new(b.rect.right(), self.view.bottom())).intersect(erect(self.view))
    }

    fn file_page_at(&self, p: Pos2) -> Option<NodeId> {
        self.scene.boxes.iter().rev().filter(|b| b.source).find(|b| self.file_page(b).contains(p)).map(|b| b.node)
    }

    fn file_metrics(&self, node: NodeId) -> Option<(f32, usize, usize)> {
        let b = self.scene.boxes.iter().find(|b| b.node == node && b.source)?;
        let line_h = file_font(b.rect.w)? * LINE_SPACING;
        let total = self.source.borrow_mut().file(self.file.graph().file(node)?)?.len();
        Some((line_h, total, (self.view.h / line_h) as usize))
    }

    fn hscrolled(&self, node: NodeId, wheel_px: f32) -> Option<f32> {
        let b = self.scene.boxes.iter().find(|b| b.node == node && b.source)?;
        let size = file_font(b.rect.w)?;
        let lines = self.source.borrow_mut().file(self.file.graph().file(node)?)?;
        let longest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
        let text = self.file_text_area(b, size, lines.len());
        Some(hscroll_by(self.file_hscroll.get(&node).copied().unwrap_or(0.0), wheel_px, longest, advance(size), text.width()))
    }

    fn file_text_area(&self, b: &crate::canvas::SceneBox, size: f32, total: usize) -> egui::Rect {
        let page = self.file_page(b);
        let gutter = total.to_string().len() as f32 * advance(size) + 2.0 * advance(size);
        egui::Rect::from_min_max(Pos2::new(b.rect.x + gutter + advance(size) * 0.5, page.top()), Pos2::new(self.scrollbar_track(page).left() - 4.0, page.bottom()))
    }

    fn scrolled(&self, node: NodeId, wheel_px: f32) -> Option<f32> {
        let (line_h, total, visible) = self.file_metrics(node)?;
        Some(scroll_by(self.file_scroll.get(&node).copied().unwrap_or(0.0), wheel_px, line_h, total, visible))
    }

    fn paint_file(&self, painter: &egui::Painter, b: &crate::canvas::SceneBox) {
        let g = self.file.graph();
        let Some(path) = g.file(b.node) else { return };
        let Some(lines) = self.source.borrow_mut().file(path) else { return };
        let Some(size) = file_font(b.rect.w) else { return };
        let line_h = size * LINE_SPACING;
        let scroll = self.file_scroll.get(&b.node).copied().unwrap_or(0.0);
        let sheet = self.file_page(b);
        let top = sheet.top() + 6.0 - scroll * line_h;
        let clip = painter.with_clip_rect(sheet);
        clip.rect_filled(sheet, 4.0, style::PANEL);
        clip.rect_stroke(sheet, 4.0, Stroke::new(1.5, if self.selected == Some(b.node) { style::ACCENT } else { style::FRAME }), StrokeKind::Inside);
        let mut own = vec![g.span(b.node)];
        let mut stack: Vec<NodeId> = g.children(b.node).collect();
        while let Some(c) = stack.pop() {
            if g.file(c) == Some(path) {
                own.push(g.span(c));
            }
            stack.extend(g.children(c));
        }
        let own = merge_spans(own);
        for (s, e) in own.iter() {
            let band = egui::Rect::from_min_size(Pos2::new(b.rect.x + 2.0, top + (*s - 1) as f32 * line_h), egui::vec2(b.rect.w - 4.0, (*e - *s + 1) as f32 * line_h));
            clip.rect_filled(band, 0.0, style::BOX_STROKE.gamma_multiply(0.10));
        }
        self.paint_scrollbar(&clip, sheet, b.node, scroll, lines.len(), (sheet.height() / line_h) as usize);
        let text_clip = painter.with_clip_rect(self.file_text_area(b, size, lines.len()));
        let hscroll = self.file_hscroll.get(&b.node).copied().unwrap_or(0.0);
        let first = ((self.view.y - top) / line_h).floor().max(0.0) as usize;
        let last = (((self.view.bottom() - top) / line_h).ceil().max(0.0) as usize).min(lines.len());
        if first >= last {
            return;
        }
        let mono = FontId::monospace(size);
        let gutter = lines.len().to_string().len() as f32 * advance(size) + 2.0 * advance(size);
        let diff = self.review.as_ref().and_then(|rv| rv.review.file(path));
        let full = |y: f32, h: f32| egui::Rect::from_min_size(Pos2::new(b.rect.x + 2.0, y), egui::vec2(b.rect.w - 4.0, h));
        if let Some(d) = diff {
            for &l in d.added.iter().filter(|&&l| (first + 1..=last).contains(&(l as usize))) {
                let band = full(top + (l - 1) as f32 * line_h, line_h);
                clip.rect_filled(band, 0.0, style::REACH.gamma_multiply(0.14));
                clip.rect_filled(egui::Rect::from_min_size(band.min, egui::vec2(3.0, line_h)), 0.0, style::REACH);
            }
        }
        let text: Vec<&str> = lines[..last].iter().map(String::as_str).collect();
        for (i, segments) in highlight(&text, g.lang(b.node)).into_iter().enumerate().skip(first) {
            let y = top + i as f32 * line_h;
            clip.text(Pos2::new(b.rect.x + gutter, y), Align2::RIGHT_TOP, (i + 1).to_string(), mono.clone(), style::MUTED.gamma_multiply(0.6));
            let mut job = egui::text::LayoutJob::default();
            for (text, kind) in segments {
                let color = match kind {
                    Token::Text => style::TEXT,
                    Token::Keyword => style::CODE_KEYWORD,
                    Token::String => style::CODE_STRING,
                    Token::Comment => style::MUTED,
                };
                job.append(&text, 0.0, egui::TextFormat { font_id: mono.clone(), color, ..Default::default() });
            }
            text_clip.galley(Pos2::new(b.rect.x + gutter + advance(size) - hscroll, y), text_clip.layout_job(job), style::TEXT);
        }
        let Some(d) = diff else { return };
        let mut hovered: Option<(f32, &Vec<String>)> = None;
        for (before, gone) in d.removed.iter().filter(|(l, _)| (first..=last + 1).contains(&(*l as usize))) {
            let y = top + (*before - 1) as f32 * line_h;
            clip.line_segment([Pos2::new(b.rect.x + 2.0, y), Pos2::new(b.rect.right() - 2.0, y)], Stroke::new(2.0, style::EDGE_UP));
            let chip = egui::Rect::from_min_size(Pos2::new(sheet.left().max(b.rect.x) + 4.0, y - size * 0.6), egui::vec2(advance(size) * (gone.len().to_string().len() as f32 + 2.0), size * 1.2));
            clip.rect_filled(chip, 3.0, style::EDGE_UP);
            clip.text(chip.center(), Align2::CENTER_CENTER, format!("−{}", gone.len()), FontId::monospace(size * 0.85), style::BG);
            if self.pointer.is_some_and(|p| (p.y - y).abs() < line_h * 0.6 && sheet.contains(p)) {
                hovered = Some((y, gone));
            }
        }
        if let Some((y, gone)) = hovered {
            let shown = gone.len().min(30);
            let pop = full(y + 2.0, shown as f32 * line_h + 8.0).intersect(sheet);
            clip.rect_filled(pop, 3.0, style::tint(style::PANEL, style::EDGE_UP, 0.18));
            clip.rect_stroke(pop, 3.0, Stroke::new(1.0, style::EDGE_UP), StrokeKind::Inside);
            for (k, l) in gone.iter().take(shown).enumerate() {
                text_clip.text(Pos2::new(b.rect.x + gutter + advance(size) - hscroll, y + 6.0 + k as f32 * line_h), Align2::LEFT_TOP, l, mono.clone(), style::TEXT.gamma_multiply(0.8));
            }
        }
    }

    fn pointer_in(&self, r: egui::Rect) -> bool {
        self.pointer.is_some_and(|p| r.expand2(egui::vec2(4.0, 0.0)).contains(p))
    }

    fn scrollbar_track(&self, page: egui::Rect) -> egui::Rect {
        egui::Rect::from_min_max(Pos2::new(page.right() - 12.0, page.top() + 6.0), Pos2::new(page.right() - 4.0, page.bottom() - 6.0))
    }

    fn scrollbar_at(&self, p: Pos2) -> Option<NodeId> {
        self.scene.boxes.iter().rev().filter(|b| b.source).find(|b| self.scrollbar_track(self.file_page(b)).expand2(egui::vec2(4.0, 0.0)).contains(p)).map(|b| b.node)
    }

    fn pointer_scroll(&self, node: NodeId, p: Pos2) -> Option<f32> {
        let b = self.scene.boxes.iter().find(|b| b.node == node && b.source)?;
        let track = self.scrollbar_track(self.file_page(b));
        let (_, total, visible) = self.file_metrics(node)?;
        Some(scroll_at(p.y - track.top(), total, visible, track.height()))
    }

    fn paint_scrollbar(&self, painter: &egui::Painter, page: egui::Rect, node: NodeId, scroll: f32, total: usize, visible: usize) {
        let track = self.scrollbar_track(page);
        painter.rect_filled(track, 4.0, style::BAR);
        let (top, len) = thumb(scroll, total, visible, track.height());
        let hot = self.scroll_drag == Some(node) || self.pointer_in(track);
        let knob = egui::Rect::from_min_size(Pos2::new(track.left() + 1.0, track.top() + top), egui::vec2(track.width() - 2.0, len));
        painter.rect_filled(knob, 3.0, style::MUTED.gamma_multiply(if hot { 0.9 } else { 0.55 }));
    }

    fn paint_rows(&self, painter: &egui::Painter, b: &crate::canvas::SceneBox) {
        let g = self.file.graph();
        let card = erect(b.rect);
        let clip = painter.with_clip_rect(card.shrink(1.0).intersect(erect(self.view)));
        let first = b.field_rows.first().map(|f| f.rect).or(b.rows.first().map(|r| r.rect)).unwrap();
        let row_h = first.h;
        let size = (row_h * 0.72).min(13.0);
        let font = FontId::proportional(size);
        let dim = |c: Color32| if b.dimmed { c.gamma_multiply(0.35) } else { c };
        let header = egui::Rect::from_min_max(card.min, Pos2::new(card.max.x, first.y - row_h * 0.3));
        clip.rect_filled(header, egui::CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, dim(style::tint(style::BAR, style::kind_stroke(b.sub), 0.18)));
        clip.line_segment([header.left_bottom(), header.right_bottom()], Stroke::new(1.0, dim(style::kind_stroke(b.sub).gamma_multiply(0.7))));
        let title = (header.height() * 0.34).clamp(7.0, 15.0);
        let stereo = style::stereotype(b.sub, g.lang(b.node));
        let name_y = if stereo.is_some() { header.center().y + title * 0.45 } else { header.center().y };
        if let Some(st) = stereo {
            clip.text(Pos2::new(header.center().x, name_y - title * 1.05), Align2::CENTER_CENTER, st, FontId::proportional(title * 0.75), dim(style::MUTED));
        }
        let highest = b.rows.iter().filter_map(|r| g.complexity(r.node)).max();
        self.complexity_pill(&clip, Pos2::new(header.right() - 6.0, header.top() + title * 0.9), highest, (title * 0.8).max(7.0));
        let name = fit_label(g.name(b.node), header.width() - 12.0, title);
        let title_color = if self.selected == Some(b.node) { style::ACCENT } else { dim(style::BOX_TITLE) };
        clip.text(Pos2::new(header.center().x, name_y), Align2::CENTER_CENTER, name, FontId::proportional(title), title_color);
        for f in &b.field_rows {
            let r = erect(f.rect);
            let vis = f.visibility.uml();
            let name = if vis.is_empty() { f.name.clone() } else { format!("{vis} {}", f.name) };
            let galley = clip.layout_no_wrap(name, font.clone(), dim(style::TEXT));
            let at = Pos2::new(f.rect.x + size * 1.1, r.center().y - galley.size().y / 2.0);
            let width = galley.size().x;
            clip.galley(at, galley, dim(style::TEXT));
            if !f.ty.is_empty() {
                let ty = fit_label(&format!(": {}", f.ty), (f.rect.w - size * 1.4 - width).max(0.0), size);
                clip.text(Pos2::new(at.x + width, r.center().y), Align2::LEFT_CENTER, ty, font.clone(), dim(style::MUTED));
            }
        }
        if b.more_fields > 0 {
            let last = erect(b.field_rows[b.field_rows.len() - 1].rect);
            clip.text(Pos2::new(last.left() + size * 1.1, last.bottom() + last.height() / 2.0), Align2::LEFT_CENTER, format!("… {} more", b.more_fields), font.clone(), style::MUTED);
        }
        if let (Some(_), Some(m)) = (b.field_rows.first(), b.rows.first()) {
            let y = m.rect.y - row_h * 0.25;
            clip.line_segment([Pos2::new(card.left(), y), Pos2::new(card.right(), y)], Stroke::new(1.0, dim(style::kind_stroke(b.sub).gamma_multiply(0.5))));
        }
        for (i, row) in b.rows.iter().enumerate() {
            let r = erect(row.rect).expand2(egui::vec2(6.0, 0.0));
            if i % 2 == 1 {
                clip.rect_filled(r, 0.0, style::TEXT.gamma_multiply(0.025));
            }
            if self.pointer.is_some_and(|p| r.contains(p)) || self.selected == Some(row.node) {
                clip.rect_filled(r, 0.0, style::ACCENT.gamma_multiply(if self.selected == Some(row.node) { 0.18 } else { 0.08 }));
            }
            let change = self.review.as_ref().map(|rv| rv.review.change(row.node));
            let card_new = self.review.as_ref().is_some_and(|rv| rv.review.change(b.node).status == Status::New);
            let row_reviewed = self.review.as_ref().is_some_and(|rv| rv.states.get(&row.node) == Some(&MarkState::Reviewed));
            let delta = if row_reviewed { Some(("reviewed".to_string(), style::MUTED)) } else { change.and_then(row_delta).map(|(d, c)| (if card_new { String::new() } else { d }, c)) };
            let delta = delta.or_else(|| self.unused(row.node).map(|u| (unused_short(u).to_string(), style::EDGE_UP)));
            if let Some((_, c)) = &delta {
                clip.rect_filled(r, 0.0, c.gamma_multiply(0.14));
            }
            let mark = Pos2::new(row.rect.x + size * 0.45, r.center().y);
            if srcrelief_layout::is_constructor(g, row.node) {
                let d = size * 0.28;
                let pts = vec![mark + egui::vec2(0.0, -d), mark + egui::vec2(d, 0.0), mark + egui::vec2(0.0, d), mark + egui::vec2(-d, 0.0)];
                clip.add(Shape::convex_polygon(pts, Color32::TRANSPARENT, Stroke::new(1.0, dim(style::CONSTRUCTOR))));
            } else if self.entry_nodes.contains(&row.node) || self.usage.used_from_outside(g, row.node) {
                clip.circle_filled(mark, size * 0.18, dim(if self.entry_nodes.contains(&row.node) { style::ENTRY } else { style::MUTED }));
            }
            let color = match row.mark {
                Mark::Down => style::REACH,
                Mark::Up => style::CALLER,
                Mark::Focus => style::ACCENT,
                Mark::None => dim(style::TEXT),
            };
            let base = if g.name(row.node) == "<init>" { g.parent(row.node).map_or("<init>", |t| g.name(t)) } else { g.name(row.node) };
            let vis = g.visibility(row.node).uml();
            let shown = if vis.is_empty() { format!("{base}()") } else { format!("{vis} {base}()") };
            let mut pill = self.complexity_pill(&clip, Pos2::new(row.rect.right(), r.center().y), g.complexity(row.node), size);
            if let Some((d, c)) = delta {
                let galley = clip.layout_no_wrap(d, FontId::proportional(size * 0.85), c);
                let w = galley.size().x;
                clip.galley(Pos2::new(row.rect.right() - pill - w, r.center().y - galley.size().y / 2.0), galley, c);
                pill += w + size * 0.4;
            }
            let text = fit_label(&shown, row.rect.w - size * 1.4 - pill, size);
            clip.text(Pos2::new(row.rect.x + size * 1.1, r.center().y), Align2::LEFT_CENTER, text, font.clone(), color);
        }
        if b.more_rows > 0 {
            let last = erect(b.rows[b.rows.len() - 1].rect);
            clip.text(Pos2::new(last.left() + size * 1.1, last.bottom() + last.height() / 2.0), Align2::LEFT_CENTER, format!("… {} more", b.more_rows), font, style::MUTED);
        }
    }

    fn complexity_pill(&self, painter: &egui::Painter, right_center: Pos2, value: Option<u16>, size: f32) -> f32 {
        let Some((v, color)) = value.filter(|_| self.settings.settings().view(View::Complexity).value).and_then(|v| style::complexity_hint(v).map(|c| (v, c))) else {
            return 0.0;
        };
        let galley = painter.layout_no_wrap(v.to_string(), FontId::proportional(size * 0.85), style::BG);
        let w = galley.size().x + size * 0.7;
        let r = egui::Rect::from_center_size(right_center - egui::vec2(w / 2.0, 0.0), egui::vec2(w, size * 1.15));
        painter.rect_filled(r, size * 0.55, color);
        painter.galley(r.center() - galley.size() / 2.0, galley, style::BG);
        w + size * 0.3
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
