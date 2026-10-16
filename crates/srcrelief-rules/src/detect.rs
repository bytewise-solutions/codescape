use crate::pack::{Packs, Rule};
use crate::query;
use crate::routes::{PrefixFlow, join};
use globset::Glob;
use srcrelief_facts::{CallFacts, FileFacts, FnFacts, ProjectFacts, TypeFacts};
use srcrelief_format::*;
use std::collections::{HashMap, HashSet, VecDeque};

pub const FALLBACK_RULE: &str = "builtin.fallback-callback";

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
    cx.queries(sources, b);
    cx.stats.entries = b.entries().len() - before;
    cx.stats
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
        for file in &facts.files {
            let imports = |prefixes: &[String]| file.imports.iter().any(|i| prefixes.iter().any(|p| i.path == *p || i.path.starts_with(&format!("{p}/")) || i.path.starts_with(&format!("{p}."))));
            let dirs: Vec<&str> = file.rel_path.split('/').collect();
            let dirs = &dirs[..dirs.len().saturating_sub(1)];
            for (_, rule) in self.packs.rules_for(file.lang) {
                match rule {
                    Rule::TestCode { file_imports, dir_names } => {
                        if imports(file_imports) || dirs.iter().any(|d| dir_names.iter().any(|n| n == d)) {
                            self.test_files.insert(file.rel_path.as_str());
                        }
                    }
                    Rule::Wiring { file_markers, file_imports, calls, annotations, type_prefixes } => {
                        let marked = !file_markers.is_empty()
                            && sources(&file.rel_path).is_some_and(|src| {
                                let head = String::from_utf8_lossy(&src[..src.len().min(1024)]).into_owned();
                                file_markers.iter().any(|m| head.contains(m.as_str()))
                            });
                        if marked || imports(file_imports) {
                            wiring_files.insert(file.rel_path.as_str());
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
                            let qualified = c.receiver_var.as_ref().map(|r| format!("{r}.{}", c.method));
                            let by_name = qualified.as_ref().is_some_and(|q| calls.contains(q));
                            let by_prefix = c.receiver_var.as_ref().into_iter().chain(c.receiver_type.as_ref()).any(|r| {
                                let simple = r.rsplit('.').next().unwrap_or(r);
                                type_prefixes.iter().any(|p| simple.starts_with(p.as_str()))
                            });
                            if by_name || by_prefix {
                                if let Some(t) = c.target {
                                    wiring_calls.insert((c.caller, t));
                                }
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
            for t in &file.types {
                b.add_flags(t.node, flag);
            }
            for f in &file.functions {
                b.add_flags(f.node, flag);
            }
        }
        for n in wiring_nodes {
            b.add_flags(n, NodeFlags::WIRING);
        }
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
            if e.kind == EdgeKind::DependsOn {
                if let Some(&(all_wiring, all_test)) = importers.get(&(e.from, e.to)) {
                    if all_test {
                        e.scope = Scope::Test;
                    } else if all_wiring {
                        e.flags |= EdgeFlags::WIRING;
                    }
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
            if let Rule::Path { glob, category } = rule {
                if Glob::new(glob).is_ok_and(|g| g.compile_matcher().is_match(&file.rel_path)) {
                    cat = *category;
                }
            }
        }
        cat
    }

    fn mains(&mut self, b: &mut GraphBuilder) {
        for file in files_sorted(self.facts) {
            let main_rule = self.packs.rules_for(file.lang).filter_map(|(id, r)| match r {
                Rule::Main { category } => Some((id.to_string(), *category)),
                _ => None,
            }).last();
            let Some((rule_id, default)) = main_rule else { continue };
            for func in file.functions.iter().filter(|f| f.is_main) {
                let label = match func.owner {
                    Some(o) => b.name(o).to_string(),
                    None => {
                        let pkg = b.parent(func.node).map(|p| b.name(p).to_string()).unwrap_or_default();
                        pkg.rsplit('/').next().unwrap_or(&pkg).to_string()
                    }
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
                    let is_group = |fi: usize, c: &CallFacts| group_methods.contains(&c.method) && Self::receiver_ok(&facts.files[fi], c, &receiver_types, &file_imports);
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
