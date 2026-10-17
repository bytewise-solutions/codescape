use crate::visual::{EntryGroup, FrameRule, TagStyle, VisualRules};
use anyhow::{Context, Result, anyhow, bail};
use codescape_format::{Category, Lang, Lifecycle};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const BUILTINS: &[(&str, &str)] = &[
    ("java-core", include_str!("../../../rules/java-core/pack.toml")),
    ("libgdx", include_str!("../../../rules/libgdx/pack.toml")),
    ("spring-boot", include_str!("../../../rules/spring-boot/pack.toml")),
    ("dagger", include_str!("../../../rules/dagger/pack.toml")),
    ("go-core", include_str!("../../../rules/go-core/pack.toml")),
    ("fiber", include_str!("../../../rules/fiber/pack.toml")),
    ("rabbitmq-amqp091", include_str!("../../../rules/rabbitmq-amqp091/pack.toml")),
    ("google-wire", include_str!("../../../rules/google-wire/pack.toml")),
    ("ts-core", include_str!("../../../rules/ts-core/pack.toml")),
    ("angular", include_str!("../../../rules/angular/pack.toml")),
    ("express", include_str!("../../../rules/express/pack.toml")),
    ("rust-core", include_str!("../../../rules/rust-core/pack.toml")),
    ("layers", include_str!("../../../rules/layers/pack.toml")),
    ("significance", include_str!("../../../rules/significance/pack.toml")),
    ("default-visuals", include_str!("../../../rules/default-visuals/pack.toml")),
];

#[derive(Clone, Debug, PartialEq)]
pub struct Callback {
    pub method: String,
    pub lifecycle: Lifecycle,
    pub order: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Rule {
    Path {
        glob: String,
        category: Category,
    },
    Main {
        category: Category,
        label: String,
    },
    MainCall {
        methods: Vec<String>,
        file_imports: String,
        label: String,
        handoff: String,
        category: Category,
    },
    Supertype {
        types: Vec<String>,
        callbacks: Vec<Callback>,
    },
    Annotation {
        names: Vec<String>,
        handoff: String,
    },
    AnnotationRoute {
        names: Vec<String>,
        method: String,
        prefix_from_type: Vec<String>,
    },
    CallRoute {
        methods: Vec<String>,
        group_methods: Vec<String>,
        receiver_types: Vec<String>,
        file_imports: String,
        path_arg: usize,
        handler_last: bool,
        label: String,
    },
    CallHandler {
        methods: Vec<String>,
        receiver_types: Vec<String>,
        file_imports: String,
        kind: String,
        label: String,
    },
    Instantiates {
        types: Vec<String>,
        handoff: String,
    },
    Dispatch {
        from_override: String,
        to_override: String,
    },
    NameTag {
        kinds: Vec<String>,
        pattern: String,
        tag: String,
    },
    Query {
        language: Lang,
        query: String,
        capture: String,
        kind: String,
        label: String,
    },
    Wiring {
        file_markers: Vec<String>,
        file_imports: Vec<String>,
        calls: Vec<String>,
        annotations: Vec<String>,
        type_prefixes: Vec<String>,
        requires_imports: Vec<String>,
        caller_prefixes: Vec<String>,
    },
    ObjectRoute {
        var_types: Vec<String>,
        calls: Vec<String>,
        path_key: String,
        target_keys: Vec<String>,
        children_key: String,
        kind: String,
        label: String,
    },
    TestCode {
        file_imports: Vec<String>,
        dir_names: Vec<String>,
        paths: Vec<String>,
    },
    Filter {
        tag: String,
        paths: Vec<String>,
        names: Vec<String>,
    },
    Noise {
        name_suffixes: Vec<String>,
        supertype_suffixes: Vec<String>,
        methods: Vec<String>,
    },
    AnnotationTag {
        names: Vec<String>,
        tag: String,
    },
    EntryOwnerTag {
        kinds: Vec<String>,
        tag: String,
    },
    Significance {
        component_annotations: Vec<String>,
        constructor_annotations: Vec<String>,
        accessor_pattern: String,
        detail_packages: String,
        hub_min_packages: usize,
    },
}

#[derive(Clone, Debug)]
pub struct Pack {
    pub name: String,
    pub version: String,
    pub languages: Vec<Lang>,
    pub rules: Vec<(String, Rule)>,
    pub visual: VisualRules,
    pub hash: String,
    pub dir: Option<PathBuf>,
}

#[derive(Clone, Debug, Default)]
pub struct PackSelection {
    pub enable: Vec<String>,
    pub disable: Vec<String>,
    pub extra_dirs: Vec<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct Packs {
    packs: Vec<Pack>,
    effective: Vec<(String, usize, Rule)>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PackFile {
    name: String,
    version: String,
    #[serde(default)]
    languages: Vec<String>,
    #[serde(default, rename = "rule")]
    rules: Vec<RuleFile>,
    #[serde(default)]
    visual: VisualFile,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleFile {
    id: String,
    path: Option<PathFile>,
    main: Option<MainFile>,
    main_call: Option<MainCallFile>,
    object_route: Option<ObjectRouteFile>,
    supertype: Option<SupertypeFile>,
    annotation: Option<AnnotationFile>,
    annotation_route: Option<AnnotationRouteFile>,
    call_route: Option<CallRouteFile>,
    call_handler: Option<CallHandlerFile>,
    instantiates: Option<InstantiatesFile>,
    dispatch: Option<DispatchFile>,
    name_tag: Option<NameTagFile>,
    query: Option<QueryFile>,
    wiring: Option<WiringFile>,
    test_code: Option<TestCodeFile>,
    noise: Option<NoiseFile>,
    annotation_tag: Option<AnnotationTagFile>,
    entry_owner_tag: Option<EntryOwnerTagFile>,
    significance: Option<SignificanceFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SignificanceFile {
    #[serde(default)]
    component_annotations: Vec<String>,
    #[serde(default)]
    constructor_annotations: Vec<String>,
    #[serde(default)]
    accessor_pattern: String,
    #[serde(default)]
    detail_packages: String,
    #[serde(default = "two")]
    hub_min_packages: usize,
}

fn two() -> usize {
    2
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AnnotationTagFile {
    names: Vec<String>,
    tag: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryOwnerTagFile {
    kinds: Vec<String>,
    tag: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoiseFile {
    #[serde(default)]
    name_suffixes: Vec<String>,
    #[serde(default)]
    supertype_suffixes: Vec<String>,
    #[serde(default)]
    methods: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WiringFile {
    #[serde(default)]
    file_markers: Vec<String>,
    #[serde(default)]
    file_imports: Vec<String>,
    #[serde(default)]
    calls: Vec<String>,
    #[serde(default)]
    annotations: Vec<String>,
    #[serde(default)]
    type_prefixes: Vec<String>,
    #[serde(default)]
    requires_imports: Vec<String>,
    #[serde(default)]
    caller_prefixes: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MainCallFile {
    methods: Vec<String>,
    #[serde(default)]
    file_imports: String,
    #[serde(default = "module_label")]
    label: String,
    #[serde(default)]
    handoff: String,
    #[serde(default = "application")]
    category: String,
}

fn module_label() -> String {
    "{module}".into()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ObjectRouteFile {
    #[serde(default)]
    var_types: Vec<String>,
    #[serde(default)]
    calls: Vec<String>,
    path_key: String,
    target_keys: Vec<String>,
    #[serde(default)]
    children_key: String,
    kind: String,
    #[serde(default = "path_label")]
    label: String,
}

fn path_label() -> String {
    "{path}".into()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TestCodeFile {
    #[serde(default)]
    file_imports: Vec<String>,
    #[serde(default)]
    dir_names: Vec<String>,
    #[serde(default)]
    paths: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathFile {
    glob: String,
    category: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MainFile {
    #[serde(default = "application")]
    category: String,
    #[serde(default)]
    label: String,
}

fn application() -> String {
    "application".into()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CallbackFile {
    method: String,
    #[serde(default = "unknown")]
    lifecycle: String,
    #[serde(default)]
    order: u32,
}

fn unknown() -> String {
    "unknown".into()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SupertypeFile {
    types: Vec<String>,
    callbacks: Vec<CallbackFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AnnotationFile {
    names: Vec<String>,
    handoff: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AnnotationRouteFile {
    names: Vec<String>,
    method: String,
    #[serde(default)]
    prefix_from_type: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CallRouteFile {
    methods: Vec<String>,
    #[serde(default)]
    group_methods: Vec<String>,
    #[serde(default)]
    receiver_types: Vec<String>,
    #[serde(default)]
    file_imports: String,
    #[serde(default)]
    path_arg: usize,
    #[serde(default = "last")]
    handler_arg: String,
    #[serde(default = "route_label")]
    label: String,
}

fn last() -> String {
    "last".into()
}

fn route_label() -> String {
    "{METHOD} {path}".into()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CallHandlerFile {
    methods: Vec<String>,
    #[serde(default)]
    receiver_types: Vec<String>,
    #[serde(default)]
    file_imports: String,
    kind: String,
    #[serde(default = "caller_label")]
    label: String,
}

fn caller_label() -> String {
    "{caller}".into()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InstantiatesFile {
    types: Vec<String>,
    handoff: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DispatchFile {
    from_override: String,
    to_override: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NameTagFile {
    kinds: Vec<String>,
    pattern: String,
    tag: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryFile {
    language: String,
    file: String,
    capture: String,
    kind: String,
    #[serde(default = "name_label")]
    label: String,
}

fn name_label() -> String {
    "{name}".into()
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct VisualFile {
    #[serde(default)]
    hide_tags: Vec<String>,
    #[serde(default)]
    fold_tags: Vec<String>,
    #[serde(default, rename = "tag_style")]
    tag_styles: Vec<TagStyleFile>,
    #[serde(default, rename = "frame")]
    frames: Vec<FrameFile>,
    #[serde(default, rename = "entry_group")]
    entry_groups: Vec<EntryGroupFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TagStyleFile {
    tag: String,
    color: [u8; 3],
    icon: Option<String>,
    badge: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FrameFile {
    auto_prefix: Option<String>,
    pattern: Option<String>,
    label: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryGroupFile {
    label: String,
    #[serde(default)]
    kinds: Vec<String>,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    collapsed: bool,
}

fn parse_lang(s: &str) -> Result<Lang> {
    match s {
        "java" => Ok(Lang::Java),
        "go" => Ok(Lang::Go),
        "typescript" => Ok(Lang::TypeScript),
        "javascript" => Ok(Lang::JavaScript),
        "rust" => Ok(Lang::Rust),
        other => bail!("unknown language `{other}`"),
    }
}

fn category(s: &str) -> Result<Category> {
    Category::parse(s).ok_or_else(|| anyhow!("unknown category `{s}` (application, dev-tool, test)"))
}

impl RuleFile {
    fn into_rule(self, dir: Option<&Path>) -> Result<Rule> {
        let count = [
            self.path.is_some(),
            self.main.is_some(),
            self.main_call.is_some(),
            self.object_route.is_some(),
            self.supertype.is_some(),
            self.annotation.is_some(),
            self.annotation_route.is_some(),
            self.call_route.is_some(),
            self.call_handler.is_some(),
            self.instantiates.is_some(),
            self.dispatch.is_some(),
            self.name_tag.is_some(),
            self.query.is_some(),
            self.wiring.is_some(),
            self.test_code.is_some(),
            self.noise.is_some(),
            self.annotation_tag.is_some(),
            self.entry_owner_tag.is_some(),
            self.significance.is_some(),
        ]
        .iter()
        .filter(|b| **b)
        .count();
        if count != 1 {
            bail!("rule `{}` must have exactly one matcher table, found {count}", self.id);
        }
        if let Some(p) = self.path {
            globset::Glob::new(&p.glob).with_context(|| format!("bad glob in rule `{}`", self.id))?;
            return Ok(Rule::Path { glob: p.glob, category: category(&p.category)? });
        }
        if let Some(m) = self.main {
            return Ok(Rule::Main { category: category(&m.category)?, label: m.label });
        }
        if let Some(m) = self.main_call {
            return Ok(Rule::MainCall {
                methods: m.methods,
                file_imports: m.file_imports,
                label: m.label,
                handoff: m.handoff,
                category: category(&m.category)?,
            });
        }
        if let Some(o) = self.object_route {
            return Ok(Rule::ObjectRoute {
                var_types: o.var_types,
                calls: o.calls,
                path_key: o.path_key,
                target_keys: o.target_keys,
                children_key: o.children_key,
                kind: o.kind,
                label: o.label,
            });
        }
        if let Some(s) = self.supertype {
            let callbacks = s
                .callbacks
                .into_iter()
                .map(|c| {
                    let lifecycle = Lifecycle::parse(&c.lifecycle).ok_or_else(|| anyhow!("unknown lifecycle `{}`", c.lifecycle))?;
                    Ok(Callback { method: c.method, lifecycle, order: c.order })
                })
                .collect::<Result<_>>()?;
            return Ok(Rule::Supertype { types: s.types, callbacks });
        }
        if let Some(a) = self.annotation {
            return Ok(Rule::Annotation { names: a.names, handoff: a.handoff });
        }
        if let Some(a) = self.annotation_route {
            return Ok(Rule::AnnotationRoute { names: a.names, method: a.method, prefix_from_type: a.prefix_from_type });
        }
        if let Some(c) = self.call_route {
            return Ok(Rule::CallRoute {
                methods: c.methods,
                group_methods: c.group_methods,
                receiver_types: c.receiver_types,
                file_imports: c.file_imports,
                path_arg: c.path_arg,
                handler_last: c.handler_arg == "last",
                label: c.label,
            });
        }
        if let Some(c) = self.call_handler {
            return Ok(Rule::CallHandler { methods: c.methods, receiver_types: c.receiver_types, file_imports: c.file_imports, kind: c.kind, label: c.label });
        }
        if let Some(i) = self.instantiates {
            return Ok(Rule::Instantiates { types: i.types, handoff: i.handoff });
        }
        if let Some(d) = self.dispatch {
            return Ok(Rule::Dispatch { from_override: d.from_override, to_override: d.to_override });
        }
        if let Some(n) = self.name_tag {
            regex::Regex::new(&n.pattern).with_context(|| format!("bad pattern in rule `{}`", self.id))?;
            return Ok(Rule::NameTag { kinds: n.kinds, pattern: n.pattern, tag: n.tag });
        }
        if let Some(w) = self.wiring {
            return Ok(Rule::Wiring {
                file_markers: w.file_markers,
                file_imports: w.file_imports,
                calls: w.calls,
                annotations: w.annotations,
                type_prefixes: w.type_prefixes,
                requires_imports: w.requires_imports,
                caller_prefixes: w.caller_prefixes,
            });
        }
        if let Some(s) = self.significance {
            for p in [&s.accessor_pattern, &s.detail_packages] {
                regex::Regex::new(p).with_context(|| format!("bad pattern in rule `{}`", self.id))?;
            }
            return Ok(Rule::Significance {
                component_annotations: s.component_annotations,
                constructor_annotations: s.constructor_annotations,
                accessor_pattern: s.accessor_pattern,
                detail_packages: s.detail_packages,
                hub_min_packages: s.hub_min_packages,
            });
        }
        if let Some(a) = self.annotation_tag {
            return Ok(Rule::AnnotationTag { names: a.names, tag: a.tag });
        }
        if let Some(e) = self.entry_owner_tag {
            return Ok(Rule::EntryOwnerTag { kinds: e.kinds, tag: e.tag });
        }
        if let Some(n) = self.noise {
            return Ok(Rule::Noise { name_suffixes: n.name_suffixes, supertype_suffixes: n.supertype_suffixes, methods: n.methods });
        }
        if let Some(t) = self.test_code {
            for g in &t.paths {
                globset::Glob::new(g).with_context(|| format!("bad glob in rule `{}`", self.id))?;
            }
            return Ok(Rule::TestCode { file_imports: t.file_imports, dir_names: t.dir_names, paths: t.paths });
        }
        let q = self.query.unwrap();
        let dir = dir.ok_or_else(|| anyhow!("rule `{}`: query rules need a pack directory", self.id))?;
        let path = dir.join(&q.file);
        let query = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let language = parse_lang(&q.language)?;
        crate::query::validate(language, &query).with_context(|| format!("bad query {}", path.display()))?;
        Ok(Rule::Query { language, query, capture: q.capture, kind: q.kind, label: q.label })
    }
}

impl Pack {
    pub fn parse(src: &str, origin: &str, dir: Option<PathBuf>) -> Result<Pack> {
        let file: PackFile = toml::from_str(src).map_err(|e| anyhow!("rule pack {origin}: {e}"))?;
        let languages = file.languages.iter().map(|l| parse_lang(l)).collect::<Result<Vec<_>>>().with_context(|| format!("rule pack {origin}"))?;
        let mut rules = Vec::new();
        for r in file.rules {
            let id = r.id.clone();
            rules.push((id, r.into_rule(dir.as_deref()).map_err(|e| anyhow!("rule pack {origin}: {e:#}"))?));
        }
        let v = file.visual;
        let frames = v
            .frames
            .into_iter()
            .map(|f| match (f.auto_prefix, f.pattern) {
                (Some(sep), None) => Ok(FrameRule::AutoPrefix { sep }),
                (None, Some(glob)) => {
                    Ok(FrameRule::Pattern { label: f.label.unwrap_or_else(|| glob.trim_end_matches('*').trim_end_matches('-').to_string()), glob })
                }
                _ => Err(anyhow!("rule pack {origin}: a frame needs exactly one of auto_prefix or pattern")),
            })
            .collect::<Result<Vec<_>>>()?;
        let entry_groups = v
            .entry_groups
            .into_iter()
            .map(|g| {
                Ok(EntryGroup {
                    label: g.label,
                    kinds: g.kinds,
                    categories: g.categories.iter().map(|c| category(c)).collect::<Result<_>>()?,
                    collapsed: g.collapsed,
                })
            })
            .collect::<Result<Vec<_>>>()
            .with_context(|| format!("rule pack {origin}"))?;
        let visual = VisualRules {
            tag_styles: v.tag_styles.into_iter().map(|t| TagStyle { tag: t.tag, color: t.color, icon: t.icon, badge: t.badge }).collect(),
            frames,
            hide_tags: v.hide_tags,
            fold_tags: v.fold_tags,
            entry_groups,
        };
        Ok(Pack { name: file.name, version: file.version, languages, rules, visual, hash: blake3::hash(src.as_bytes()).to_hex()[..16].to_string(), dir })
    }
}

impl Packs {
    pub fn from_packs(packs: Vec<Pack>, sel: &PackSelection) -> Packs {
        let packs: Vec<Pack> = packs.into_iter().filter(|p| !sel.disable.contains(&p.name) || sel.enable.contains(&p.name)).collect();
        let mut effective: Vec<(String, usize, Rule)> = Vec::new();
        let mut index: HashMap<String, usize> = HashMap::new();
        for (pi, p) in packs.iter().enumerate() {
            for (id, rule) in &p.rules {
                match index.get(id) {
                    Some(&i) => effective[i] = (id.clone(), pi, rule.clone()),
                    None => {
                        index.insert(id.clone(), effective.len());
                        effective.push((id.clone(), pi, rule.clone()));
                    }
                }
            }
        }
        Packs { packs, effective }
    }

    pub fn names(&self) -> Vec<&str> {
        self.packs.iter().map(|p| p.name.as_str()).collect()
    }

    pub fn packs(&self) -> &[Pack] {
        &self.packs
    }

    pub fn rules(&self) -> impl Iterator<Item = (&str, &Rule)> {
        self.effective.iter().map(|(id, _, r)| (id.as_str(), r))
    }

    pub fn rules_for(&self, lang: Lang) -> impl Iterator<Item = (&str, &Rule)> {
        self.effective
            .iter()
            .filter(move |(_, pi, _)| {
                let langs = &self.packs[*pi].languages;
                langs.is_empty() || langs.contains(&lang)
            })
            .map(|(id, _, r)| (id.as_str(), r))
    }

    pub fn visual(&self) -> VisualRules {
        let mut v = VisualRules::default();
        for p in &self.packs {
            for t in &p.visual.tag_styles {
                v.tag_styles.retain(|x| x.tag != t.tag);
                v.tag_styles.push(t.clone());
            }
            for f in &p.visual.frames {
                if !v.frames.contains(f) {
                    v.frames.push(f.clone());
                }
            }
            for t in &p.visual.hide_tags {
                if !v.hide_tags.contains(t) {
                    v.hide_tags.push(t.clone());
                }
            }
            for t in &p.visual.fold_tags {
                if !v.fold_tags.contains(t) {
                    v.fold_tags.push(t.clone());
                }
            }
            if !p.visual.entry_groups.is_empty() {
                v.entry_groups = p.visual.entry_groups.clone();
            }
        }
        v
    }
}

pub fn detection_hash(packs: &Packs) -> String {
    let mut h = blake3::Hasher::new();
    for (id, rule) in packs.rules() {
        h.update(id.as_bytes());
        h.update(format!("{rule:?}").as_bytes());
    }
    h.finalize().to_hex()[..16].to_string()
}

fn load_dir(dir: &Path, out: &mut Vec<Pack>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    let mut subdirs: Vec<PathBuf> = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.join("pack.toml").is_file()).collect();
    if dir.join("pack.toml").is_file() {
        subdirs.insert(0, dir.to_path_buf());
    }
    subdirs.sort();
    for d in subdirs {
        let file = d.join("pack.toml");
        let src = std::fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
        out.push(Pack::parse(&src, &file.display().to_string(), Some(d))?);
    }
    Ok(())
}

pub fn builtin_packs() -> Vec<Pack> {
    BUILTINS.iter().map(|(name, src)| Pack::parse(src, &format!("built-in {name}"), None).expect("built-in pack parses")).collect()
}

pub fn user_rules_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("codescape").join("rules"))
}

pub fn load_packs(project_dir: Option<&Path>, user_dir: Option<&Path>, sel: &PackSelection) -> Result<Packs> {
    let mut packs = builtin_packs();
    if let Some(u) = user_dir {
        load_dir(u, &mut packs)?;
    }
    if let Some(p) = project_dir {
        load_dir(&p.join(".codescape").join("rules"), &mut packs)?;
    }
    for extra in &sel.extra_dirs {
        load_dir(extra, &mut packs)?;
    }
    Ok(Packs::from_packs(packs, sel))
}
