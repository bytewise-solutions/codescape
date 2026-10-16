use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use srcrelief_format::Category;
use srcrelief_rules::{Pack, PackSelection, Rule};
use std::path::{Path, PathBuf};

pub const CONFIG_FILE: &str = "srcrelief.toml";

#[derive(Clone, Debug, PartialEq)]
pub struct CategoryOverride {
    pub glob: String,
    pub category: Category,
}

#[derive(Clone, Debug, Default)]
pub struct ProjectConfig {
    pub name: Option<String>,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub packs: PackSelection,
    pub categories: Vec<CategoryOverride>,
    pub index: Option<PathBuf>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    name: Option<String>,
    #[serde(default)]
    include: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
    #[serde(default)]
    packs: PacksFile,
    #[serde(default, rename = "category")]
    categories: Vec<CategoryFile>,
    index: Option<PathBuf>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct PacksFile {
    #[serde(default)]
    enable: Vec<String>,
    #[serde(default)]
    disable: Vec<String>,
    #[serde(default)]
    dirs: Vec<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CategoryFile {
    glob: String,
    category: String,
}

impl ProjectConfig {
    pub fn load(project_dir: &Path, explicit: Option<&Path>) -> Result<ProjectConfig> {
        let path = match explicit {
            Some(p) => p.to_path_buf(),
            None => project_dir.join(CONFIG_FILE),
        };
        if !path.is_file() {
            if explicit.is_some() {
                return Err(anyhow!("config file {} not found", path.display()));
            }
            return Ok(ProjectConfig::default());
        }
        let src = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let file: ConfigFile = toml::from_str(&src).map_err(|e| anyhow!("{}: {e}", path.display()))?;
        if !file.include.is_empty() {
            return Err(anyhow!("{}: `include` is not supported yet; list what to leave out under `exclude`", path.display()));
        }
        let base = path.parent().unwrap_or(project_dir);
        let categories = file
            .categories
            .into_iter()
            .map(|c| {
                let category = Category::parse(&c.category).ok_or_else(|| anyhow!("{}: unknown category `{}`", path.display(), c.category))?;
                Ok(CategoryOverride { glob: c.glob, category })
            })
            .collect::<Result<_>>()?;
        Ok(ProjectConfig {
            name: file.name,
            include: file.include,
            exclude: file.exclude,
            packs: PackSelection { enable: file.packs.enable, disable: file.packs.disable, extra_dirs: file.packs.dirs.into_iter().map(|d| base.join(d)).collect() },
            categories,
            index: file.index.map(|i| base.join(i)),
        })
    }

    pub fn config_pack(&self) -> Option<Pack> {
        if self.categories.is_empty() {
            return None;
        }
        let mut pack = Pack::parse("name = \"project-config\"\nversion = \"0\"\n", "project config", None).ok()?;
        for (i, c) in self.categories.iter().enumerate() {
            pack.rules.push((format!("project-config.category.{i}"), Rule::Path { glob: c.glob.clone(), category: c.category }));
        }
        pack.hash = blake3::hash(format!("{:?}", self.categories).as_bytes()).to_hex()[..16].to_string();
        Some(pack)
    }
}
