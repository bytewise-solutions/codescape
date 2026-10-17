use crate::model::*;
use memmap2::Mmap;
use std::fs::File;
use std::io::Write;
use std::ops::Range;
use std::path::Path;

pub const MAGIC: &[u8; 8] = b"SRCRLF01";
pub const FORMAT_VERSION: u32 = 11;
const HEADER: usize = 48;

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("index format version {found}, expected {expected}")]
    Version { found: u32, expected: u32 },
    #[error("corrupt index: {0}")]
    Corrupt(String),
}

pub fn write_graph(graph: &Graph, path: &Path) -> Result<(), FormatError> {
    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(graph).map_err(|e| FormatError::Corrupt(e.to_string()))?;
    let mut f = File::create(path)?;
    f.write_all(MAGIC)?;
    f.write_all(&FORMAT_VERSION.to_le_bytes())?;
    f.write_all(&[0u8; 4])?;
    f.write_all(blake3::hash(&bytes).as_bytes())?;
    f.write_all(&bytes)?;
    f.sync_all()?;
    Ok(())
}

pub struct ReliefFile {
    map: Mmap,
}

impl ReliefFile {
    pub fn open(path: &Path) -> Result<ReliefFile, FormatError> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        if len < HEADER as u64 {
            return Err(FormatError::Corrupt("file too short".into()));
        }
        let map = unsafe { Mmap::map(&file)? };
        if &map[..8] != MAGIC {
            return Err(FormatError::Corrupt("bad magic".into()));
        }
        let found = u32::from_le_bytes(map[8..12].try_into().unwrap());
        if found != FORMAT_VERSION {
            return Err(FormatError::Version { found, expected: FORMAT_VERSION });
        }
        if map[12..16] != [0u8; 4] || blake3::hash(&map[HEADER..]).as_bytes() != &map[16..HEADER] {
            return Err(FormatError::Corrupt("checksum mismatch".into()));
        }
        rkyv::access::<ArchivedGraph, rkyv::rancor::Error>(&map[HEADER..]).map_err(|e| FormatError::Corrupt(e.to_string()))?;
        Ok(ReliefFile { map })
    }

    pub fn graph(&self) -> &ArchivedGraph {
        unsafe { rkyv::access_unchecked::<ArchivedGraph>(&self.map[HEADER..]) }
    }

    pub fn byte_len(&self) -> usize {
        self.map.len()
    }
}

impl ArchivedGraph {
    pub fn root(&self) -> NodeId {
        0
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn str(&self, id: impl Into<StrRef>) -> &str {
        let id = id.into().0 as usize;
        let start = self.str_offsets[id].to_native() as usize;
        let end = self.str_offsets[id + 1].to_native() as usize;
        std::str::from_utf8(&self.strings[start..end]).unwrap_or("")
    }

    pub fn opt_str(&self, id: u32) -> Option<&str> {
        (id != NONE).then(|| self.str(id))
    }

    pub fn node(&self, id: NodeId) -> &ArchivedNode {
        &self.nodes[id as usize]
    }

    pub fn name(&self, id: NodeId) -> &str {
        self.str(self.node(id).name)
    }

    pub fn kind(&self, id: NodeId) -> NodeKind {
        NodeKind::from_u8(self.node(id).kind)
    }

    pub fn sub_kind(&self, id: NodeId) -> SubKind {
        SubKind::from_u8(self.node(id).sub)
    }

    pub fn visibility(&self, id: NodeId) -> Visibility {
        Visibility::from_u8(self.node(id).vis)
    }

    pub fn lang(&self, id: NodeId) -> Lang {
        Lang::from_u8(self.node(id).lang)
    }

    pub fn flags(&self, id: NodeId) -> NodeFlags {
        NodeFlags(self.node(id).flags)
    }

    pub fn lines(&self, id: NodeId) -> u32 {
        let n = self.node(id);
        let (s, e) = (n.start_line.to_native(), n.end_line.to_native());
        if n.file.to_native() == NONE || e < s { 0 } else { e - s + 1 }
    }

    pub fn subtree_lines(&self, id: NodeId) -> u64 {
        self.node(id).subtree_lines.to_native() as u64
    }

    pub fn children(&self, id: NodeId) -> Range<NodeId> {
        self.child_offsets[id as usize].to_native()..self.child_offsets[id as usize + 1].to_native()
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        let p = self.node(id).parent.to_native();
        (p != NONE).then_some(p)
    }

    pub fn depth(&self, mut id: NodeId) -> u32 {
        let mut d = 0;
        while let Some(p) = self.parent(id) {
            d += 1;
            id = p;
        }
        d
    }

    pub fn is_ancestor(&self, ancestor: NodeId, mut node: NodeId) -> bool {
        loop {
            if node == ancestor {
                return true;
            }
            match self.parent(node) {
                Some(p) => node = p,
                None => return false,
            }
        }
    }

    pub fn path(&self, mut id: NodeId) -> Vec<NodeId> {
        let mut out = vec![id];
        while let Some(p) = self.parent(id) {
            out.push(p);
            id = p;
        }
        out.reverse();
        out
    }

    pub fn file(&self, id: NodeId) -> Option<&str> {
        self.opt_str(self.node(id).file.to_native())
    }

    pub fn span(&self, id: NodeId) -> (u32, u32) {
        let n = self.node(id);
        (n.start_line.to_native(), n.end_line.to_native())
    }

    pub fn aggregates(&self, id: NodeId) -> &[ArchivedAgg] {
        let s = self.agg_offsets[id as usize].to_native() as usize;
        let e = self.agg_offsets[id as usize + 1].to_native() as usize;
        &self.aggs[s..e]
    }

    pub fn out_edges(&self, id: NodeId) -> &[ArchivedEdge] {
        let s = self.edge_index[id as usize].to_native() as usize;
        let e = self.edge_index[id as usize + 1].to_native() as usize;
        &self.edges[s..e]
    }

    pub fn tags(&self, id: NodeId) -> impl Iterator<Item = &str> {
        let s = self.tag_offsets[id as usize].to_native() as usize;
        let e = self.tag_offsets[id as usize + 1].to_native() as usize;
        self.tags[s..e].iter().map(|t| self.str(t.to_native()))
    }

    pub fn has_tag(&self, id: NodeId, tag: &str) -> bool {
        self.tags(id).any(|t| t == tag)
    }

    pub fn entries(&self) -> &[ArchivedEntryPoint] {
        &self.entries
    }

    pub fn entry_kind(&self, i: usize) -> &str {
        self.str(self.entries[i].kind)
    }

    pub fn entry_label(&self, i: usize) -> &str {
        self.str(self.entries[i].label)
    }

    pub fn entry_rule(&self, i: usize) -> &str {
        self.str(self.entries[i].rule)
    }

    pub fn entry_handoff(&self, i: usize) -> Option<&str> {
        self.opt_str(self.entries[i].handoff.to_native())
    }

    pub fn entry_parent(&self, i: usize) -> Option<usize> {
        let p = self.entries[i].parent.to_native();
        (p != NONE).then_some(p as usize)
    }

    pub fn entry_category(&self, i: usize) -> Category {
        Category::from_u8(self.entries[i].category)
    }

    pub fn entry_lifecycle(&self, i: usize) -> Lifecycle {
        Lifecycle::from_u8(self.entries[i].lifecycle)
    }

    pub fn stats(&self) -> &ArchivedGraphStats {
        &self.stats
    }

    pub fn skipped_roots(&self) -> impl Iterator<Item = (&str, &str)> {
        self.stats
            .skipped_kinds
            .iter()
            .zip(self.stats.skipped_paths.iter())
            .map(|(k, p)| (self.str(k.to_native()), self.str(p.to_native())))
    }

    pub fn provenance(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        self.provenance
            .iter()
            .map(|p| (self.str(p.name.to_native()), self.str(p.version.to_native()), self.str(p.hash.to_native())))
    }
}

pub struct StrRef(pub u32);

impl From<u32> for StrRef {
    fn from(v: u32) -> Self {
        StrRef(v)
    }
}

impl From<rkyv::rend::u32_le> for StrRef {
    fn from(v: rkyv::rend::u32_le) -> Self {
        StrRef(v.to_native())
    }
}

impl From<&rkyv::rend::u32_le> for StrRef {
    fn from(v: &rkyv::rend::u32_le) -> Self {
        StrRef(v.to_native())
    }
}
