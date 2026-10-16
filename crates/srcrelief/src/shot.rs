use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const SETTLE_FRAMES: u32 = 12;

#[derive(Clone, Debug, Default)]
pub struct ShotPlan {
    pub path: PathBuf,
    pub focus: Option<String>,
    pub entry: Option<String>,
    pub overview: bool,
}

pub struct Shot {
    pub plan: ShotPlan,
    frames: u32,
    setup_done: bool,
    requested: bool,
    finished: Arc<AtomicBool>,
}

pub enum ShotStep {
    Setup,
    Wait,
    Capture,
    Close,
}

impl Shot {
    pub fn new(plan: ShotPlan) -> Shot {
        Shot { plan, frames: 0, setup_done: false, requested: false, finished: Arc::new(AtomicBool::new(false)) }
    }

    pub fn step(&mut self, animating: bool) -> ShotStep {
        self.frames += 1;
        if self.finished.load(Ordering::SeqCst) {
            return ShotStep::Close;
        }
        if !self.setup_done && self.frames >= 2 {
            self.setup_done = true;
            return ShotStep::Setup;
        }
        if self.setup_done && !self.requested && !animating && self.frames >= SETTLE_FRAMES {
            self.requested = true;
            return ShotStep::Capture;
        }
        ShotStep::Wait
    }

    pub fn request(&self, ctx: &egui::Context) {
        let path = self.plan.path.clone();
        let finished = self.finished.clone();
        let repaint = ctx.clone();
        ctx.request_screenshot(move |image| {
            if let Err(e) = write_png(&path, &image) {
                eprintln!("screenshot failed: {e}");
            }
            finished.store(true, Ordering::SeqCst);
            repaint.request_repaint();
        });
    }
}

fn write_png(path: &Path, image: &egui::ColorImage) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let [w, h] = image.size;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
    writer.write_image_data(&bytes).map_err(|e| e.to_string())
}
