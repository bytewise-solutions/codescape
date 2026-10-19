use crate::model::*;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawEdge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
    pub scope: Scope,
    pub confidence: Confidence,
    pub flags: EdgeFlags,
    pub line: u32,
}

struct BNode {
    name: u32,
    parent: NodeId,
    kind: NodeKind,
    sub: SubKind,
    lang: Lang,
    flags: NodeFlags,
    vis: Visibility,
    detail: u32,
    complexity: u16,
    lines: u32,
    file: u32,
    start_line: u32,
    end_line: u32,
    tags: Vec<u32>,
}

#[derive(Default)]
struct Interner {
    map: HashMap<String, u32>,
    list: Vec<String>,
}

impl Interner {
    fn intern(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.map.get(s) {
            return id;
        }
        let id = self.list.len() as u32;
        self.list.push(s.to_owned());
        self.map.insert(s.to_owned(), id);
        id
    }
}

pub struct GraphBuilder {
    strings: Interner,
    nodes: Vec<BNode>,
    children: Vec<Vec<NodeId>>,
    child_index: HashMap<(NodeId, u32), NodeId>,
    edges: Vec<RawEdge>,
    refs: Vec<Ref>,
    entries: Vec<EntryPointSpec>,
    stats: Stats,
    provenance: Vec<(String, String, String)>,
}

impl GraphBuilder {
    pub fn new(project_name: &str) -> Self {
        let mut b = GraphBuilder {
            strings: Interner::default(),
            nodes: Vec::new(),
            children: Vec::new(),
            child_index: HashMap::new(),
            edges: Vec::new(),
            refs: Vec::new(),
            entries: Vec::new(),
            stats: Stats::default(),
            provenance: Vec::new(),
        };
        let name = b.strings.intern(project_name);
        b.push(name, NONE, NodeKind::Project, SubKind::None, Lang::None);
        b
    }

    fn push(&mut self, name: u32, parent: NodeId, kind: NodeKind, sub: SubKind, lang: Lang) -> NodeId {
        let id = self.nodes.len() as NodeId;
        self.nodes.push(BNode {
            name,
            parent,
            kind,
            sub,
            lang,
            flags: NodeFlags::NONE,
            vis: Visibility::Unknown,
            detail: NONE,
            complexity: 1,
            lines: 0,
            file: NONE,
            start_line: 0,
            end_line: 0,
            tags: Vec::new(),
        });
        self.children.push(Vec::new());
        id
    }

    fn is_field(&self, n: NodeId) -> bool {
        let node = &self.nodes[n as usize];
        node.kind == NodeKind::Member && node.sub == SubKind::Field
    }

    pub fn root(&self) -> NodeId {
        0
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn add_node(&mut self, parent: NodeId, kind: NodeKind, sub: SubKind, name: &str, lang: Lang) -> NodeId {
        let name = self.strings.intern(name);
        let id = self.push(name, parent, kind, sub, lang);
        self.children[parent as usize].push(id);
        self.child_index.entry((parent, name)).or_insert(id);
        id
    }

    pub fn child(&self, parent: NodeId, name: &str) -> Option<NodeId> {
        let name = *self.strings.map.get(name)?;
        self.child_index.get(&(parent, name)).copied()
    }

    pub fn children(&self, id: NodeId) -> &[NodeId] {
        &self.children[id as usize]
    }

    pub fn set_span(&mut self, node: NodeId, file: &str, start_line: u32, end_line: u32) {
        let file = self.strings.intern(file);
        let n = &mut self.nodes[node as usize];
        n.file = file;
        n.start_line = start_line;
        n.end_line = end_line;
    }

    pub fn set_lines(&mut self, node: NodeId, lines: u32) {
        self.nodes[node as usize].lines = lines;
    }

    pub fn add_flags(&mut self, node: NodeId, flags: NodeFlags) {
        self.nodes[node as usize].flags |= flags;
    }

    pub fn set_visibility(&mut self, node: NodeId, vis: Visibility) {
        self.nodes[node as usize].vis = vis;
    }

    pub fn set_complexity(&mut self, node: NodeId, value: u16) {
        self.nodes[node as usize].complexity = value.max(1);
    }

    pub fn complexity(&self, node: NodeId) -> u16 {
        self.nodes[node as usize].complexity
    }

    pub fn set_detail(&mut self, node: NodeId, detail: &str) {
        self.nodes[node as usize].detail = self.strings.intern(detail);
    }

    pub fn detail(&self, node: NodeId) -> Option<&str> {
        let d = self.nodes[node as usize].detail;
        (d != NONE).then(|| self.strings.list[d as usize].as_str())
    }

    pub fn visibility(&self, node: NodeId) -> Visibility {
        self.nodes[node as usize].vis
    }

    pub fn flags(&self, node: NodeId) -> NodeFlags {
        self.nodes[node as usize].flags
    }

    pub fn add_tag(&mut self, node: NodeId, tag: &str) {
        let tag = self.strings.intern(tag);
        let tags = &mut self.nodes[node as usize].tags;
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }

    pub fn has_tag(&self, node: NodeId, tag: &str) -> bool {
        self.strings.map.get(tag).is_some_and(|t| self.nodes[node as usize].tags.contains(t))
    }

    pub fn add_edge(&mut self, from: NodeId, to: NodeId, kind: EdgeKind, scope: Scope, confidence: Confidence, flags: EdgeFlags) {
        self.add_edge_at(from, to, kind, scope, confidence, flags, 0);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_edge_at(&mut self, from: NodeId, to: NodeId, kind: EdgeKind, scope: Scope, confidence: Confidence, flags: EdgeFlags, line: u32) {
        self.edges.push(RawEdge { from, to, kind, scope, confidence, flags, line });
    }

    pub fn add_ref(&mut self, from: NodeId, to: NodeId, line: u32, col: u32, len: u32) {
        let Ok(col) = u16::try_from(col) else { return };
        self.refs.push(Ref { from, to, line, col, len: u16::try_from(len).unwrap_or(u16::MAX) });
    }

    pub fn refs(&self) -> &[Ref] {
        &self.refs
    }

    pub fn edges(&self) -> &[RawEdge] {
        &self.edges
    }

    pub fn edges_mut(&mut self) -> &mut [RawEdge] {
        &mut self.edges
    }

    pub fn add_entry(&mut self, spec: EntryPointSpec) -> u32 {
        self.entries.push(spec);
        (self.entries.len() - 1) as u32
    }

    pub fn entries(&self) -> &[EntryPointSpec] {
        &self.entries
    }

    pub fn entries_mut(&mut self) -> &mut [EntryPointSpec] {
        &mut self.entries
    }

    pub fn node_kind(&self, id: NodeId) -> NodeKind {
        self.nodes[id as usize].kind
    }

    pub fn sub_kind(&self, id: NodeId) -> SubKind {
        self.nodes[id as usize].sub
    }

    pub fn lang(&self, id: NodeId) -> Lang {
        self.nodes[id as usize].lang
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        let p = self.nodes[id as usize].parent;
        (p != NONE).then_some(p)
    }

    pub fn name(&self, id: NodeId) -> &str {
        &self.strings.list[self.nodes[id as usize].name as usize]
    }

    pub fn file(&self, id: NodeId) -> Option<&str> {
        let f = self.nodes[id as usize].file;
        (f != NONE).then(|| self.strings.list[f as usize].as_str())
    }

    pub fn span(&self, id: NodeId) -> (u32, u32) {
        let n = &self.nodes[id as usize];
        (n.start_line, n.end_line)
    }

    pub fn set_stats(&mut self, stats: Stats) {
        self.stats = stats;
    }

    pub fn add_provenance(&mut self, pack: &str, version: &str, hash: &str) {
        self.provenance.push((pack.into(), version.into(), hash.into()));
    }

    pub fn finish(mut self) -> Graph {
        let n = self.nodes.len();
        let mut order: Vec<NodeId> = Vec::with_capacity(n);
        let mut new_id = vec![NONE; n];
        let mut child_offsets = vec![0u32; n + 1];
        order.push(0);
        new_id[0] = 0;
        let mut head = 0;
        while head < order.len() {
            let old = order[head] as usize;
            child_offsets[head] = order.len() as u32;
            head += 1;
            for &c in &self.children[old] {
                if self.is_field(c) {
                    continue;
                }
                new_id[c as usize] = order.len() as NodeId;
                order.push(c);
            }
        }
        let mut fields: Vec<FieldRec> = Vec::new();
        for (new, &old) in order.iter().enumerate() {
            for &c in self.children[old as usize].iter().filter(|&&c| self.is_field(c)) {
                let f = &self.nodes[c as usize];
                fields.push(FieldRec { owner: new as u32, name: f.name, ty: f.detail, line: f.start_line, vis: f.vis as u8 });
            }
        }
        let complexity: Vec<Metric> = order
            .iter()
            .enumerate()
            .filter(|(_, old)| self.nodes[**old as usize].complexity > 1)
            .map(|(new, &old)| Metric { node: new as u32, value: self.nodes[old as usize].complexity })
            .collect();
        let n = order.len();
        child_offsets.truncate(n + 1);
        child_offsets[n] = n as u32;
        let mut depth = vec![0u32; n];
        let mut parent_new = vec![NONE; n];
        for (new, &old) in order.iter().enumerate() {
            let p = self.nodes[old as usize].parent;
            if p != NONE {
                parent_new[new] = new_id[p as usize];
                depth[new] = depth[parent_new[new] as usize] + 1;
            }
        }

        for &old in &order {
            let inherited = NodeFlags(self.nodes[old as usize].flags.0 & (NodeFlags::WIRING.0 | NodeFlags::NOISE.0));
            if inherited.0 != 0 {
                for &c in &self.children[old as usize] {
                    self.nodes[c as usize].flags |= inherited;
                }
            }
        }
        for &old in order.iter().rev() {
            let kids = &self.children[old as usize];
            if old == 0 || kids.is_empty() || self.nodes[old as usize].kind == NodeKind::Type {
                continue;
            }
            let flags: Vec<NodeFlags> = kids.iter().map(|&c| self.nodes[c as usize].flags).collect();
            let live: Vec<&NodeFlags> = flags.iter().filter(|f| !f.contains(NodeFlags::TEST)).collect();
            if live.is_empty() {
                self.nodes[old as usize].flags |= NodeFlags::TEST;
            } else if live.iter().all(|f| f.contains(NodeFlags::WIRING)) {
                self.nodes[old as usize].flags |= NodeFlags::WIRING;
            } else if self.nodes[old as usize].kind == NodeKind::Package
                && live.iter().all(|f| f.intersects(NodeFlags::DETAIL | NodeFlags::WIRING | NodeFlags::NOISE | NodeFlags::MOCK))
                && live.iter().any(|f| f.contains(NodeFlags::DETAIL))
            {
                self.nodes[old as usize].flags |= NodeFlags::DETAIL;
            }
        }

        let mut merged: HashMap<(u32, u32, u8, u8), (u8, u8, u16, u16)> = HashMap::new();
        for e in &self.edges {
            let (from, to) = (new_id[e.from as usize], new_id[e.to as usize]);
            if from == to || from == NONE || to == NONE {
                continue;
            }
            let touching = self.nodes[e.from as usize].flags | self.nodes[e.to as usize].flags;
            let scope = if touching.contains(NodeFlags::TEST) { Scope::Test } else { e.scope };
            let mut flags = e.flags;
            if touching.contains(NodeFlags::WIRING) {
                flags |= EdgeFlags::WIRING;
            }
            if touching.contains(NodeFlags::NOISE) {
                flags |= EdgeFlags::NOISE;
            }
            let start = self.nodes[e.from as usize].start_line;
            let offset = if e.line >= start && e.line > 0 { u16::try_from(e.line - start + 1).unwrap_or(0) } else { 0 };
            let slot = merged.entry((from, to, e.kind as u8, scope as u8)).or_insert((e.confidence as u8, 0, 0, 0));
            slot.0 = slot.0.min(e.confidence as u8);
            slot.1 |= flags.0;
            slot.2 = slot.2.saturating_add(1);
            if offset > 0 && (slot.3 == 0 || offset < slot.3) {
                slot.3 = offset;
            }
        }
        let mut sorted: Vec<(u32, Edge)> = merged
            .into_iter()
            .map(|((from, to, kind, scope), (confidence, flags, count, line))| (from, Edge { to, kind, scope, confidence, flags, count, line }))
            .collect();
        sorted.sort_by_key(|(from, e)| (*from, e.to, e.kind, e.scope));

        let mut edge_index = vec![0u32; n + 1];
        for (from, _) in &sorted {
            edge_index[*from as usize + 1] += 1;
        }
        for i in 0..n {
            edge_index[i + 1] += edge_index[i];
        }

        let mut carrier: Vec<NodeId> = (0..n as NodeId).collect();
        let mut in_type = vec![false; n];
        for (new, &old) in order.iter().enumerate() {
            let p = parent_new[new];
            if self.nodes[old as usize].kind == NodeKind::Type && (p == NONE || !in_type[p as usize]) {
                in_type[new] = true;
            } else if p != NONE && in_type[p as usize] {
                in_type[new] = true;
                carrier[new] = carrier[p as usize];
            }
        }
        let mut linked: HashSet<(u32, u32, u32, u32, u32)> = HashSet::new();
        let mut agg_map: HashMap<(u32, u32, u32), Agg> = HashMap::new();
        let mut guessed: HashMap<(u32, u32, u32), u32> = HashMap::new();
        for (from, e) in &sorted {
            if EdgeFlags(e.flags).contains(EdgeFlags::NOISE) {
                continue;
            }
            let (mut a, mut b) = (*from, e.to);
            while depth[a as usize] > depth[b as usize] {
                a = parent_new[a as usize];
            }
            while depth[b as usize] > depth[a as usize] {
                b = parent_new[b as usize];
            }
            if a == b {
                continue;
            }
            while parent_new[a as usize] != parent_new[b as usize] {
                a = parent_new[a as usize];
                b = parent_new[b as usize];
            }
            let container = parent_new[a as usize];
            let agg = agg_map.entry((container, a, b)).or_insert(Agg { a, b, calls: 0, deps: 0, wiring: 0, implements: 0, links: 0, scope_mask: 0, marks: 0 });
            let kind = EdgeKind::from_u8(e.kind);
            let structural = !EdgeFlags(e.flags).contains(EdgeFlags::WIRING) && !matches!(kind, EdgeKind::Implements | EdgeKind::Inherits);
            if structural && Scope::from_u8(e.scope) == Scope::Main && linked.insert((container, a, b, carrier[*from as usize], carrier[e.to as usize])) {
                agg.links = agg.links.saturating_add(1);
            }
            let add = e.count;
            let slot = if EdgeFlags(e.flags).contains(EdgeFlags::WIRING) {
                &mut agg.wiring
            } else if matches!(kind, EdgeKind::Implements | EdgeKind::Inherits) {
                &mut agg.implements
            } else if kind.is_call_like() {
                &mut agg.calls
            } else {
                &mut agg.deps
            };
            *slot = slot.saturating_add(add);
            agg.scope_mask |= Scope::from_u8(e.scope).mask();
            if EdgeFlags(e.flags).contains(EdgeFlags::UNDECLARED) {
                agg.marks |= AGG_UNDECLARED;
            }
            if kind.is_call_like() && !EdgeFlags(e.flags).contains(EdgeFlags::WIRING) && e.confidence == Confidence::Guessed as u8 {
                *guessed.entry((container, a, b)).or_default() += u32::from(add);
            }
        }
        for (key, agg) in agg_map.iter_mut() {
            let only_guesses = guessed.get(key).is_some_and(|&n| n == u32::from(agg.calls));
            if only_guesses && agg.calls > 0 && agg.deps == 0 && agg.implements == 0 && agg.wiring == 0 {
                agg.marks |= AGG_GUESSED;
            }
        }
        let mut agg_list: Vec<((u32, u32, u32), Agg)> = agg_map.into_iter().collect();
        agg_list.sort_by_key(|(k, _)| *k);
        let mut agg_offsets = vec![0u32; n + 1];
        let mut aggs = Vec::with_capacity(agg_list.len());
        for ((container, _, _), agg) in agg_list {
            agg_offsets[container as usize + 1] += 1;
            aggs.push(agg);
        }
        for i in 0..n {
            agg_offsets[i + 1] += agg_offsets[i];
        }
        let edges: Vec<Edge> = sorted.into_iter().map(|(_, e)| e).collect();
        let mut refs: Vec<Ref> = std::mem::take(&mut self.refs)
            .into_iter()
            .map(|r| Ref { from: new_id[r.from as usize], to: new_id[r.to as usize], ..r })
            .filter(|r| r.from != NONE && r.to != NONE)
            .collect();
        refs.sort_by_key(|r| (r.from, r.line, r.col, r.to, r.len));
        refs.dedup();

        let mut subtree = vec![0u64; n];
        for new in (0..n).rev() {
            let old = order[new] as usize;
            let own = self.nodes[old].lines as u64;
            if own > 0 {
                subtree[new] = own;
            }
            let total = subtree[new];
            let p = parent_new[new];
            if p != NONE && self.nodes[order[p as usize] as usize].lines == 0 {
                subtree[p as usize] += total;
            }
        }

        let mut tags = Vec::new();
        let mut tag_offsets = Vec::with_capacity(n + 1);
        let mut nodes = Vec::with_capacity(n);
        for (new, &old) in order.iter().enumerate() {
            let b = &mut self.nodes[old as usize];
            b.tags.sort_by(|x, y| self.strings.list[*x as usize].cmp(&self.strings.list[*y as usize]));
            tag_offsets.push(tags.len() as u32);
            tags.extend_from_slice(&b.tags);
            nodes.push(Node {
                name: b.name,
                parent: parent_new[new],
                kind: b.kind as u8,
                sub: b.sub as u8,
                lang_vis: b.lang as u8 | (b.vis as u8) << 4,
                flags: b.flags.0,
                subtree_lines: subtree[new].min(u32::MAX as u64) as u32,
                file: b.file,
                start_line: b.start_line,
                end_line: b.end_line,
            });
        }
        tag_offsets.push(tags.len() as u32);

        let opt = |s: &Option<String>, strings: &mut Interner| s.as_deref().map_or(NONE, |s| strings.intern(s));
        let entries: Vec<EntryPoint> = self
            .entries
            .iter()
            .map(|e| EntryPoint {
                node: new_id[e.node as usize],
                kind: self.strings.intern(&e.kind),
                category: e.category as u8,
                lifecycle: e.lifecycle as u8,
                label: self.strings.intern(&e.label),
                parent: e.parent.unwrap_or(NONE),
                order: e.order,
                rule: self.strings.intern(&e.rule),
                handoff: opt(&e.handoff, &mut self.strings),
            })
            .collect();

        let provenance = std::mem::take(&mut self.provenance)
            .into_iter()
            .map(|(a, b, c)| PackStamp { name: self.strings.intern(&a), version: self.strings.intern(&b), hash: self.strings.intern(&c) })
            .collect();
        let s = std::mem::take(&mut self.stats);
        let mut skipped_kinds = Vec::new();
        let mut skipped_paths = Vec::new();
        for (k, p) in &s.skipped_roots {
            skipped_kinds.push(self.strings.intern(k));
            skipped_paths.push(self.strings.intern(p));
        }
        let stats = GraphStats {
            files: s.files,
            lines: s.lines,
            source_bytes: s.source_bytes,
            calls_resolved: s.calls_resolved,
            calls_external: s.calls_external,
            calls_guessed: s.calls_guessed,
            calls_unresolved: s.calls_unresolved,
            skipped_kinds,
            skipped_paths,
        };

        let mut strings = Vec::new();
        let mut str_offsets = Vec::with_capacity(self.strings.list.len() + 1);
        for s in &self.strings.list {
            str_offsets.push(strings.len() as u32);
            strings.extend_from_slice(s.as_bytes());
        }
        str_offsets.push(strings.len() as u32);

        Graph {
            strings,
            str_offsets,
            nodes,
            child_offsets,
            edges,
            edge_index,
            aggs,
            agg_offsets,
            tags,
            tag_offsets,
            entries,
            stats,
            provenance,
            fields,
            complexity,
            refs,
        }
    }
}
