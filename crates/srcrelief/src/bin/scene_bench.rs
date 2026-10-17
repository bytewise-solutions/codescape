use srcrelief::camera::Camera;
use srcrelief::canvas::{build_scene, root_world, world_rect};
use srcrelief_format::{NodeKind, ReliefFile};
use srcrelief_index::{ProjectConfig, index_project, project_packs};
use srcrelief_layout::{LayoutCache, LayoutOptions, Rect};
use std::path::PathBuf;
use std::time::Instant;

const VIEW: Rect = Rect { x: 0.0, y: 0.0, w: 1440.0, h: 860.0 };
const STEPS: usize = 300;

fn peak_rss_mb() -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmHWM:")).map(|l| l.split_whitespace().nth(1).unwrap_or("0").parse::<f64>().unwrap_or(0.0)))
        .map_or(0.0, |kb| kb / 1024.0)
}

fn verdict(ok: bool) -> &'static str {
    if ok { "PASS" } else { "FAIL" }
}

fn main() -> anyhow::Result<()> {
    let dir = PathBuf::from(std::env::args().nth(1).expect("usage: scene_bench <project dir> [index out]"));
    let out = std::env::args().nth(2).map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("scene_bench.relief"));
    let cfg = ProjectConfig { index: Some(out.clone()), ..ProjectConfig::load(&dir, None)? };
    let t = Instant::now();
    let outcome = index_project(&dir, &cfg, &|_| {})?;
    let index_s = t.elapsed().as_secs_f64();
    let rss_after_index = peak_rss_mb();

    let t = Instant::now();
    let file = ReliefFile::open(&outcome.path)?;
    let g = file.graph();
    let visual = project_packs(&dir, &cfg)?.visual();
    let mut cache = LayoutCache::new(visual.frames.clone(), LayoutOptions::default());
    let cam = Camera::fit(root_world(&mut cache, g), VIEW);
    let first = build_scene(g, &mut cache, &cam, VIEW, &visual, None);
    let open_ms = t.elapsed().as_secs_f64() * 1000.0;

    let target_node = (0..g.node_count() as u32)
        .filter(|&n| g.kind(n) == NodeKind::Type)
        .max_by_key(|&n| (g.parent(n).map_or(0, |p| g.children(p).len()), n))
        .expect("a type");
    let target = Camera::fit(world_rect(g, &mut cache, target_node).unwrap(), VIEW);
    let mut worst = 0.0f64;
    let mut times = Vec::with_capacity(STEPS);
    let mut max_boxes = 0;
    let mut layouts = 0;
    for i in 0..=STEPS {
        let f = i as f64 / STEPS as f64;
        let zoom = (cam.zoom.ln() + (target.zoom.ln() - cam.zoom.ln()) * f).exp();
        let k = ((zoom / cam.zoom).ln() / (target.zoom / cam.zoom).ln()).clamp(0.0, 1.0);
        let c = Camera { center: [cam.center[0] + (target.center[0] - cam.center[0]) * k, cam.center[1] + (target.center[1] - cam.center[1]) * k], zoom };
        let t = Instant::now();
        let s = build_scene(g, &mut cache, &c, VIEW, &visual, None);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        worst = worst.max(ms);
        times.push(ms);
        max_boxes = max_boxes.max(s.stats.drawn);
        layouts += s.stats.layouts_computed;
    }
    times.sort_by(|a, b| a.total_cmp(b));
    let p99 = times[(times.len() * 99) / 100];

    let mut worst_layout = 0.0f64;
    let mut sizes: Vec<(usize, u32)> = (0..g.node_count() as u32).map(|n| (g.children(n).len(), n)).collect();
    sizes.sort();
    for &(_, n) in sizes.iter().rev().take(5) {
        let mut fresh = LayoutCache::new(visual.frames.clone(), LayoutOptions::default());
        let t = Instant::now();
        fresh.get_or_compute(g, n);
        worst_layout = worst_layout.max(t.elapsed().as_secs_f64() * 1000.0);
    }

    let s = &outcome.summary;
    let ratio = s.index_bytes as f64 / s.source_bytes.max(1) as f64;
    let rss = peak_rss_mb();
    println!("project: {} files, {} lines, {} nodes, {} edges", s.files, s.lines, g.node_count(), g.edge_count());
    println!("{} index in {index_s:.1} s (target < 60 s)", verdict(index_s < 60.0));
    println!(
        "{} index size {:.1} MiB = {:.0}% of {:.1} MiB source (target <= 30%)",
        verdict(ratio <= 0.30),
        s.index_bytes as f64 / 1048576.0,
        ratio * 100.0,
        s.source_bytes as f64 / 1048576.0
    );
    println!("{} open + first scene {open_ms:.0} ms (target < 1000 ms), {} boxes", verdict(open_ms < 1000.0), first.stats.drawn);
    println!(
        "{} zoom: worst scene {worst:.2} ms, p99 {p99:.2} ms over {} steps (target < 16 ms), max {max_boxes} boxes, {layouts} layouts",
        verdict(worst < 16.0),
        STEPS + 1
    );
    println!("{} biggest container layout {worst_layout:.0} ms (target < 100 ms)", verdict(worst_layout < 100.0));
    println!("zoom range {:.2}x -> {:.0}x (single type {})", cam.zoom, target.zoom, g.name(target_node));
    println!("peak RSS {rss:.0} MiB incl. indexing ({rss_after_index:.0} MiB after index) — viewer target < 500 MiB");
    Ok(())
}
