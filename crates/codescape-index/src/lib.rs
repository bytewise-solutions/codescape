pub mod analysis;
mod cache;
pub mod churn;
pub mod clones;
mod config;
mod pipeline;
pub mod review;
mod settings;
mod summary;

pub use cache::{cache_path, engine_stamp, index_path, needs_reindex, project_key};
pub use config::{BUILTIN_EXCLUDES, CONFIG_FILE, CategoryOverride, Filter, ProjectConfig, filter_tag};
pub use pipeline::{IndexOutcome, Progress, excludes, index_project, project_packs};
pub use settings::{Key, Layer, Settings, SettingsPaths, SettingsStore, Sourced, Threshold, View, config_root};
pub use summary::{EntryLine, ModuleLine, Summary};

pub(crate) fn unique_suffix() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    format!("{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}
