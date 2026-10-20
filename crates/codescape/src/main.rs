#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "codescape", about = "See a codebase as one zoomable map, and review what changed on it", version)]
struct Args {
    #[arg(help = "Project folder to open (default: the start screen)")]
    dir: Option<PathBuf>,
    #[arg(long, help = "Settings file to use instead of the project's codescape.toml")]
    config: Option<PathBuf>,
    #[arg(long, help = "Render one frame to this PNG and exit")]
    screenshot: Option<PathBuf>,
    #[arg(long, help = "With --screenshot: zoom to the first node matching this name")]
    focus: Option<String>,
    #[arg(long, help = "With --screenshot: select the entry point with this label")]
    entry: Option<String>,
    #[arg(long, help = "With --screenshot: open the project settings window")]
    settings: bool,
    #[arg(long, help = "With --screenshot and --focus: zoom this many times closer than the framed view")]
    zoom: Option<f64>,
    #[arg(long, help = "With --screenshot: open the shortcuts cheatsheet")]
    shortcuts: bool,
    #[arg(long, help = "With --screenshot: compare with this revision (\"main\" for the merge-base with main)")]
    review: Option<String>,
    #[arg(long, help = "With --screenshot: review this branch or commit instead of the working copy")]
    review_target: Option<String>,
    #[arg(long, help = "With --screenshot: review commit by commit, starting at the first")]
    review_by_commit: bool,
    #[arg(long, help = "With --screenshot: open the file at the first method matching this name, as a double-click on its row")]
    open: Option<String>,
    #[arg(long, help = "With --screenshot: show the sequence diagram of the first method matching this name")]
    sequence: Option<String>,
    #[arg(long, help = "With --screenshot: hide both side panels")]
    map_only: bool,
    #[arg(long, help = "With --screenshot: frame the first node matching this name without selecting it")]
    fly: Option<String>,
    #[arg(long, help = "With --screenshot: open the What's on the map panel")]
    map_panel: bool,
    #[arg(long, help = "With --screenshot: search the open file for this text, as Ctrl+F")]
    find: Option<String>,
    #[arg(long, help = "With --screenshot: list the open file's structure, as Ctrl+F12")]
    structure: bool,
    #[arg(long, help = "With --screenshot: open the keyboard shortcuts editor")]
    keys: bool,
    #[arg(long, help = "With --screenshot: underline every name in the open file that Ctrl+click follows")]
    links: bool,
    #[arg(long, help = "With --screenshot: hover the first flag icon in the open file's gutter")]
    flag_hover: bool,
}

fn main() -> eframe::Result {
    let args = Args::parse();
    #[cfg(target_os = "linux")]
    if let (Some(appimage), Some(data)) = (std::env::var_os("APPIMAGE"), dirs::data_dir()) {
        let _ = codescape::desktop::ensure_entry(&data, std::path::Path::new(&appimage));
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(codescape::app::window_title(None))
            .with_app_id("codescape")
            .with_inner_size([1440.0, 900.0])
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../../../packaging/icon/codescape.png")).expect("the app icon is a valid PNG")),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    let shot = args.screenshot.map(|path| codescape::shot::ShotPlan {
        path,
        focus: args.focus,
        entry: args.entry,
        settings: args.settings,
        zoom: args.zoom,
        shortcuts: args.shortcuts,
        review: args.review,
        review_target: args.review_target,
        review_by_commit: args.review_by_commit,
        open: args.open,
        sequence: args.sequence,
        map_only: args.map_only,
        fly: args.fly,
        map_panel: args.map_panel,
        find: args.find,
        structure: args.structure,
        keys: args.keys,
        links: args.links,
        flag_hover: args.flag_hover,
    });
    eframe::run_native("CodeScape", options, Box::new(move |cc| Ok(Box::new(codescape::app::CodescapeApp::new(cc, args.dir, args.config, shot)))))
}
