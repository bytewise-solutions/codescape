use crate::model::*;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawEdge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
    pub scope: Scope,
    pub confidence: Confidence,
    pub flags: EdgeFlags,
}

struct BNode {
    name: u32,
    parent: NodeId,
    kind: NodeKind,
    sub: SubKind,
    lang: Lang,
    flags: NodeFlags,
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
            lines: 0,
            file: NONE,
            start_line: 0,
            end_line: 0,
            tags: Vec::new(),
        });
        self.children.push(Vec::new());
        id
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
        self.edges.push(RawEdge { from, to, kind, scope, confidence, flags });
    }

    pub fn edges(&self) -> &[RawEdge] {
        &self.edges
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
        order.push(0);
        new_id[0] = 0;
        let mut head = 0;
        while head < order.len() {
            let old = order[head] as usize;
            head += 1;
            for &c in &self.children[old] {
                new_id[c as usize] = order.len() as NodeId;
                order.push(c);
            }
        }

        let mut depth = vec![0u32; n];
        let mut parent_new = vec![NONE; n];
        for (new, &old) in order.iter().enumerate() {
            let p = self.nodes[old as usize].parent;
            if p != NONE {
                parent_new[new] = new_id[p as usize];
                depth[new] = depth[parent_new[new] as usize] + 1;
            }
        }

        let mut merged: HashMap<(u32, u32, u8, u8), (u8, u8, u32)> = HashMap::new();
        for e in &self.edges {
            let (from, to) = (new_id[e.from as usize], new_id[e.to as usize]);
            if from == to || from == NONE || to == NONE {
                continue;
            }
            let slot = merged.entry((from, to, e.kind as u8, e.scope as u8)).or_insert((e.confidence as u8, 0, 0));
            slot.0 = slot.0.min(e.confidence as u8);
            slot.1 |= e.flags.0;
            slot.2 += 1;
        }
        let mut edges: Vec<Edge> = merged
            .into_iter()
            .map(|((from, to, kind, scope), (confidence, flags, count))| Edge { from, to, kind, scope, confidence, flags, count })
            .collect();
        edges.sort_by_key(|e| (e.from, e.to, e.kind, e.scope));

        let mut edge_index = vec![0u32; n + 1];
        for e in &edges {
            edge_index[e.from as usize + 1] += 1;
        }
        for i in 0..n {
            edge_index[i + 1] += edge_index[i];
        }

        let mut agg_map: HashMap<(u32, u32, u32), Agg> = HashMap::new();
        for e in &edges {
            let (mut a, mut b) = (e.from, e.to);
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
            let agg = agg_map
                .entry((container, a, b))
                .or_insert(Agg { a, b, calls: 0, deps: 0, scope_mask: 0, undeclared: false });
            if EdgeKind::from_u8(e.kind).is_call_like() {
                agg.calls += e.count;
            } else {
                agg.deps += e.count;
            }
            agg.scope_mask |= Scope::from_u8(e.scope).mask();
            agg.undeclared |= EdgeFlags(e.flags).contains(EdgeFlags::UNDECLARED);
        }
        let mut agg_list: Vec<((u32, u32, u32), Agg)> = agg_map.into_iter().collect();
        agg_list.sort_by_key(|(k, _)| *k);
        let mut agg_range = vec![(0u32, 0u32); n];
        let mut aggs = Vec::with_capacity(agg_list.len());
        for ((container, _, _), agg) in agg_list {
            let r = &mut agg_range[container as usize];
            if r.1 == 0 {
                r.0 = aggs.len() as u32;
            }
            r.1 += 1;
            aggs.push(agg);
        }

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
        let mut nodes = Vec::with_capacity(n);
        for (new, &old) in order.iter().enumerate() {
            let b = &mut self.nodes[old as usize];
            let kids = &self.children[old as usize];
            let first_child = kids.first().map_or(0, |&c| new_id[c as usize]);
            b.tags.sort_by(|x, y| self.strings.list[*x as usize].cmp(&self.strings.list[*y as usize]));
            let tag_start = tags.len() as u32;
            tags.extend_from_slice(&b.tags);
            nodes.push(Node {
                name: b.name,
                parent: parent_new[new],
                first_child,
                child_count: kids.len() as u32,
                kind: b.kind as u8,
                sub: b.sub as u8,
                lang: b.lang as u8,
                flags: b.flags.0,
                subtree_lines: subtree[new].min(u32::MAX as u64) as u32,
                file: b.file,
                start_line: b.start_line,
                end_line: b.end_line,
                agg_start: agg_range[new].0,
                agg_len: agg_range[new].1,
                tag_start,
                tag_len: b.tags.len() as u32,
            });
        }

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

        Graph { strings, str_offsets, nodes, edges, edge_index, aggs, tags, entries, stats, provenance }
    }
}
