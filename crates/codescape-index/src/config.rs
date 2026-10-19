use crate::settings::{Layer, Settings, SettingsFile, Threshold};
use anyhow::{Result, anyhow};
use codescape_format::Category;
use codescape_rules::{Pack, PackSelection, Rule};
use std::path::{Path, PathBuf};

pub const CONFIG_FILE: &str = "codescape.toml";

pub const BUILTIN_EXCLUDES: &[&str] =
    &["**/build/**", "**/.gradle/**", "**/node_modules/**", "**/vendor/**", "**/.git/**", "**/target/**", "**/dist/**", "**/.*/**"];

#[derive(Clone, Debug, PartialEq)]
pub struct CategoryOverride {
    pub glob: String,
    pub category: Category,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Filter {
    pub name: String,
    pub paths: Vec<String>,
    pub names: Vec<String>,
    pub shown: bool,
}

impl Filter {
    pub fn tag(&self) -> String {
        filter_tag(&self.name)
    }
}

pub fn filter_tag(name: &str) -> String {
    format!("filter:{name}")
}

#[derive(Clone, Debug)]
pub struct ProjectConfig {
    pub name: Option<String>,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub packs: PackSelection,
    pub categories: Vec<CategoryOverride>,
    pub test_paths: Vec<String>,
    pub filters: Vec<Filter>,
    pub index: Option<PathBuf>,
    pub duplicate_tokens: u32,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        ProjectConfig {
            name: None,
            include: Vec::new(),
            exclude: BUILTIN_EXCLUDES.iter().map(|g| g.to_string()).collect(),
            packs: PackSelection::default(),
            categories: Vec::new(),
            test_paths: Vec::new(),
            filters: Vec::new(),
            index: None,
            duplicate_tokens: Threshold::DuplicateTokens.default_value(),
        }
    }
}

impl ProjectConfig {
    pub fn load(project_dir: &Path, explicit: Option<&Path>) -> Result<ProjectConfig> {
        let path = match explicit {
            Some(p) => p.to_path_buf(),
            None => project_dir.join(CONFIG_FILE),
        };
        if explicit.is_some() && !path.is_file() {
            return Err(anyhow!("config file {} not found", path.display()));
        }
        let file = SettingsFile::read(&path)?;
        let base = path.parent().unwrap_or(project_dir).to_path_buf();
        Ok(Settings::merge(&[(Layer::Shared, &file, path, base)])?.config())
    }

    pub fn fingerprint(&self) -> String {
        blake3::hash(format!("{:?}{:?}{}", self.name, self.exclude, self.duplicate_tokens).as_bytes()).to_hex()[..16].to_string()
    }

    pub fn config_pack(&self) -> Option<Pack> {
        if self.categories.is_empty() && self.test_paths.is_empty() && self.filters.is_empty() {
            return None;
        }
        let mut pack = Pack::parse("name = \"project-config\"\nversion = \"0\"\n", "project config", None).ok()?;
        for (i, c) in self.categories.iter().enumerate() {
            pack.rules.push((format!("project-config.category.{i}"), Rule::Path { glob: c.glob.clone(), category: c.category }));
        }
        if !self.test_paths.is_empty() {
            pack.rules.push(("project-config.test".into(), Rule::TestCode { file_imports: Vec::new(), dir_names: Vec::new(), paths: self.test_paths.clone() }));
        }
        for f in &self.filters {
            pack.rules.push((format!("project-config.filter.{}", f.name), Rule::Filter { tag: f.tag(), paths: f.paths.clone(), names: f.names.clone() }));
        }
        pack.hash = blake3::hash(format!("{:?}", pack.rules).as_bytes()).to_hex()[..16].to_string();
        Some(pack)
    }
}
