use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "srcrelief", about = "See a Java or Go codebase as one zoomable map")]
struct Args {
    dir: Option<PathBuf>,
    #[arg(long)]
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
}

fn main() -> eframe::Result {
    let args = Args::parse();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("SrcRelief").with_inner_size([1440.0, 900.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    let shot = args.screenshot.map(|path| srcrelief::shot::ShotPlan { path, focus: args.focus, entry: args.entry, settings: args.settings, zoom: args.zoom, shortcuts: args.shortcuts, review: args.review, review_target: args.review_target, review_by_commit: args.review_by_commit });
    eframe::run_native("SrcRelief", options, Box::new(move |cc| Ok(Box::new(srcrelief::app::ReliefApp::new(cc, args.dir, args.config, shot)))))
}
