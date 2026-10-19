use crate::cache::project_key;
use crate::config::{BUILTIN_EXCLUDES, CONFIG_FILE, CategoryOverride, Filter, ProjectConfig};
use anyhow::{Context, Result, anyhow, bail};
use codescape_format::Category;
use codescape_rules::PackSelection;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layer {
    Default,
    User,
    Shared,
    Personal,
}

impl Layer {
    pub fn label(self) -> &'static str {
        match self {
            Layer::Default => "default",
            Layer::User => "your defaults",
            Layer::Shared => CONFIG_FILE,
            Layer::Personal => "this project",
        }
    }
}

const FILE_LAYERS: [Layer; 3] = [Layer::User, Layer::Shared, Layer::Personal];
const PERSONAL: usize = 2;

fn slot(layer: Layer) -> Result<usize> {
    FILE_LAYERS.iter().position(|&l| l == layer).ok_or_else(|| anyhow!("the built-in defaults cannot be changed"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum View {
    Tests,
    TestDeps,
    Details,
    Errors,
    Logging,
    Wiring,
    Complexity,
    ComplexityColour,
    Unused,
    QuietArrows,
    Legend,
    Generated,
    PrivateMembers,
    Fields,
    Accessors,
    Constructors,
    GuessedCalls,
    ImplementsArrows,
    ImportArrows,
}

impl View {
    pub const ALL: [View; 19] = [
        View::Tests,
        View::TestDeps,
        View::Details,
        View::Errors,
        View::Logging,
        View::Wiring,
        View::Complexity,
        View::ComplexityColour,
        View::Unused,
        View::QuietArrows,
        View::Legend,
        View::Generated,
        View::PrivateMembers,
        View::Fields,
        View::Accessors,
        View::Constructors,
        View::GuessedCalls,
        View::ImplementsArrows,
        View::ImportArrows,
    ];

    pub fn default_value(self) -> bool {
        !matches!(
            self,
            View::Tests
                | View::TestDeps
                | View::Details
                | View::Errors
                | View::Logging
                | View::Wiring
                | View::ComplexityColour
                | View::Unused
                | View::Generated
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            View::Tests => "Tests",
            View::TestDeps => "Test dependencies",
            View::Details => "Details",
            View::Errors => "Errors",
            View::Logging => "Logging",
            View::Wiring => "Wiring",
            View::Complexity => "Complexity hints",
            View::ComplexityColour => "Colour by complexity",
            View::Unused => "Unused code",
            View::QuietArrows => "Quiet arrows",
            View::Legend => "Legend",
            View::Generated => "Generated code",
            View::PrivateMembers => "Private members",
            View::Fields => "Fields",
            View::Accessors => "Getters and setters",
            View::Constructors => "Constructors",
            View::GuessedCalls => "Guessed calls",
            View::ImplementsArrows => "Implements / extends",
            View::ImportArrows => "Imports without calls",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Threshold {
    ComplexityHint,
    ComplexityHigh,
    ManyCallers,
    KeepVersionsDays,
    DuplicateTokens,
    HotspotDays,
}

impl Threshold {
    pub const ALL: [Threshold; 6] = [
        Threshold::ComplexityHint,
        Threshold::ComplexityHigh,
        Threshold::ManyCallers,
        Threshold::KeepVersionsDays,
        Threshold::DuplicateTokens,
        Threshold::HotspotDays,
    ];

    pub fn default_value(self) -> u32 {
        match self {
            Threshold::ComplexityHint => 10,
            Threshold::ComplexityHigh => 20,
            Threshold::ManyCallers => 5,
            Threshold::KeepVersionsDays => 14,
            Threshold::DuplicateTokens => 50,
            Threshold::HotspotDays => 365,
        }
    }

    pub fn min(self) -> u32 {
        match self {
            Threshold::DuplicateTokens => 10,
            _ => 1,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Threshold::ComplexityHint => "Complexity hint from",
            Threshold::ComplexityHigh => "High complexity from",
            Threshold::ManyCallers => "Many callers from",
            Threshold::KeepVersionsDays => "Keep compared versions (days)",
            Threshold::DuplicateTokens => "Duplicates from (tokens)",
            Threshold::HotspotDays => "Hotspot period (days)",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    View(View),
    Threshold(Threshold),
    Hidden(String),
    HiddenName(String),
    HiddenSupertype(String),
    AlwaysShown(String),
    FilterShown(String),
    Exclude(String),
    Pack(String),
    Category(String),
    TestPath(String),
    Filter(String),
}

impl Key {
    pub fn is_display(&self) -> bool {
        matches!(
            self,
            Key::View(_) | Key::Threshold(_) | Key::Hidden(_) | Key::HiddenName(_) | Key::HiddenSupertype(_) | Key::AlwaysShown(_) | Key::FilterShown(_)
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sourced<T> {
    pub value: T,
    pub layer: Layer,
}

fn at<T>(value: T, layer: Layer) -> Sourced<T> {
    Sourced { value, layer }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct SettingsFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    index: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    include: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    exclude: Vec<String>,
    #[serde(default, skip_serializing_if = "PacksFile::is_empty")]
    packs: PacksFile,
    #[serde(default, skip_serializing_if = "TestFile::is_empty")]
    test: TestFile,
    #[serde(default, skip_serializing_if = "ViewFile::is_empty")]
    view: ViewFile,
    #[serde(default, skip_serializing_if = "RemoveFile::is_empty")]
    remove: RemoveFile,
    #[serde(default, rename = "category", skip_serializing_if = "Vec::is_empty")]
    categories: Vec<CategoryFile>,
    #[serde(default, rename = "filter", skip_serializing_if = "Vec::is_empty")]
    filters: Vec<FilterFile>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(deny_unknown_fields)]
struct PacksFile {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    enable: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    disable: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    dirs: Vec<PathBuf>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(deny_unknown_fields)]
struct TestFile {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    paths: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(deny_unknown_fields)]
struct ViewFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tests: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    test_deps: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    details: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    errors: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    logging: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    wiring: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    complexity: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    complexity_colour: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unused: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    quiet_arrows: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    legend: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    generated: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    private_members: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fields: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    accessors: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    constructors: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    guessed_calls: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    implements_arrows: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    import_arrows: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    complexity_hint: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    complexity_high: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    many_callers: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    keep_versions_days: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    duplicate_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hotspot_days: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    hidden: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    hidden_names: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    hidden_supertypes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    always_shown: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    shown_again: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    filters: BTreeMap<String, bool>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(deny_unknown_fields)]
struct RemoveFile {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    exclude: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    category: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    test_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    filter: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
struct CategoryFile {
    glob: String,
    category: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
struct FilterFile {
    name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    names: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shown: Option<bool>,
}

macro_rules! is_default {
    ($($t:ty),+) => {$(
        impl $t {
            fn is_empty(&self) -> bool {
                *self == Self::default()
            }
        }
    )+};
}

is_default!(PacksFile, TestFile, ViewFile, RemoveFile);

impl ViewFile {
    fn slot(&mut self, v: View) -> &mut Option<bool> {
        match v {
            View::Tests => &mut self.tests,
            View::TestDeps => &mut self.test_deps,
            View::Details => &mut self.details,
            View::Errors => &mut self.errors,
            View::Logging => &mut self.logging,
            View::Wiring => &mut self.wiring,
            View::Complexity => &mut self.complexity,
            View::ComplexityColour => &mut self.complexity_colour,
            View::Unused => &mut self.unused,
            View::QuietArrows => &mut self.quiet_arrows,
            View::Legend => &mut self.legend,
            View::Generated => &mut self.generated,
            View::PrivateMembers => &mut self.private_members,
            View::Fields => &mut self.fields,
            View::Accessors => &mut self.accessors,
            View::Constructors => &mut self.constructors,
            View::GuessedCalls => &mut self.guessed_calls,
            View::ImplementsArrows => &mut self.implements_arrows,
            View::ImportArrows => &mut self.import_arrows,
        }
    }

    fn get(&self, v: View) -> Option<bool> {
        self.clone().slot(v).take()
    }

    fn threshold_slot(&mut self, t: Threshold) -> &mut Option<u32> {
        match t {
            Threshold::ComplexityHint => &mut self.complexity_hint,
            Threshold::ComplexityHigh => &mut self.complexity_high,
            Threshold::ManyCallers => &mut self.many_callers,
            Threshold::KeepVersionsDays => &mut self.keep_versions_days,
            Threshold::DuplicateTokens => &mut self.duplicate_tokens,
            Threshold::HotspotDays => &mut self.hotspot_days,
        }
    }

    fn threshold(&self, t: Threshold) -> Option<u32> {
        self.clone().threshold_slot(t).take()
    }
}

fn drain_by<T>(v: &mut Vec<T>, f: impl Fn(&T) -> bool) -> Vec<T> {
    let (out, keep) = std::mem::take(v).into_iter().partition(|x| f(x));
    *v = keep;
    out
}

fn drain_eq(v: &mut Vec<String>, s: &str) -> Vec<String> {
    drain_by(v, |x| x == s)
}

impl SettingsFile {
    pub(crate) fn read(path: &Path) -> Result<SettingsFile> {
        if !path.is_file() {
            return Ok(SettingsFile::default());
        }
        let src = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&src).map_err(|e| anyhow!("{}: {e}", path.display()))
    }

    fn take(&mut self, key: &Key) -> SettingsFile {
        let mut out = SettingsFile::default();
        match key {
            Key::View(v) => *out.view.slot(*v) = self.view.slot(*v).take(),
            Key::Threshold(t) => *out.view.threshold_slot(*t) = self.view.threshold_slot(*t).take(),
            Key::Hidden(p) => {
                out.view.hidden = drain_eq(&mut self.view.hidden, p);
                out.view.shown_again = drain_eq(&mut self.view.shown_again, p);
            }
            Key::HiddenName(p) => {
                out.view.hidden_names = drain_eq(&mut self.view.hidden_names, p);
                out.view.shown_again = drain_eq(&mut self.view.shown_again, p);
            }
            Key::HiddenSupertype(p) => {
                out.view.hidden_supertypes = drain_eq(&mut self.view.hidden_supertypes, p);
                out.view.shown_again = drain_eq(&mut self.view.shown_again, p);
            }
            Key::AlwaysShown(p) => out.view.always_shown = drain_eq(&mut self.view.always_shown, p),
            Key::FilterShown(n) => out.view.filters.extend(self.view.filters.remove_entry(n)),
            Key::Exclude(g) => {
                out.exclude = drain_eq(&mut self.exclude, g);
                out.remove.exclude = drain_eq(&mut self.remove.exclude, g);
            }
            Key::Pack(n) => {
                out.packs.enable = drain_eq(&mut self.packs.enable, n);
                out.packs.disable = drain_eq(&mut self.packs.disable, n);
            }
            Key::Category(g) => {
                out.categories = drain_by(&mut self.categories, |c| &c.glob == g);
                out.remove.category = drain_eq(&mut self.remove.category, g);
            }
            Key::TestPath(p) => {
                out.test.paths = drain_eq(&mut self.test.paths, p);
                out.remove.test_paths = drain_eq(&mut self.remove.test_paths, p);
            }
            Key::Filter(n) => {
                out.filters = drain_by(&mut self.filters, |f| &f.name == n);
                out.remove.filter = drain_eq(&mut self.remove.filter, n);
            }
        }
        out
    }

    fn put(&mut self, frag: SettingsFile) {
        for v in View::ALL {
            if let Some(b) = frag.view.get(v) {
                *self.view.slot(v) = Some(b);
            }
        }
        for t in Threshold::ALL {
            if let Some(n) = frag.view.threshold(t) {
                *self.view.threshold_slot(t) = Some(n);
            }
        }
        self.view.hidden.extend(frag.view.hidden);
        self.view.hidden_names.extend(frag.view.hidden_names);
        self.view.hidden_supertypes.extend(frag.view.hidden_supertypes);
        self.view.always_shown.extend(frag.view.always_shown);
        self.view.shown_again.extend(frag.view.shown_again);
        self.view.filters.extend(frag.view.filters);
        self.exclude.extend(frag.exclude);
        self.remove.exclude.extend(frag.remove.exclude);
        self.packs.enable.extend(frag.packs.enable);
        self.packs.disable.extend(frag.packs.disable);
        self.categories.extend(frag.categories);
        self.remove.category.extend(frag.remove.category);
        self.test.paths.extend(frag.test.paths);
        self.remove.test_paths.extend(frag.remove.test_paths);
        self.filters.extend(frag.filters);
        self.remove.filter.extend(frag.remove.filter);
    }

    fn removal(key: &Key) -> SettingsFile {
        let mut out = SettingsFile::default();
        match key {
            Key::Exclude(g) => out.remove.exclude.push(g.clone()),
            Key::Category(g) => out.remove.category.push(g.clone()),
            Key::TestPath(p) => out.remove.test_paths.push(p.clone()),
            Key::Filter(n) => out.remove.filter.push(n.clone()),
            Key::Hidden(p) | Key::HiddenName(p) | Key::HiddenSupertype(p) => out.view.shown_again.push(p.clone()),
            Key::View(_) | Key::Threshold(_) | Key::AlwaysShown(_) | Key::FilterShown(_) | Key::Pack(_) => {}
        }
        out
    }

    fn without_view(&self) -> SettingsFile {
        SettingsFile { view: ViewFile::default(), ..self.clone() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub name: Option<Sourced<String>>,
    pub index: Option<Sourced<PathBuf>>,
    pub exclude: Vec<Sourced<String>>,
    pub packs: Vec<Sourced<(String, bool)>>,
    pub pack_dirs: Vec<Sourced<PathBuf>>,
    pub categories: Vec<Sourced<CategoryOverride>>,
    pub test_paths: Vec<Sourced<String>>,
    pub filters: Vec<Sourced<Filter>>,
    pub views: [Sourced<bool>; View::ALL.len()],
    pub thresholds: [Sourced<u32>; Threshold::ALL.len()],
    pub hidden: Vec<Sourced<String>>,
    pub hidden_names: Vec<Sourced<String>>,
    pub hidden_supertypes: Vec<Sourced<String>>,
    pub always_shown: Vec<Sourced<String>>,
    pub filter_views: BTreeMap<String, Sourced<bool>>,
    pub removed: Vec<Sourced<Key>>,
}

fn check_glob(g: &str) -> Result<()> {
    globset::Glob::new(g).map(|_| ()).with_context(|| format!("bad glob `{g}`"))
}

impl Settings {
    fn builtin() -> Settings {
        Settings {
            name: None,
            index: None,
            exclude: BUILTIN_EXCLUDES.iter().map(|g| at(g.to_string(), Layer::Default)).collect(),
            packs: Vec::new(),
            pack_dirs: Vec::new(),
            categories: Vec::new(),
            test_paths: Vec::new(),
            filters: Vec::new(),
            views: View::ALL.map(|v| at(v.default_value(), Layer::Default)),
            thresholds: Threshold::ALL.map(|t| at(t.default_value(), Layer::Default)),
            hidden: Vec::new(),
            hidden_names: Vec::new(),
            hidden_supertypes: Vec::new(),
            always_shown: Vec::new(),
            filter_views: BTreeMap::new(),
            removed: Vec::new(),
        }
    }

    pub(crate) fn merge(layers: &[(Layer, &SettingsFile, PathBuf, PathBuf)]) -> Result<Settings> {
        let mut s = Settings::builtin();
        for (layer, file, path, base) in layers {
            s.absorb(*layer, file, base).map_err(|e| anyhow!("{}: {e:#}", path.display()))?;
        }
        Ok(s)
    }

    fn drop_entry(&mut self, key: Key, layer: Layer) {
        match &key {
            Key::Exclude(g) => self.exclude.retain(|e| &e.value != g),
            Key::Category(g) => self.categories.retain(|c| &c.value.glob != g),
            Key::TestPath(p) => self.test_paths.retain(|t| &t.value != p),
            Key::Filter(n) => self.filters.retain(|f| &f.value.name != n),
            Key::Hidden(p) => self.hidden.retain(|h| &h.value != p),
            Key::HiddenName(p) => self.hidden_names.retain(|h| &h.value != p),
            Key::HiddenSupertype(p) => self.hidden_supertypes.retain(|h| &h.value != p),
            Key::AlwaysShown(p) => self.always_shown.retain(|h| &h.value != p),
            Key::View(_) | Key::Threshold(_) | Key::FilterShown(_) | Key::Pack(_) => {}
        }
        self.removed.retain(|r| r.value != key);
        self.removed.push(at(key, layer));
    }

    fn readd(&mut self, key: Key) {
        self.drop_entry(key.clone(), Layer::Default);
        self.removed.retain(|r| r.value != key);
    }

    fn absorb(&mut self, layer: Layer, f: &SettingsFile, base: &Path) -> Result<()> {
        if !f.include.is_empty() {
            bail!("`include` is not supported yet; list what to leave out under `exclude`");
        }
        if let Some(n) = &f.name {
            self.name = Some(at(n.clone(), layer));
        }
        if let Some(i) = &f.index {
            self.index = Some(at(base.join(i), layer));
        }
        for g in &f.remove.exclude {
            self.drop_entry(Key::Exclude(g.clone()), layer);
        }
        for g in &f.exclude {
            check_glob(g)?;
            self.readd(Key::Exclude(g.clone()));
            self.exclude.push(at(g.clone(), layer));
        }
        for (names, on) in [(&f.packs.disable, false), (&f.packs.enable, true)] {
            for n in names {
                self.packs.retain(|p| &p.value.0 != n);
                self.packs.push(at((n.clone(), on), layer));
            }
        }
        self.pack_dirs.extend(f.packs.dirs.iter().map(|d| at(base.join(d), layer)));
        for g in &f.remove.category {
            self.drop_entry(Key::Category(g.clone()), layer);
        }
        for c in &f.categories {
            check_glob(&c.glob)?;
            let category = Category::parse(&c.category).ok_or_else(|| anyhow!("unknown category `{}` (application, dev-tool, test)", c.category))?;
            self.readd(Key::Category(c.glob.clone()));
            self.categories.push(at(CategoryOverride { glob: c.glob.clone(), category }, layer));
        }
        for p in &f.remove.test_paths {
            self.drop_entry(Key::TestPath(p.clone()), layer);
        }
        for p in &f.test.paths {
            check_glob(p)?;
            self.readd(Key::TestPath(p.clone()));
            self.test_paths.push(at(p.clone(), layer));
        }
        for n in &f.remove.filter {
            self.drop_entry(Key::Filter(n.clone()), layer);
        }
        for fl in &f.filters {
            if fl.name.trim().is_empty() {
                bail!("a filter needs a name");
            }
            for p in &fl.paths {
                check_glob(p).with_context(|| format!("filter `{}`", fl.name))?;
            }
            for r in &fl.names {
                regex::Regex::new(r).with_context(|| format!("filter `{}`: bad name pattern `{r}`", fl.name))?;
            }
            self.readd(Key::Filter(fl.name.clone()));
            self.filters.push(at(Filter { name: fl.name.clone(), paths: fl.paths.clone(), names: fl.names.clone(), shown: fl.shown.unwrap_or(true) }, layer));
        }
        for v in View::ALL {
            if let Some(b) = f.view.get(v) {
                self.views[v as usize] = at(b, layer);
            }
        }
        for (n, b) in &f.view.filters {
            self.filter_views.insert(n.clone(), at(*b, layer));
        }
        for t in Threshold::ALL {
            if let Some(n) = f.view.threshold(t) {
                self.thresholds[t as usize] = at(n.max(t.min()), layer);
            }
        }
        for p in &f.view.shown_again {
            self.hidden.retain(|h| &h.value != p);
            self.hidden_names.retain(|h| &h.value != p);
            self.hidden_supertypes.retain(|h| &h.value != p);
        }
        for (key, patterns) in [("hidden_names", &f.view.hidden_names), ("hidden_supertypes", &f.view.hidden_supertypes)] {
            for p in patterns {
                regex::Regex::new(p).with_context(|| format!("bad name pattern `{p}` under [view] {key}"))?;
            }
        }
        for (list, entries) in [
            (&mut self.hidden, &f.view.hidden),
            (&mut self.hidden_names, &f.view.hidden_names),
            (&mut self.hidden_supertypes, &f.view.hidden_supertypes),
            (&mut self.always_shown, &f.view.always_shown),
        ] {
            for p in entries {
                list.retain(|h| &h.value != p);
                list.push(at(p.clone(), layer));
            }
        }
        Ok(())
    }

    pub fn view(&self, v: View) -> Sourced<bool> {
        self.views[v as usize]
    }

    pub fn threshold(&self, t: Threshold) -> Sourced<u32> {
        self.thresholds[t as usize]
    }

    pub fn pack(&self, name: &str) -> Option<Sourced<bool>> {
        self.packs.iter().find(|p| p.value.0 == name).map(|p| at(p.value.1, p.layer))
    }

    pub fn filter_shown(&self, name: &str) -> Option<Sourced<bool>> {
        self.filter_views.get(name).copied().or_else(|| self.filters.iter().find(|f| f.value.name == name).map(|f| at(f.value.shown, f.layer)))
    }

    fn has(&self, key: &Key) -> bool {
        match key {
            Key::Exclude(g) => self.exclude.iter().any(|e| &e.value == g),
            Key::Category(g) => self.categories.iter().any(|c| &c.value.glob == g),
            Key::TestPath(p) => self.test_paths.iter().any(|t| &t.value == p),
            Key::Filter(n) => self.filters.iter().any(|f| &f.value.name == n),
            Key::Hidden(p) => self.hidden.iter().any(|h| &h.value == p),
            Key::HiddenName(p) => self.hidden_names.iter().any(|h| &h.value == p),
            Key::HiddenSupertype(p) => self.hidden_supertypes.iter().any(|h| &h.value == p),
            Key::AlwaysShown(p) => self.always_shown.iter().any(|h| &h.value == p),
            Key::View(_) | Key::Threshold(_) | Key::FilterShown(_) | Key::Pack(_) => false,
        }
    }

    pub fn config(&self) -> ProjectConfig {
        let packs = |on: bool| self.packs.iter().filter(|p| p.value.1 == on).map(|p| p.value.0.clone()).collect();
        ProjectConfig {
            name: self.name.as_ref().map(|n| n.value.clone()),
            include: Vec::new(),
            exclude: self.exclude.iter().map(|e| e.value.clone()).collect(),
            packs: PackSelection { enable: packs(true), disable: packs(false), extra_dirs: self.pack_dirs.iter().map(|d| d.value.clone()).collect() },
            categories: self.categories.iter().map(|c| c.value.clone()).collect(),
            test_paths: self.test_paths.iter().map(|t| t.value.clone()).collect(),
            filters: self.filters.iter().map(|f| f.value.clone()).collect(),
            index: self.index.as_ref().map(|i| i.value.clone()),
            duplicate_tokens: self.threshold(Threshold::DuplicateTokens).value,
        }
    }
}

pub fn config_root() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("codescape")
}

#[derive(Clone, Debug, PartialEq)]
pub struct SettingsPaths {
    pub project: PathBuf,
    pub user: PathBuf,
    pub shared: PathBuf,
    pub personal: PathBuf,
    pub explicit: bool,
}

impl SettingsPaths {
    pub fn new(project_dir: &Path, explicit: Option<&Path>) -> SettingsPaths {
        SettingsPaths::with_root(project_dir, explicit, &config_root())
    }

    pub fn with_root(project_dir: &Path, explicit: Option<&Path>, root: &Path) -> SettingsPaths {
        SettingsPaths {
            project: project_dir.to_path_buf(),
            user: root.join("settings.toml"),
            shared: explicit.map_or_else(|| project_dir.join(CONFIG_FILE), Path::to_path_buf),
            personal: root.join("projects").join(format!("{}.toml", project_key(project_dir))),
            explicit: explicit.is_some(),
        }
    }

    pub fn file(&self, layer: Layer) -> Option<&Path> {
        match layer {
            Layer::Default => None,
            Layer::User => Some(&self.user),
            Layer::Shared => Some(&self.shared),
            Layer::Personal => Some(&self.personal),
        }
    }

    fn base(&self, layer: Layer) -> PathBuf {
        match layer {
            Layer::Shared => self.shared.parent().unwrap_or(&self.project).to_path_buf(),
            _ => self.project.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SettingsStore {
    pub paths: SettingsPaths,
    saved: [SettingsFile; 3],
    draft: [SettingsFile; 3],
    settings: Settings,
}

fn standard(item: &toml_edit::Item) -> toml_edit::Item {
    use toml_edit::{ArrayOfTables, Item, Value};
    match item {
        Item::Value(Value::InlineTable(t)) => Item::Table(t.clone().into_table()),
        Item::Value(Value::Array(a)) if !a.is_empty() && a.iter().all(|v| v.is_inline_table()) => {
            let mut out = ArrayOfTables::new();
            for v in a.iter().filter_map(|v| v.as_inline_table()) {
                out.push(v.clone().into_table());
            }
            Item::ArrayOfTables(out)
        }
        other => other.clone(),
    }
}

fn write_layer(path: &Path, old: &SettingsFile, new: &SettingsFile) -> Result<()> {
    if *new == SettingsFile::default() && !path.exists() {
        return Ok(());
    }
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| anyhow!("{}: {e}", path.display()))?;
    let fresh = toml_edit::ser::to_document(new)?;
    let (before, after) = (toml::Table::try_from(old)?, toml::Table::try_from(new)?);
    for (k, v) in &after {
        if (before.get(k) != Some(v) || !doc.contains_key(k))
            && let Some(item) = fresh.get(k)
        {
            doc.insert(k, standard(item));
        }
    }
    for k in before.keys().filter(|k| !after.contains_key(*k)) {
        doc.remove(k);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, doc.to_string()).with_context(|| format!("writing {}", path.display()))
}

impl SettingsStore {
    pub fn load(paths: SettingsPaths) -> Result<SettingsStore> {
        if paths.explicit && !paths.shared.is_file() {
            bail!("config file {} not found", paths.shared.display());
        }
        let saved = [SettingsFile::read(&paths.user)?, SettingsFile::read(&paths.shared)?, SettingsFile::read(&paths.personal)?];
        let settings = SettingsStore::merged(&paths, &saved)?;
        Ok(SettingsStore { paths, draft: saved.clone(), saved, settings })
    }

    fn merged(paths: &SettingsPaths, files: &[SettingsFile; 3]) -> Result<Settings> {
        let layers: Vec<_> = FILE_LAYERS.iter().zip(files).map(|(&l, f)| (l, f, paths.file(l).unwrap().to_path_buf(), paths.base(l))).collect();
        Settings::merge(&layers)
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn config(&self) -> ProjectConfig {
        self.settings.config()
    }

    fn edit(&mut self, f: impl FnOnce(&mut [SettingsFile; 3])) -> Result<()> {
        let mut next = self.draft.clone();
        f(&mut next);
        self.commit(next)
    }

    fn commit(&mut self, next: [SettingsFile; 3]) -> Result<()> {
        self.settings = SettingsStore::merged(&self.paths, &next)?;
        self.draft = next;
        Ok(())
    }

    pub fn set_view(&mut self, v: View, on: bool) {
        let _ = self.edit(|f| *f[PERSONAL].view.slot(v) = Some(on));
    }

    pub fn set_threshold(&mut self, t: Threshold, value: u32) {
        let _ = self.edit(|f| *f[PERSONAL].view.threshold_slot(t) = Some(value));
    }

    pub fn hide(&mut self, path: &str) -> Result<()> {
        self.replace(Key::Hidden(path.into()), SettingsFile { view: ViewFile { hidden: vec![path.into()], ..Default::default() }, ..Default::default() })
    }

    pub fn hide_names(&mut self, pattern: &str) -> Result<()> {
        regex::Regex::new(pattern).with_context(|| format!("bad name pattern `{pattern}`"))?;
        let frag = SettingsFile { view: ViewFile { hidden_names: vec![pattern.into()], ..Default::default() }, ..Default::default() };
        self.replace(Key::HiddenName(pattern.into()), frag)
    }

    pub fn hide_supertypes(&mut self, pattern: &str) -> Result<()> {
        regex::Regex::new(pattern).with_context(|| format!("bad name pattern `{pattern}`"))?;
        let frag = SettingsFile { view: ViewFile { hidden_supertypes: vec![pattern.into()], ..Default::default() }, ..Default::default() };
        self.replace(Key::HiddenSupertype(pattern.into()), frag)
    }

    pub fn always_show(&mut self, path: &str) -> Result<()> {
        let frag = SettingsFile { view: ViewFile { always_shown: vec![path.into()], ..Default::default() }, ..Default::default() };
        self.replace(Key::AlwaysShown(path.into()), frag)
    }

    pub fn unhide(&mut self, entry: &str) -> Result<()> {
        if self.settings.hidden.iter().any(|h| h.value == entry) {
            self.remove_entry(Key::Hidden(entry.into()))
        } else if self.settings.hidden_supertypes.iter().any(|h| h.value == entry) {
            self.remove_entry(Key::HiddenSupertype(entry.into()))
        } else {
            self.remove_entry(Key::HiddenName(entry.into()))
        }
    }

    pub fn stop_always_showing(&mut self, path: &str) -> Result<()> {
        self.remove_entry(Key::AlwaysShown(path.into()))
    }

    pub fn set_filter_shown(&mut self, name: &str, on: bool) {
        let _ = self.edit(|f| {
            f[PERSONAL].view.filters.insert(name.to_string(), on);
        });
    }

    fn replace(&mut self, key: Key, frag: SettingsFile) -> Result<()> {
        self.edit(|f| {
            f[PERSONAL].take(&key);
            f[PERSONAL].put(frag);
        })
    }

    pub fn add_exclude(&mut self, glob: &str) -> Result<()> {
        self.replace(Key::Exclude(glob.into()), SettingsFile { exclude: vec![glob.into()], ..Default::default() })
    }

    pub fn add_test_path(&mut self, glob: &str) -> Result<()> {
        self.replace(Key::TestPath(glob.into()), SettingsFile { test: TestFile { paths: vec![glob.into()] }, ..Default::default() })
    }

    pub fn add_category(&mut self, c: CategoryOverride) -> Result<()> {
        let frag = SettingsFile { categories: vec![CategoryFile { glob: c.glob.clone(), category: c.category.label().into() }], ..Default::default() };
        self.replace(Key::Category(c.glob), frag)
    }

    pub fn put_filter(&mut self, fl: Filter) -> Result<()> {
        let file = FilterFile { name: fl.name.clone(), paths: fl.paths, names: fl.names, shown: Some(fl.shown) };
        self.replace(Key::Filter(fl.name), SettingsFile { filters: vec![file], ..Default::default() })
    }

    pub fn set_pack(&mut self, name: &str, on: bool) -> Result<()> {
        let packs =
            if on { PacksFile { enable: vec![name.into()], ..Default::default() } } else { PacksFile { disable: vec![name.into()], ..Default::default() } };
        self.replace(Key::Pack(name.into()), SettingsFile { packs, ..Default::default() })
    }

    fn remove_entry(&mut self, key: Key) -> Result<()> {
        let mut next = self.draft.clone();
        next[PERSONAL].take(&key);
        if SettingsStore::merged(&self.paths, &next)?.has(&key) {
            next[PERSONAL].put(SettingsFile::removal(&key));
        }
        self.commit(next)
    }

    pub fn remove_exclude(&mut self, glob: &str) -> Result<()> {
        self.remove_entry(Key::Exclude(glob.into()))
    }

    pub fn remove_test_path(&mut self, glob: &str) -> Result<()> {
        self.remove_entry(Key::TestPath(glob.into()))
    }

    pub fn remove_category(&mut self, glob: &str) -> Result<()> {
        self.remove_entry(Key::Category(glob.into()))
    }

    pub fn remove_filter(&mut self, name: &str) -> Result<()> {
        self.remove_entry(Key::Filter(name.into()))
    }

    pub fn reset(&mut self, key: &Key) -> Result<()> {
        self.edit(|f| {
            f[PERSONAL].take(key);
        })
    }

    pub fn promote(&mut self, key: &Key, to: Layer) -> Result<()> {
        let to = slot(to)?;
        if to == PERSONAL {
            return Ok(());
        }
        self.edit(|f| {
            let frag = f[PERSONAL].take(key);
            f[to].take(key);
            f[to].put(frag);
        })
    }

    pub fn index_dirty(&self) -> bool {
        self.saved.iter().zip(&self.draft).any(|(s, d)| s.without_view() != d.without_view())
    }

    pub fn save_view(&mut self) -> Result<()> {
        for (i, &layer) in FILE_LAYERS.iter().enumerate() {
            if self.saved[i].view != self.draft[i].view {
                let next = SettingsFile { view: self.draft[i].view.clone(), ..self.saved[i].clone() };
                write_layer(self.paths.file(layer).unwrap(), &self.saved[i], &next)?;
                self.saved[i] = next;
            }
        }
        Ok(())
    }

    pub fn apply(&mut self) -> Result<()> {
        for (i, &layer) in FILE_LAYERS.iter().enumerate() {
            if self.saved[i] != self.draft[i] {
                write_layer(self.paths.file(layer).unwrap(), &self.saved[i], &self.draft[i])?;
                self.saved[i] = self.draft[i].clone();
            }
        }
        Ok(())
    }

    pub fn discard(&mut self) {
        let mut next = self.saved.clone();
        for (n, d) in next.iter_mut().zip(&self.draft) {
            n.view = d.view.clone();
        }
        let _ = self.commit(next);
    }
}
