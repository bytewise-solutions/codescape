use globset::GlobSet;
use srcrelief_build::Discovery;
use srcrelief_facts::{ExtractStats, ProjectFacts};
use srcrelief_format::{GraphBuilder, NodeId};

pub fn extract_rust(_d: &Discovery, _module_nodes: &[NodeId], _excludes: &GlobSet, _b: &mut GraphBuilder, _facts: &mut ProjectFacts) -> ExtractStats {
    ExtractStats::default()
}
