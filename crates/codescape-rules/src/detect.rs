use crate::objects::{RouteShape, RouteTarget, object_routes};
use crate::pack::{Packs, Rule};
use crate::query;
use crate::routes::{PrefixFlow, join};
use codescape_facts::{CallFacts, FileFacts, FnFacts, ProjectFacts, TypeFacts};
use codescape_format::*;
use globset::Glob;
use std::collections::{HashMap, HashSet, VecDeque};

pub const FALLBACK_RULE: &str = "builtin.fallback-callback";
const COMPONENT_ROLES: [&str; 3] = ["layer:handler", "layer:service", "layer:repository"];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DetectStats {
    pub entries: usize,
    pub dispatch_edges: usize,
    pub tags: usize,
}

struct MainEntry {
    entry: u32,
    node: NodeId,
    module: NodeId,
    category: Category,
}

struct Ctx<'a> {
    packs: &'a Packs,
    facts: &'a ProjectFacts,
    types: HashMap<&'a str, &'a TypeFacts>,
    type_nodes: HashMap<NodeId, &'a TypeFacts>,
    mains: Vec<MainEntry>,
    reach: HashMap<NodeId, usize>,
    handled: HashSet<NodeId>,
    test_files: HashSet<&'a str>,
    stats: DetectStats,
}

pub fn apply_detection(packs: &Packs, facts: &ProjectFacts, sources: &dyn Fn(&str) -> Option<Vec<u8>>, b: &mut GraphBuilder) -> DetectStats {
    let mut cx = Ctx {
        packs,
        facts,
        types: HashMap::new(),
        type_nodes: HashMap::new(),
        mains: Vec::new(),
        reach: HashMap::new(),
        handled: HashSet::new(),
        test_files: HashSet::new(),
        stats: DetectStats::default(),
    };
    for f in &facts.files {
        for t in &f.types {
            cx.types.insert(t.fqn.as_str(), t);
            cx.type_nodes.insert(t.node, t);
        }
    }
    let before = b.entries().len();
    cx.classify(sources, b);
    cx.name_tags(b);
    cx.dispatch(b);
    cx.mains(b);
    cx.compute_reach(b);
    cx.handoffs(b);
    cx.callbacks(b);
    cx.annotation_routes(b);
    cx.call_rules(b);
    cx.object_routes(sources, b);
    cx.queries(sources, b);
    cx.role_tags(b);
    cx.significance(b);
    cx.filters(b);
    cx.stats.entries = b.entries().len() - before;
    cx.stats
}

fn glob_set(globs: &[String]) -> globset::GlobSet {
    let mut set = globset::GlobSetBuilder::new();
    for g in globs.iter().filter_map(|g| Glob::new(g).ok()) {
        set.add(g);
    }
    set.build().unwrap_or_else(|_| globset::GlobSet::empty())
}

fn files_sorted(facts: &ProjectFacts) -> Vec<&FileFacts> {
    let mut v: Vec<&FileFacts> = facts.files.iter().collect();
    v.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    v
}

fn fill(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

impl<'a> Ctx<'a> {
    fn extends(&self, fqn: &str, target: &str) -> bool {
        let mut seen = HashSet::new();
        let mut q = VecDeque::from([fqn.to_string()]);
        while let Some(cur) = q.pop_front() {
            if cur == target {
                return true;
            }
            if !seen.insert(cur.clone()) {
                continue;
            }
            if let Some(t) = self.types.get(cur.as_str()) {
                q.extend(t.supertypes.iter().cloned());
            }
        }
        false
    }

    fn owner_extends(&self, f: &FnFacts, target: &str) -> bool {
        f.owner.and_then(|o| self.type_nodes.get(&o)).is_some_and(|t| self.extends(&t.fqn, target))
    }

    fn all_fns(&self) -> impl Iterator<Item = (&'a FileFacts, &'a FnFacts)> {
        let facts = self.facts;
        facts.files.iter().flat_map(|f| f.functions.iter().map(move |func| (f, func)))
    }

    fn classify(&mut self, sources: &dyn Fn(&str) -> Option<Vec<u8>>, b: &mut GraphBuilder) {
        let facts = self.facts;
        let mut wiring_files: HashSet<&'a str> = HashSet::new();
        let mut wiring_calls: HashSet<(NodeId, NodeId)> = HashSet::new();
        let mut wiring_nodes: Vec<NodeId> = Vec::new();
        let test_globs: HashMap<&str, globset::GlobSet> = self
            .packs
            .rules()
            .filter_map(|(id, r)| match r {
                Rule::TestCode { paths, .. } if !paths.is_empty() => Some((id, glob_set(paths))),
                _ => None,
            })
            .collect();
        for file in &facts.files {
            let imports = |prefixes: &[String]| {
                file.imports
                    .iter()
                    .any(|i| prefixes.iter().any(|p| i.path == *p || i.path.starts_with(&format!("{p}/")) || i.path.starts_with(&format!("{p}."))))
            };
            let dirs: Vec<&str> = file.rel_path.split('/').collect();
            let dirs = &dirs[..dirs.len().saturating_sub(1)];
            for (id, rule) in self.packs.rules_for(file.lang) {
                match rule {
                    Rule::TestCode { file_imports, dir_names, .. } => {
                        let by_path = test_globs.get(id).is_some_and(|g| g.is_match(&file.rel_path));
                        if by_path || imports(file_imports) || dirs.iter().any(|d| dir_names.iter().any(|n| n == d)) {
                            self.test_files.insert(file.rel_path.as_str());
                        }
                    }
                    Rule::Wiring { file_markers, file_imports, calls, annotations, type_prefixes, requires_imports, caller_prefixes } => {
                        let marked = !file_markers.is_empty()
                            && sources(&file.rel_path).is_some_and(|src| {
                                let head = String::from_utf8_lossy(&src[..src.len().min(1024)]).into_owned();
                                file_markers.iter().any(|m| head.contains(m.as_str()))
                            });
                        if marked || (imports(file_imports) && file.types.is_empty()) {
                            wiring_files.insert(file.rel_path.as_str());
                        }
                        if !requires_imports.is_empty() && !imports(requires_imports) {
                            continue;
                        }
                        for t in &file.types {
                            let simple = t.fqn.rsplit('.').next().unwrap_or(&t.fqn);
                            if t.annotations.iter().any(|a| annotations.contains(&a.name)) || type_prefixes.iter().any(|p| simple.starts_with(p.as_str())) {
                                wiring_nodes.push(t.node);
                            }
                        }
                        for f in &file.functions {
                            if f.annotations.iter().any(|a| annotations.contains(&a.name)) {
                                wiring_nodes.push(f.node);
                            }
                        }
                        for c in &file.calls {
                            if caller_prefixes.iter().any(|p| c.method.starts_with(p.as_str())) {
                                wiring_nodes.push(c.caller);
                            }
                            let qualified = c.receiver_var.as_ref().map(|r| format!("{r}.{}", c.method));
                            let by_name = qualified.as_ref().is_some_and(|q| calls.contains(q));
                            let by_prefix = c.receiver_var.as_ref().into_iter().chain(c.receiver_type.as_ref()).any(|r| {
                                let simple = r.rsplit('.').next().unwrap_or(r);
                                type_prefixes.iter().any(|p| simple.starts_with(p.as_str()))
                            });
                            if (by_name || by_prefix)
                                && let Some(t) = c.target
                            {
                                wiring_calls.insert((c.caller, t));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        for file in &facts.files {
            let flag = if self.test_files.contains(file.rel_path.as_str()) {
                NodeFlags::TEST
            } else if wiring_files.contains(file.rel_path.as_str()) {
                NodeFlags::WIRING
            } else {
                continue;
            };
            let why = if flag == NodeFlags::TEST { "why:test:a test helper" } else { "why:wiring:in a dependency-injection file" };
            for t in &file.types {
                b.add_flags(t.node, flag);
                b.add_tag(t.node, why);
            }
            for f in &file.functions {
                b.add_flags(f.node, flag);
                b.add_tag(f.node, why);
            }
        }
        for n in wiring_nodes {
            b.add_flags(n, NodeFlags::WIRING);
            b.add_tag(n, "why:wiring:dependency-injection setup");
        }
        self.mark_noise(b);
        self.mark_logging(b);
        let mut importers: HashMap<(NodeId, NodeId), (bool, bool)> = HashMap::new();
        for file in &facts.files {
            let Some(p) = file.package else { continue };
            let wiring = wiring_files.contains(file.rel_path.as_str());
            let test = self.test_files.contains(file.rel_path.as_str()) || file.is_test;
            for &q in &file.import_targets {
                let slot = importers.entry((p, q)).or_insert((true, true));
                slot.0 &= wiring;
                slot.1 &= test;
            }
        }
        for e in b.edges_mut() {
            if wiring_calls.contains(&(e.from, e.to)) {
                e.flags |= EdgeFlags::WIRING;
            }
            if e.kind == EdgeKind::DependsOn
                && let Some(&(all_wiring, all_test)) = importers.get(&(e.from, e.to))
            {
                if all_test {
                    e.scope = Scope::Test;
                } else if all_wiring {
                    e.flags |= EdgeFlags::WIRING;
                }
            }
        }
    }

    fn supertype_ends_with(&self, fqn: &str, suffixes: &[String]) -> bool {
        let mut seen = HashSet::new();
        let mut q: VecDeque<String> = self.types.get(fqn).map(|t| t.supertypes.iter().cloned().collect()).unwrap_or_default();
        while let Some(cur) = q.pop_front() {
            if suffixes.iter().any(|s| cur.ends_with(s.as_str())) {
                return true;
            }
            if !seen.insert(cur.clone()) {
                continue;
            }
            if let Some(t) = self.types.get(cur.as_str()) {
                q.extend(t.supertypes.iter().cloned());
            }
        }
        false
    }

    fn mark_noise(&self, b: &mut GraphBuilder) {
        for file in &self.facts.files {
            for (_, rule) in self.packs.rules_for(file.lang) {
                let Rule::Noise { name_suffixes, supertype_suffixes, methods } = rule else { continue };
                for t in &file.types {
                    let simple = t.fqn.rsplit('.').next().unwrap_or(&t.fqn);
                    let by_name = name_suffixes.iter().find(|s| simple.ends_with(s.as_str()) && simple.len() > s.len());
                    let by_super = self.supertype_ends_with(&t.fqn, supertype_suffixes);
                    let by_method = file.functions.iter().find(|f| f.owner == Some(t.node) && methods.contains(&f.name));
                    let why = match (by_name, by_super, by_method) {
                        (Some(suffix), _, _) => format!("why:error:name ends with {suffix}"),
                        (None, true, _) => "why:error:extends an error type".to_string(),
                        (None, false, Some(f)) => format!("why:error:has an {}() method", f.name),
                        (None, false, None) => continue,
                    };
                    b.add_flags(t.node, NodeFlags::NOISE);
                    b.add_tag(t.node, &why);
                }
            }
        }
    }

    fn mark_logging(&self, b: &mut GraphBuilder) {
        for file in &self.facts.files {
            for (_, rule) in self.packs.rules_for(file.lang) {
                let Rule::Logging { type_names, name_suffixes } = rule else { continue };
                let is_logger = |fqn: &str| {
                    let simple = fqn.rsplit('.').next().unwrap_or(fqn);
                    type_names.iter().any(|n| n == simple) || name_suffixes.iter().any(|s| simple.ends_with(s.as_str()) && simple.len() > s.len())
                };
                for t in &file.types {
                    let simple = t.fqn.rsplit('.').next().unwrap_or(&t.fqn);
                    let why = if let Some(s) = name_suffixes.iter().find(|s| simple.ends_with(s.as_str()) && simple.len() > s.len()) {
                        format!("why:logging:name ends with {s}")
                    } else if type_names.iter().any(|n| n == simple) {
                        "why:logging:a logger type".to_string()
                    } else if self.supertype_matches(&t.fqn, &is_logger) {
                        "why:logging:extends a logger".to_string()
                    } else {
                        continue;
                    };
                    b.add_flags(t.node, NodeFlags::LOGGING);
                    b.add_tag(t.node, &why);
                }
            }
        }
    }

    fn supertype_matches(&self, fqn: &str, test: &dyn Fn(&str) -> bool) -> bool {
        let mut seen = HashSet::new();
        let mut q: VecDeque<String> = self.types.get(fqn).map(|t| t.supertypes.iter().cloned().collect()).unwrap_or_default();
        while let Some(cur) = q.pop_front() {
            if test(&cur) {
                return true;
            }
            if !seen.insert(cur.clone()) {
                continue;
            }
            if let Some(t) = self.types.get(cur.as_str()) {
                q.extend(t.supertypes.iter().cloned());
            }
        }
        false
    }

    fn significance(&mut self, b: &mut GraphBuilder) {
        let Some((annotations, ctor_annotations, accessor, detail_pkgs, hub_min)) = self.packs.rules().find_map(|(_, r)| match r {
            Rule::Significance { component_annotations, constructor_annotations, accessor_pattern, detail_packages, hub_min_packages } => {
                Some((component_annotations.clone(), constructor_annotations.clone(), accessor_pattern.clone(), detail_packages.clone(), *hub_min_packages))
            }
            _ => None,
        }) else {
            return;
        };
        let accessor = regex::Regex::new(&accessor).ok().filter(|_| !accessor.is_empty());
        let detail_pkgs = regex::Regex::new(&detail_pkgs).ok().filter(|_| !detail_pkgs.is_empty());
        let owner_type = |b: &GraphBuilder, n: NodeId| -> Option<NodeId> {
            let mut cur = Some(n);
            while let Some(c) = cur {
                if b.node_kind(c) == NodeKind::Type {
                    return Some(c);
                }
                cur = b.parent(c);
            }
            None
        };
        let package_of = |b: &GraphBuilder, n: NodeId| -> Option<NodeId> {
            let mut cur = Some(n);
            while let Some(c) = cur {
                if b.node_kind(c) == NodeKind::Package {
                    return Some(c);
                }
                cur = b.parent(c);
            }
            None
        };
        let set_aside = NodeFlags::TEST | NodeFlags::MOCK | NodeFlags::WIRING | NodeFlags::NOISE | NodeFlags::EXTERNAL;
        let mut di_built: HashSet<NodeId> = HashSet::new();
        let mut users: HashMap<NodeId, HashSet<NodeId>> = HashMap::new();
        for e in b.edges() {
            let Some(target_type) = owner_type(b, e.to).or_else(|| (b.node_kind(e.to) == NodeKind::Member).then_some(e.to)) else { continue };
            if b.flags(e.from).contains(NodeFlags::WIRING) {
                di_built.insert(target_type);
                continue;
            }
            if let (Some(from_pkg), Some(to_pkg)) = (package_of(b, e.from), package_of(b, e.to))
                && from_pkg != to_pkg
            {
                users.entry(target_type).or_default().insert(from_pkg);
            }
        }
        let extended: HashSet<NodeId> = b
            .edges()
            .iter()
            .filter(|e| matches!(e.kind, EdgeKind::Inherits | EdgeKind::Implements) && !b.flags(e.from).intersects(set_aside))
            .map(|e| e.to)
            .collect();
        let entry_nodes: Vec<NodeId> = b.entries().iter().map(|e| e.node).collect();
        let mut details: Vec<(NodeId, String)> = Vec::new();
        let mut accessors: Vec<NodeId> = Vec::new();
        for file in &self.facts.files {
            for t in &file.types {
                let n = t.node;
                if let Some(re) = &accessor {
                    accessors.extend(
                        b.children(n).iter().copied().filter(|&c| b.sub_kind(c) == SubKind::Method && !b.name(c).starts_with('<') && re.is_match(b.name(c))),
                    );
                }
                if b.flags(n).intersects(set_aside) {
                    continue;
                }
                let sub = b.sub_kind(n);
                let fns: Vec<&FnFacts> = file.functions.iter().filter(|f| f.owner == Some(n)).collect();
                let all_fns: Vec<&FnFacts> = self.facts.files.iter().flat_map(|f| f.functions.iter()).filter(|f| f.owner == Some(n)).collect();
                let methods: Vec<&str> = b.children(n).iter().filter(|&&c| b.sub_kind(c) == SubKind::Method).map(|&c| b.name(c)).collect();
                let model = b.has_tag(n, "layer:model");
                let strong = !model
                    && (COMPONENT_ROLES.iter().any(|r| b.has_tag(n, r))
                        || sub == SubKind::Interface
                        || extended.contains(&n)
                        || entry_nodes.iter().any(|&e| e == n || b.parent(e) == Some(n))
                        || t.annotations.iter().any(|a| annotations.contains(&a.name))
                        || all_fns.iter().chain(fns.iter()).any(|f| f.annotations.iter().any(|a| ctor_annotations.contains(&a.name)))
                        || di_built.contains(&n));
                if strong {
                    continue;
                }
                let nested = b.parent(n).is_some_and(|p| b.node_kind(p) == NodeKind::Type);
                let data_kind = matches!(sub, SubKind::Record | SubKind::Enum | SubKind::Annotation);
                let no_methods = methods.is_empty();
                let accessors_only = accessor.as_ref().is_some_and(|re| methods.iter().all(|m| re.is_match(m)));
                let helper_pkg = detail_pkgs.as_ref().is_some_and(|re| package_of(b, n).is_some_and(|p| re.is_match(b.name(p))));
                let hub = users.get(&n).is_some_and(|u| u.len() >= hub_min);
                let why = if model {
                    "a model (DTO, request, response…)".to_string()
                } else if nested {
                    "nested in another type".to_string()
                } else if data_kind {
                    "a record, enum or annotation".to_string()
                } else if no_methods {
                    "has no methods".to_string()
                } else if accessors_only {
                    "only getters and setters".to_string()
                } else if helper_pkg {
                    "in a helper package".to_string()
                } else if !hub {
                    format!("used from fewer than {hub_min} other packages")
                } else {
                    continue;
                };
                details.push((n, why));
            }
        }
        for file in &self.facts.files {
            for f in file.functions.iter().filter(|f| f.owner.is_none()) {
                let n = f.node;
                if b.flags(n).intersects(set_aside) || b.parent(n).is_none_or(|p| b.node_kind(p) != NodeKind::Package) {
                    continue;
                }
                let entry = entry_nodes.contains(&n);
                let shared = users.get(&n).is_some_and(|u| u.len() >= hub_min);
                if !entry && !shared && !f.is_main {
                    details.push((n, format!("a package function used from fewer than {hub_min} other packages")));
                }
            }
        }
        for (n, why) in details {
            b.add_flags(n, NodeFlags::DETAIL);
            b.add_tag(n, &format!("why:detail:{why}"));
        }
        for n in accessors {
            b.add_tag(n, "accessor");
        }
    }

    fn role_tags(&mut self, b: &mut GraphBuilder) {
        for (_, rule) in self.packs.rules() {
            match rule {
                Rule::AnnotationTag { names, tag } => {
                    for file in &self.facts.files {
                        let hits = file
                            .types
                            .iter()
                            .filter(|t| t.annotations.iter().any(|a| names.contains(&a.name)))
                            .map(|t| t.node)
                            .chain(file.functions.iter().filter(|f| f.annotations.iter().any(|a| names.contains(&a.name))).map(|f| f.node));
                        for n in hits.collect::<Vec<_>>() {
                            b.add_tag(n, tag);
                            self.stats.tags += 1;
                        }
                    }
                }
                Rule::EntryOwnerTag { kinds, tag } => {
                    let owners: Vec<NodeId> = b
                        .entries()
                        .iter()
                        .filter(|e| kinds.contains(&e.kind))
                        .filter_map(|e| match b.node_kind(e.node) {
                            NodeKind::Type => Some(e.node),
                            _ => b.parent(e.node).filter(|&p| b.node_kind(p) == NodeKind::Type),
                        })
                        .collect();
                    for n in owners {
                        b.add_tag(n, tag);
                        self.stats.tags += 1;
                    }
                }
                _ => {}
            }
        }
    }

    fn filters(&mut self, b: &mut GraphBuilder) {
        for (_, rule) in self.packs.rules() {
            let Rule::Filter { tag, paths, names } = rule else { continue };
            let paths = glob_set(paths);
            let names: Vec<regex::Regex> = names.iter().filter_map(|r| regex::Regex::new(r).ok()).collect();
            let named = |b: &GraphBuilder, n: NodeId| names.iter().any(|re| re.is_match(b.name(n)));
            let mut stack: Vec<NodeId> = Vec::new();
            for file in &self.facts.files {
                let by_path = paths.is_match(&file.rel_path);
                stack.extend(file.types.iter().map(|t| t.node).filter(|&n| by_path || named(b, n)));
                stack.extend(file.functions.iter().filter(|f| by_path || (f.owner.is_none() && named(b, f.node))).map(|f| f.node));
            }
            while let Some(n) = stack.pop() {
                if !b.has_tag(n, tag) {
                    b.add_tag(n, tag);
                    self.stats.tags += 1;
                    stack.extend_from_slice(b.children(n));
                }
            }
            for n in (1..b.node_count() as NodeId).rev() {
                if !matches!(b.node_kind(n), NodeKind::Package | NodeKind::Module) || b.has_tag(n, tag) {
                    continue;
                }
                let mut live = b.children(n).iter().filter(|&&c| !b.flags(c).contains(NodeFlags::TEST)).peekable();
                if live.peek().is_some() && live.all(|&c| b.has_tag(c, tag)) {
                    b.add_tag(n, tag);
                }
            }
        }
    }

    fn name_tags(&mut self, b: &mut GraphBuilder) {
        for (_, rule) in self.packs.rules() {
            let Rule::NameTag { kinds, pattern, tag } = rule else { continue };
            let Ok(re) = regex::Regex::new(pattern) else { continue };
            for id in 0..b.node_count() as NodeId {
                let kind = match b.node_kind(id) {
                    NodeKind::Type => "type",
                    NodeKind::Member => "member",
                    NodeKind::Package => "package",
                    NodeKind::Module => "module",
                    _ => continue,
                };
                if (kinds.is_empty() || kinds.iter().any(|k| k == kind)) && re.is_match(b.name(id)) {
                    b.add_tag(id, tag);
                    self.stats.tags += 1;
                }
            }
        }
    }

    fn dispatch(&mut self, b: &mut GraphBuilder) {
        for (_, rule) in self.packs.rules() {
            let Rule::Dispatch { from_override, to_override } = rule else { continue };
            let (Some((ft, fm)), Some((tt, tm))) = (from_override.rsplit_once('.'), to_override.rsplit_once('.')) else { continue };
            let froms: Vec<NodeId> = self.all_fns().filter(|(_, f)| f.name == fm && self.owner_extends(f, ft)).map(|(_, f)| f.node).collect();
            let tos: Vec<NodeId> = self.all_fns().filter(|(_, f)| f.name == tm && self.owner_extends(f, tt)).map(|(_, f)| f.node).collect();
            for &from in &froms {
                for &to in &tos {
                    if from != to {
                        b.add_edge(from, to, EdgeKind::FrameworkDispatch, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
                        self.stats.dispatch_edges += 1;
                    }
                }
            }
        }
    }

    fn category_for(&self, file: &FileFacts, default: Category) -> Category {
        if file.is_test || self.test_files.contains(file.rel_path.as_str()) {
            return Category::Test;
        }
        let mut cat = if file.is_dev { Category::DevTool } else { default };
        for (_, rule) in self.packs.rules_for(file.lang) {
            if let Rule::Path { glob, category } = rule
                && Glob::new(glob).is_ok_and(|g| g.compile_matcher().is_match(&file.rel_path))
            {
                cat = *category;
            }
        }
        cat
    }

    fn mains(&mut self, b: &mut GraphBuilder) {
        for file in files_sorted(self.facts) {
            self.call_mains(file, b);
            let main_rule = self
                .packs
                .rules_for(file.lang)
                .filter_map(|(id, r)| match r {
                    Rule::Main { category, label } => Some((id.to_string(), *category, label.clone())),
                    _ => None,
                })
                .last();
            let Some((rule_id, default, template)) = main_rule else { continue };
            for func in file.functions.iter().filter(|f| f.is_main) {
                if self.mains.iter().any(|m| m.node == func.node) {
                    continue;
                }
                let pkg = b.parent(func.node).map(|p| b.name(p).to_string()).unwrap_or_default();
                let label = match func.owner {
                    Some(o) => b.name(o).to_string(),
                    None if !template.is_empty() => fill(&template, &[("module", b.name(file.module)), ("package", &pkg), ("name", &func.name)]),
                    None => pkg.rsplit('/').next().unwrap_or(&pkg).to_string(),
                };
                let category = self.category_for(file, default);
                let entry = b.add_entry(EntryPointSpec {
                    node: func.node,
                    kind: "main".into(),
                    category,
                    label,
                    parent: None,
                    lifecycle: Lifecycle::Once,
                    order: 0,
                    rule: rule_id.clone(),
                    handoff: None,
                });
                self.handled.insert(func.node);
                self.mains.push(MainEntry { entry, node: func.node, module: file.module, category });
            }
        }
    }

    fn call_mains(&mut self, file: &'a FileFacts, b: &mut GraphBuilder) {
        let rules: Vec<(String, Rule)> =
            self.packs.rules_for(file.lang).filter(|(_, r)| matches!(r, Rule::MainCall { .. })).map(|(id, r)| (id.to_string(), r.clone())).collect();
        for (id, rule) in rules {
            let Rule::MainCall { methods, file_imports, label, handoff, category } = rule else { continue };
            if !file_imports.is_empty() && !file.imports.iter().any(|i| i.path.starts_with(&file_imports)) {
                continue;
            }
            for c in file.calls.iter().filter(|c| methods.contains(&c.method)) {
                let handoff = (!handoff.is_empty()).then(|| handoff.clone());
                if let Some(m) = self.mains.iter().position(|m| m.node == c.caller) {
                    if let Some(h) = &handoff {
                        self.set_handoff(b, m, h);
                    }
                    continue;
                }
                let arg0 = c.ident_args.first().cloned().flatten().or_else(|| c.string_args.first().cloned().flatten()).unwrap_or_default();
                let text = fill(&label, &[("module", b.name(file.module)), ("caller", b.name(c.caller)), ("arg0", &arg0)]);
                let category = self.category_for(file, category);
                let entry = b.add_entry(EntryPointSpec {
                    node: c.caller,
                    kind: "main".into(),
                    category,
                    label: text,
                    parent: None,
                    lifecycle: Lifecycle::Once,
                    order: 0,
                    rule: id.clone(),
                    handoff,
                });
                self.handled.insert(c.caller);
                self.mains.push(MainEntry { entry, node: c.caller, module: file.module, category });
            }
        }
    }

    fn compute_reach(&mut self, b: &GraphBuilder) {
        let mut adj: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        for e in b.edges() {
            if e.kind.is_call_like() {
                adj.entry(e.from).or_default().push(e.to);
            }
        }
        let mut best: HashMap<NodeId, (u32, usize)> = HashMap::new();
        for (mi, m) in self.mains.iter().enumerate() {
            let mut seen: HashSet<NodeId> = HashSet::new();
            let mut q = VecDeque::from([(m.node, 0u32)]);
            while let Some((n, dist)) = q.pop_front() {
                if !seen.insert(n) {
                    continue;
                }
                let slot = best.entry(n).or_insert((dist, mi));
                if dist < slot.0 {
                    *slot = (dist, mi);
                }
                if let Some(next) = adj.get(&n) {
                    q.extend(next.iter().map(|&x| (x, dist + 1)));
                }
                if b.node_kind(n) == NodeKind::Type {
                    q.extend(b.children(n).iter().copied().filter(|&c| b.node_kind(c) == NodeKind::Member).map(|c| (c, dist + 1)));
                }
            }
        }
        self.reach = best.into_iter().map(|(n, (_, mi))| (n, mi)).collect();
    }

    fn main_for(&self, b: &GraphBuilder, node: NodeId, module: NodeId) -> Option<usize> {
        let mut n = Some(node);
        while let Some(cur) = n {
            if let Some(&m) = self.reach.get(&cur) {
                return Some(m);
            }
            n = b.parent(cur).filter(|&p| matches!(b.node_kind(p), NodeKind::Type));
        }
        self.mains.iter().position(|m| m.module == module && m.category == Category::Application)
    }

    fn add_child(&mut self, b: &mut GraphBuilder, node: NodeId, module: NodeId, kind: &str, label: String, lifecycle: Lifecycle, order: u32, rule: &str) {
        if b.flags(node).intersects(NodeFlags::TEST | NodeFlags::MOCK) {
            return;
        }
        let main = self.main_for(b, node, module);
        let (parent, category) = match main {
            Some(m) => (Some(self.mains[m].entry), self.mains[m].category),
            None => (None, Category::Application),
        };
        if b.entries().iter().any(|e| e.node == node && e.label == label) {
            return;
        }
        b.add_entry(EntryPointSpec { node, kind: kind.into(), category, label, parent, lifecycle, order, rule: rule.into(), handoff: None });
        self.handled.insert(node);
    }

    fn set_handoff(&self, b: &mut GraphBuilder, main: usize, handoff: &str) {
        let entry = self.mains[main].entry as usize;
        let e = &mut b.entries_mut()[entry];
        if e.handoff.is_none() {
            e.handoff = Some(handoff.into());
        }
    }

    fn handoffs(&mut self, b: &mut GraphBuilder) {
        for (id, rule) in self.packs.rules() {
            match rule {
                Rule::Annotation { names, handoff } => {
                    for file in files_sorted(self.facts) {
                        for t in file.types.iter().filter(|t| t.annotations.iter().any(|a| names.contains(&a.name))) {
                            let main = self.mains.iter().position(|m| b.parent(m.node) == Some(t.node));
                            match main {
                                Some(m) => self.set_handoff(b, m, handoff),
                                None => {
                                    let label = b.name(t.node).to_string();
                                    let entry = b.add_entry(EntryPointSpec {
                                        node: t.node,
                                        kind: "framework-annotation".into(),
                                        category: Category::Application,
                                        label,
                                        parent: None,
                                        lifecycle: Lifecycle::Once,
                                        order: 0,
                                        rule: id.into(),
                                        handoff: Some(handoff.clone()),
                                    });
                                    self.mains.push(MainEntry { entry, node: t.node, module: file.module, category: Category::Application });
                                }
                            }
                        }
                    }
                }
                Rule::Instantiates { types, handoff } => {
                    for file in &self.facts.files {
                        for c in file.calls.iter().filter(|c| c.instantiates.as_ref().is_some_and(|i| types.contains(i))) {
                            if let Some(m) = self.main_for(b, c.caller, NONE) {
                                self.set_handoff(b, m, handoff);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn find_method(&self, ty: &TypeFacts, method: &str) -> Option<NodeId> {
        let mut seen = HashSet::new();
        let mut q = VecDeque::from([ty.fqn.as_str()]);
        while let Some(cur) = q.pop_front() {
            if !seen.insert(cur) {
                continue;
            }
            let Some(t) = self.types.get(cur) else { continue };
            if let Some(f) = self.all_fns().find(|(_, f)| f.owner == Some(t.node) && f.name == method) {
                return Some(f.1.node);
            }
            q.extend(t.supertypes.iter().map(|s| s.as_str()));
        }
        None
    }

    fn callbacks(&mut self, b: &mut GraphBuilder) {
        for (id, rule) in self.packs.rules() {
            let Rule::Supertype { types, callbacks } = rule else { continue };
            for file in files_sorted(self.facts) {
                for t in &file.types {
                    if !types.iter().any(|target| self.extends(&t.fqn, target)) {
                        continue;
                    }
                    if !self.reach.contains_key(&t.node) {
                        continue;
                    }
                    for cb in callbacks {
                        let Some(node) = self.find_method(t, &cb.method) else { continue };
                        let label = format!("{}.{}()", b.name(t.node), cb.method);
                        self.add_child(b, node, file.module, "framework-callback", label, cb.lifecycle, cb.order, id);
                    }
                }
            }
        }
        let instantiated_by_main: HashSet<NodeId> = {
            let main_nodes: HashSet<NodeId> = self.mains.iter().map(|m| m.node).collect();
            b.edges().iter().filter(|e| e.kind == EdgeKind::Instantiates && main_nodes.contains(&e.from)).map(|e| e.to).collect()
        };
        let mut order = 100;
        for file in files_sorted(self.facts) {
            for f in file.functions.iter().filter(|f| f.overrides_external.is_some()) {
                let Some(owner) = f.owner else { continue };
                if self.handled.contains(&f.node) || !instantiated_by_main.contains(&owner) {
                    continue;
                }
                let label = format!("{}.{}()", b.name(owner), f.name);
                order += 1;
                self.add_child(b, f.node, file.module, "framework-callback", label, Lifecycle::Unknown, order, FALLBACK_RULE);
            }
        }
    }

    fn annotation_routes(&mut self, b: &mut GraphBuilder) {
        for (id, rule) in self.packs.rules() {
            let Rule::AnnotationRoute { names, method, prefix_from_type } = rule else { continue };
            for file in files_sorted(self.facts) {
                for f in &file.functions {
                    let Some(a) = f.annotations.iter().find(|a| names.contains(&a.name)) else { continue };
                    let prefix = f
                        .owner
                        .and_then(|o| self.type_nodes.get(&o))
                        .and_then(|t| t.annotations.iter().find(|a| prefix_from_type.contains(&a.name)))
                        .and_then(|a| a.args.first().cloned())
                        .unwrap_or_default();
                    let path = join(&Some(prefix), a.args.first().map(String::as_str).unwrap_or(""));
                    self.add_child(b, f.node, file.module, "http-route", format!("{method} {path}"), Lifecycle::OnEvent, 0, id);
                }
            }
        }
    }

    fn receiver_ok(file: &FileFacts, c: &CallFacts, receiver_types: &[String], file_imports: &str) -> bool {
        match &c.receiver_type {
            Some(t) if receiver_types.iter().any(|r| r == t || r.trim_start_matches('*') == t.trim_start_matches('*')) => true,
            Some(_) => false,
            None => !file_imports.is_empty() && file.imports.iter().any(|i| i.path.starts_with(file_imports)),
        }
    }

    fn call_rules(&mut self, b: &mut GraphBuilder) {
        let rules: Vec<(String, Rule)> = self.packs.rules().map(|(id, r)| (id.to_string(), r.clone())).collect();
        for (id, rule) in rules {
            match rule {
                Rule::CallRoute { methods, group_methods, receiver_types, file_imports, path_arg, handler_last, label } => {
                    let facts = self.facts;
                    let is_group =
                        |fi: usize, c: &CallFacts| group_methods.contains(&c.method) && Self::receiver_ok(&facts.files[fi], c, &receiver_types, &file_imports);
                    let flow = PrefixFlow::compute(facts, &is_group);
                    for file in files_sorted(facts) {
                        for c in &file.calls {
                            if b.flags(c.caller).intersects(NodeFlags::TEST | NodeFlags::MOCK) {
                                continue;
                            }
                            if !methods.contains(&c.method) || !Self::receiver_ok(file, c, &receiver_types, &file_imports) || c.string_args.len() < 2 {
                                continue;
                            }
                            let Some(Some(path)) = c.string_args.get(path_arg) else { continue };
                            if !(path.is_empty() || path.starts_with('/')) {
                                continue;
                            }
                            let prefix = flow.prefix_of(c.caller, c.receiver_var.as_deref().unwrap_or(""));
                            let full = join(&prefix, path);
                            let node = if handler_last { c.arg_targets.last().copied().flatten() } else { c.arg_targets.get(1).copied().flatten() };
                            let node = node.unwrap_or(c.caller);
                            let text = fill(&label, &[("METHOD", &c.method.to_uppercase()), ("path", &full)]);
                            self.add_child(b, node, file.module, "http-route", text, Lifecycle::OnEvent, 0, &id);
                        }
                    }
                }
                Rule::CallHandler { methods, receiver_types, file_imports, kind, label } => {
                    for file in files_sorted(self.facts) {
                        for c in &file.calls {
                            if !methods.contains(&c.method) || !Self::receiver_ok(file, c, &receiver_types, &file_imports) {
                                continue;
                            }
                            let arg0 = c
                                .string_args
                                .first()
                                .cloned()
                                .flatten()
                                .filter(|s| !s.is_empty())
                                .or_else(|| c.ident_args.first().cloned().flatten())
                                .unwrap_or_else(|| "?".into());
                            let caller = b.name(c.caller).to_string();
                            let text = fill(&label, &[("arg0", &arg0), ("caller", &caller)]);
                            self.add_child(b, c.caller, file.module, &kind, text, Lifecycle::OnEvent, 0, &id);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn route_target(&self, file: &FileFacts, target: &RouteTarget) -> Option<NodeId> {
        let simple = |t: &TypeFacts| t.fqn.rsplit('.').next().unwrap_or(&t.fqn).to_string();
        let (spec, member) = match target {
            RouteTarget::Local(name) => return file.types.iter().find(|t| simple(t) == *name).map(|t| t.node),
            RouteTarget::Import { spec, member } => (spec, member),
        };
        let stem = |rel: &str| rel.rsplit_once('.').map_or(rel.to_string(), |(s, _)| s.to_string());
        let candidates: Vec<&FileFacts> = if spec.starts_with('.') {
            let dir = file.rel_path.rsplit_once('/').map_or("", |(d, _)| d);
            let mut parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
            for seg in spec.split('/') {
                match seg {
                    "." | "" => {}
                    ".." => {
                        parts.pop();
                    }
                    other => parts.push(other),
                }
            }
            let base = parts.join("/");
            let base_stem = stem(&base);
            self.facts
                .files
                .iter()
                .filter(|f| {
                    let s = stem(&f.rel_path);
                    s == base || s == base_stem || s == format!("{base}/index")
                })
                .collect()
        } else {
            self.facts.files.iter().filter(|f| f.module == file.module).collect()
        };
        let types: Vec<&TypeFacts> = candidates.iter().flat_map(|f| f.types.iter()).filter(|t| member.as_ref().is_none_or(|m| simple(t) == *m)).collect();
        match types.as_slice() {
            [one] => Some(one.node),
            [first, ..] if spec.starts_with('.') && member.is_some() => Some(first.node),
            _ => None,
        }
    }

    fn object_routes(&mut self, sources: &dyn Fn(&str) -> Option<Vec<u8>>, b: &mut GraphBuilder) {
        let facts = self.facts;
        for file in files_sorted(facts) {
            let rules: Vec<(String, Rule)> =
                self.packs.rules_for(file.lang).filter(|(_, r)| matches!(r, Rule::ObjectRoute { .. })).map(|(id, r)| (id.to_string(), r.clone())).collect();
            if rules.is_empty() {
                continue;
            }
            let Some(src) = sources(&file.rel_path) else { continue };
            for (id, rule) in rules {
                let Rule::ObjectRoute { var_types, calls, path_key, target_keys, children_key, kind, label } = rule else { continue };
                let text = String::from_utf8_lossy(&src);
                if !var_types.iter().chain(&calls).any(|m| text.contains(m.as_str())) {
                    continue;
                }
                let shape = RouteShape { var_types: &var_types, calls: &calls, path_key: &path_key, target_keys: &target_keys, children_key: &children_key };
                for r in object_routes(&file.rel_path, &src, &shape) {
                    let node = r.target.as_ref().and_then(|t| self.route_target(file, t)).or_else(|| {
                        file.functions
                            .iter()
                            .filter(|f| {
                                let (s, e) = b.span(f.node);
                                s <= r.line && r.line <= e
                            })
                            .min_by_key(|f| {
                                let (s, e) = b.span(f.node);
                                e - s
                            })
                            .map(|f| f.node)
                    });
                    let Some(node) = node else { continue };
                    let text = fill(&label, &[("path", &r.path)]);
                    self.add_child(b, node, file.module, &kind, text, Lifecycle::OnEvent, 0, &id);
                }
            }
        }
    }

    fn queries(&mut self, sources: &dyn Fn(&str) -> Option<Vec<u8>>, b: &mut GraphBuilder) {
        let rules: Vec<(String, Rule)> = self.packs.rules().map(|(id, r)| (id.to_string(), r.clone())).collect();
        for (id, rule) in rules {
            let Rule::Query { language, query, capture, kind, label } = rule else { continue };
            for file in files_sorted(self.facts).into_iter().filter(|f| f.lang == language) {
                let Some(src) = sources(&file.rel_path) else { continue };
                for (line, text) in query::captures(language, &query, &capture, &src) {
                    let enclosing = file
                        .functions
                        .iter()
                        .filter(|f| {
                            let (s, e) = b.span(f.node);
                            s <= line && line <= e
                        })
                        .min_by_key(|f| {
                            let (s, e) = b.span(f.node);
                            e - s
                        });
                    let Some(f) = enclosing else { continue };
                    let text = fill(&label, &[("name", &text)]);
                    self.add_child(b, f.node, file.module, &kind, text, Lifecycle::Unknown, 0, &id);
                }
            }
        }
    }
}
