use anyhow::Result;
use clap::Parser;
use srcrelief_format::ReliefFile;
use srcrelief_index::{SettingsPaths, SettingsStore, Summary, index_path, index_project, needs_reindex, project_packs};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "srcrelief-index", about = "Index a Java or Go project directory into a .relief file")]
struct Args {
    dir: PathBuf,
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    out: Option<PathBuf>,
    #[arg(long)]
    force: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let mut cfg = SettingsStore::load(SettingsPaths::new(&args.dir, args.config.as_deref()))?.config();
    if let Some(out) = args.out {
        cfg.index = Some(out);
    }
    let path = index_path(&args.dir, &cfg);
    let packs = project_packs(&args.dir, &cfg)?;
    if !args.force && !needs_reindex(&args.dir, &path, &cfg, &packs) {
        let file = ReliefFile::open(&path)?;
        println!("{}", Summary::from_graph(file.graph(), &packs.visual(), file.byte_len() as u64, Default::default()));
        println!("Up to date: {} (use --force to rebuild)", path.display());
        return Ok(());
    }
    let out = index_project(&args.dir, &cfg, &|p| eprintln!("{p:?}…"))?;
    println!("{}", out.summary);
    println!("Written to {}", out.path.display());
    Ok(())
}
