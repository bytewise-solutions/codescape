use crate::order::{Ordering, minimize};
use crate::{ChildBox, ContainerLayout, Direction, FrameBox, LaidEdge, LayoutOptions, Rect, box_size};
use globset::Glob;
use srcrelief_format::{ArchivedGraph, Category, NodeId, SCOPE_MAIN, SCOPE_TEST, SubKind};
use std::collections::HashSet;
use srcrelief_rules::FrameRule;
use std::collections::HashMap;

pub const HEADER: f32 = 30.0;
pub const PAD: f32 = 24.0;
const GAP_X: f32 = 40.0;
const GAP_Y: f32 = 80.0;
const FRAME_PAD: f32 = 12.0;
const FRAME_LABEL: f32 = 16.0;
const MIN_LAYER_CAP: usize = 8;

struct SibEdge {
    a: usize,
    b: usize,
    calls: u32,
    deps: u32,
    wiring: u32,
    implements: u32,
    undeclared: bool,
    test_only: bool,
    layering: bool,
    back: bool,
}

fn entry_owners(g: &ArchivedGraph) -> HashSet<NodeId> {
    let mut out = HashSet::new();
    for i in 0..g.entries().len() {
        if g.entry_category(i) == Category::Application && matches!(g.entry_kind(i), "main" | "framework-annotation") {
            out.extend(g.path(g.entries()[i].node.to_native()));
        }
    }
    out
}

pub fn layout_container(g: &ArchivedGraph, node: NodeId, frames: &[FrameRule], opts: &LayoutOptions) -> ContainerLayout {
    let hidden = opts.effective_hidden();
    let kids: Vec<NodeId> = g.children(node).filter(|&c| !g.flags(c).intersects(hidden)).collect();
    let n = kids.len();
    if n == 0 {
        return ContainerLayout { size: [0.0, 0.0], header: HEADER, ..Default::default() };
    }
    let owners = entry_owners(g);
    let pinned: Vec<bool> = kids.iter().map(|k| owners.contains(k)).collect();
    let index: HashMap<NodeId, usize> = kids.iter().enumerate().map(|(i, &k)| (k, i)).collect();
    let mut edges: Vec<SibEdge> = Vec::new();
    for agg in g.aggregates(node) {
        let (Some(&a), Some(&b)) = (index.get(&agg.a.to_native()), index.get(&agg.b.to_native())) else { continue };
        let test_only = agg.scope_mask & SCOPE_MAIN == 0 && agg.scope_mask & SCOPE_TEST != 0;
        if test_only && !opts.include_test_edges {
            continue;
        }
        let (calls, deps, wiring, implements) = (agg.calls.to_native(), agg.deps.to_native(), agg.wiring.to_native(), agg.implements.to_native());
        let structural = calls + deps + implements > 0;
        if !structural && !opts.show_wiring {
            continue;
        }
        let wiring = if opts.show_wiring { wiring } else { 0 };
        edges.push(SibEdge { a, b, calls, deps, wiring, implements, undeclared: agg.undeclared, test_only, layering: structural && !pinned[b], back: false });
    }

    let mut out: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (ei, e) in edges.iter().enumerate() {
        if e.layering {
            out[e.a].push(ei);
        }
    }
    mark_back_edges(n, &mut edges, &out);

    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut indeg = vec![0usize; n];
    for e in edges.iter().filter(|e| e.layering && !e.back) {
        succ[e.a].push(e.b);
        indeg[e.b] += 1;
    }
    let mut layer = vec![0u32; n];
    let mut queue: Vec<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
    let mut head = 0;
    while head < queue.len() {
        let v = queue[head];
        head += 1;
        for &w in &succ[v] {
            layer[w] = layer[w].max(layer[v] + 1);
            indeg[w] -= 1;
            if indeg[w] == 0 {
                queue.push(w);
            }
        }
    }

    let raw_layer = layer.clone();
    let raw_layers = layer.iter().max().map_or(0, |m| *m as usize + 1);
    let max_layers = ((n as f32).sqrt().ceil() as usize).max(MIN_LAYER_CAP);
    if raw_layers > max_layers {
        for l in layer.iter_mut() {
            *l = (*l as usize * max_layers / raw_layers) as u32;
        }
    }
    let layer_count = layer.iter().max().map_or(0, |m| *m as usize + 1);
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); layer_count];
    for i in 0..n {
        layers[layer[i] as usize].push(i);
    }

    let groups = group_names(g, &kids, frames);
    let mut group_ids: HashMap<&str, u32> = HashMap::new();
    let group_keys: Vec<Option<u32>> = groups
        .iter()
        .map(|gname| gname.as_deref().map(|s| {
            let next = group_ids.len() as u32;
            *group_ids.entry(s).or_insert(next)
        }))
        .collect();
    let mut pinned_names: Vec<(&str, usize)> = (0..n).filter(|&i| pinned[i]).map(|i| (g.name(kids[i]), i)).collect();
    pinned_names.sort();
    let mut first_keys: Vec<Option<u32>> = vec![None; n];
    for (rank, (_, i)) in pinned_names.iter().enumerate() {
        first_keys[*i] = Some(rank as u32);
    }
    for i in 0..n {
        if first_keys[i].is_none() && g.sub_kind(kids[i]) == SubKind::Constructor {
            first_keys[i] = Some(u32::MAX);
        }
    }
    let order_edges: Vec<(usize, usize)> = edges.iter().filter(|e| e.calls + e.deps + e.implements > 0).map(|e| (e.a, e.b)).collect();
    let layers = minimize(&layers, &layer, &order_edges, &Ordering { first_keys, groups: group_keys, max_sweeps: 0 });

    let sizes: Vec<[f32; 2]> = kids.iter().map(|&k| box_size(g, k)).collect();
    let area: f32 = sizes.iter().map(|s| (s[0] + GAP_X) * (s[1] + GAP_Y)).sum();
    let widest = sizes.iter().map(|s| s[0]).fold(0.0, f32::max);
    let max_row = (area.sqrt() * 1.6).max(widest);

    let mut rows: Vec<(u32, Vec<usize>)> = Vec::new();
    for (li, l) in layers.iter().enumerate() {
        let mut row: Vec<usize> = Vec::new();
        let mut w = 0.0;
        let keep_whole = l.iter().any(|&v| pinned[v]);
        for &v in l {
            let add = sizes[v][0] + if row.is_empty() { 0.0 } else { GAP_X };
            if !keep_whole && !row.is_empty() && w + add > max_row {
                rows.push((li as u32, std::mem::take(&mut row)));
                w = 0.0;
            }
            w += sizes[v][0] + if row.is_empty() { 0.0 } else { GAP_X };
            row.push(v);
        }
        if !row.is_empty() {
            rows.push((li as u32, row));
        }
    }
    let row_widths: Vec<f32> = rows.iter().map(|(_, r)| r.iter().map(|&v| sizes[v][0]).sum::<f32>() + GAP_X * (r.len() as f32 - 1.0)).collect();
    let content_w = row_widths.iter().copied().fold(0.0, f32::max);
    let mut rects = vec![Rect::default(); n];
    let mut y = HEADER + PAD;
    let frame_top_room = if groups.iter().any(|g| g.is_some()) { FRAME_LABEL + FRAME_PAD } else { 0.0 };
    y += frame_top_room;
    for ((_, row), rw) in rows.iter().zip(&row_widths) {
        let row_h = row.iter().map(|&v| sizes[v][1]).fold(0.0, f32::max);
        let mut x = PAD + (content_w - rw) / 2.0;
        for &v in row {
            rects[v] = Rect::new(x, y + (row_h - sizes[v][1]) / 2.0, sizes[v][0], sizes[v][1]);
            x += sizes[v][0] + GAP_X;
        }
        y += row_h + GAP_Y;
    }
    let height = y - GAP_Y + PAD + frame_top_room;

    let mut frame_boxes: Vec<FrameBox> = Vec::new();
    for (_, row) in &rows {
        let mut labels: Vec<&str> = row.iter().filter_map(|&v| groups[v].as_deref()).collect();
        labels.dedup();
        for label in labels {
            let members: Vec<usize> = row.iter().copied().filter(|&v| groups[v].as_deref() == Some(label)).collect();
            let bbox = members.iter().skip(1).fold(rects[members[0]], |acc, &v| acc.union(&rects[v]));
            let r = bbox.expand(FRAME_PAD);
            frame_boxes.push(FrameBox { label: label.to_string(), rect: Rect::new(r.x, r.y - FRAME_LABEL, r.w, r.h + FRAME_LABEL) });
        }
    }

    let row_of: HashMap<usize, usize> = rows.iter().enumerate().flat_map(|(ri, (_, r))| r.iter().map(move |&v| (v, ri))).collect();
    let laid_edges = edges
        .iter()
        .map(|e| {
            let (ra, rb) = (rects[e.a], rects[e.b]);
            let dir = if e.back || raw_layer[e.a] > raw_layer[e.b] {
                Direction::Up
            } else if raw_layer[e.a] == raw_layer[e.b] {
                Direction::Sideways
            } else {
                Direction::Down
            };
            let same_row = row_of[&e.a] == row_of[&e.b];
            let side_points = || if ra.x < rb.x { vec![[ra.right(), ra.center()[1]], [rb.x, rb.center()[1]]] } else { vec![[ra.x, ra.center()[1]], [rb.right(), rb.center()[1]]] };
            let points = match dir {
                Direction::Down if same_row => side_points(),
                Direction::Down => vec![[toward(&ra, rb.center()[0]), ra.bottom()], [toward(&rb, ra.center()[0]), rb.y]],
                Direction::Up => {
                    let (from, to) = ([toward(&ra, rb.center()[0]), ra.y], [toward(&rb, ra.center()[0]), rb.bottom()]);
                    let mid = [(from[0] + to[0]) / 2.0 + GAP_X, (from[1] + to[1]) / 2.0];
                    vec![from, mid, to]
                }
                Direction::Sideways => {
                    let (from, to) = if ra.x < rb.x { ([ra.right(), ra.center()[1]], [rb.x, rb.center()[1]]) } else { ([ra.x, ra.center()[1]], [rb.right(), rb.center()[1]]) };
                    vec![from, to]
                }
            };
            LaidEdge { a: kids[e.a], b: kids[e.b], calls: e.calls, deps: e.deps, wiring: e.wiring, implements: e.implements, dir, undeclared: e.undeclared, test_only: e.test_only, points }
        })
        .collect();

    ContainerLayout {
        size: [content_w + 2.0 * PAD, height.max(HEADER + 2.0 * PAD)],
        header: HEADER,
        children: (0..n).map(|v| ChildBox { node: kids[v], rect: rects[v], layer: layer[v], pinned: pinned[v] }).collect(),
        edges: laid_edges,
        frames: frame_boxes,
    }
}

pub fn count_crossings(l: &ContainerLayout) -> usize {
    let segs: Vec<([f32; 2], [f32; 2])> = l.edges.iter().filter_map(|e| Some((*e.points.first()?, *e.points.last()?))).collect();
    let cross = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    let near = |p: [f32; 2], q: [f32; 2]| (p[0] - q[0]).abs() < 1.0 && (p[1] - q[1]).abs() < 1.0;
    let mut total = 0;
    for i in 0..segs.len() {
        for j in i + 1..segs.len() {
            let ((p1, p2), (q1, q2)) = (segs[i], segs[j]);
            if near(p1, q1) || near(p1, q2) || near(p2, q1) || near(p2, q2) {
                continue;
            }
            let d1 = cross(q1, q2, p1);
            let d2 = cross(q1, q2, p2);
            let d3 = cross(p1, p2, q1);
            let d4 = cross(p1, p2, q2);
            if d1 * d2 < 0.0 && d3 * d4 < 0.0 {
                total += 1;
            }
        }
    }
    total
}

fn toward(r: &Rect, x: f32) -> f32 {
    let inset = (r.w * 0.15).min(16.0);
    x.clamp(r.x + inset, r.right() - inset)
}

fn mark_back_edges(n: usize, edges: &mut [SibEdge], out: &[Vec<usize>]) {
    let mut state = vec![0u8; n];
    for start in 0..n {
        if state[start] != 0 {
            continue;
        }
        let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
        state[start] = 1;
        while let Some(&mut (v, ref mut next)) = stack.last_mut() {
            if *next < out[v].len() {
                let ei = out[v][*next];
                *next += 1;
                let w = edges[ei].b;
                match state[w] {
                    0 => {
                        state[w] = 1;
                        stack.push((w, 0));
                    }
                    1 => edges[ei].back = true,
                    _ => {}
                }
            } else {
                state[v] = 2;
                stack.pop();
            }
        }
    }
}

fn group_names(g: &ArchivedGraph, kids: &[NodeId], frames: &[FrameRule]) -> Vec<Option<String>> {
    let names: Vec<&str> = kids.iter().map(|&k| g.name(k)).collect();
    let mut out: Vec<Option<String>> = vec![None; kids.len()];
    for rule in frames {
        match rule {
            FrameRule::Pattern { glob, label } => {
                let Ok(m) = Glob::new(glob).map(|g| g.compile_matcher()) else { continue };
                for (i, name) in names.iter().enumerate() {
                    if out[i].is_none() && m.is_match(name) {
                        out[i] = Some(label.clone());
                    }
                }
            }
            FrameRule::AutoPrefix { sep } => {
                let mut counts: HashMap<&str, usize> = HashMap::new();
                for name in &names {
                    if let Some((prefix, _)) = name.split_once(sep.as_str()) {
                        *counts.entry(prefix).or_default() += 1;
                    }
                }
                for (i, name) in names.iter().enumerate() {
                    if out[i].is_some() {
                        continue;
                    }
                    if let Some((prefix, _)) = name.split_once(sep.as_str()) {
                        if counts[prefix] >= 2 && counts[prefix] < names.len() {
                            out[i] = Some(prefix.to_string());
                        }
                    }
                }
            }
        }
    }
    out
}
