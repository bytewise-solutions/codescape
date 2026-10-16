use crate::{ChildBox, ContainerLayout, Direction, FrameBox, LaidEdge, LayoutOptions, Rect, box_size};
use globset::Glob;
use srcrelief_format::{ArchivedGraph, NodeId, SCOPE_MAIN, SCOPE_TEST};
use srcrelief_rules::FrameRule;
use std::collections::HashMap;

pub const HEADER: f32 = 30.0;
pub const PAD: f32 = 24.0;
const GAP_X: f32 = 40.0;
const GAP_Y: f32 = 80.0;
const FRAME_PAD: f32 = 12.0;
const FRAME_LABEL: f32 = 16.0;
const BARYCENTER_SWEEPS: usize = 4;
const MIN_LAYER_CAP: usize = 8;

struct SibEdge {
    a: usize,
    b: usize,
    calls: u32,
    deps: u32,
    undeclared: bool,
    test_only: bool,
    back: bool,
}

pub fn layout_container(g: &ArchivedGraph, node: NodeId, frames: &[FrameRule], opts: &LayoutOptions) -> ContainerLayout {
    let kids: Vec<NodeId> = g.children(node).filter(|&c| !g.flags(c).intersects(opts.hidden)).collect();
    let n = kids.len();
    if n == 0 {
        return ContainerLayout { size: [0.0, 0.0], header: HEADER, ..Default::default() };
    }
    let index: HashMap<NodeId, usize> = kids.iter().enumerate().map(|(i, &k)| (k, i)).collect();
    let mut edges: Vec<SibEdge> = Vec::new();
    for agg in g.aggregates(node) {
        let (Some(&a), Some(&b)) = (index.get(&agg.a.to_native()), index.get(&agg.b.to_native())) else { continue };
        let test_only = agg.scope_mask & SCOPE_MAIN == 0 && agg.scope_mask & SCOPE_TEST != 0;
        if test_only && !opts.include_test_edges {
            continue;
        }
        edges.push(SibEdge { a, b, calls: agg.calls.to_native(), deps: agg.deps.to_native(), undeclared: agg.undeclared, test_only, back: false });
    }

    let mut out: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (ei, e) in edges.iter().enumerate() {
        out[e.a].push(ei);
    }
    mark_back_edges(n, &mut edges, &out);

    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut indeg = vec![0usize; n];
    for e in edges.iter().filter(|e| !e.back) {
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
    let mut pos = vec![0f32; n];
    for l in &layers {
        for (p, &v) in l.iter().enumerate() {
            pos[v] = p as f32;
        }
    }
    let mut neighbors: Vec<Vec<usize>> = vec![Vec::new(); n];
    for e in &edges {
        neighbors[e.a].push(e.b);
        neighbors[e.b].push(e.a);
    }
    for sweep in 0..BARYCENTER_SWEEPS {
        let order: Vec<usize> = if sweep % 2 == 0 { (1..layer_count).collect() } else { (0..layer_count.saturating_sub(1)).rev().collect() };
        for li in order {
            let reference = if sweep % 2 == 0 { li - 1 } else { li + 1 };
            let mut keyed: Vec<(f32, usize)> = layers[li]
                .iter()
                .map(|&v| {
                    let adj: Vec<f32> = neighbors[v].iter().filter(|&&w| layer[w] as usize == reference).map(|&w| pos[w]).collect();
                    let key = if adj.is_empty() { pos[v] } else { adj.iter().sum::<f32>() / adj.len() as f32 };
                    (key, v)
                })
                .collect();
            keyed.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
            layers[li] = keyed.into_iter().map(|(_, v)| v).collect();
            for (p, &v) in layers[li].iter().enumerate() {
                pos[v] = p as f32;
            }
        }
    }

    let groups = group_names(g, &kids, frames);
    for l in layers.iter_mut() {
        let mut first_pos: HashMap<&str, f32> = HashMap::new();
        for &v in l.iter() {
            if let Some(gname) = groups[v].as_deref() {
                first_pos.entry(gname).or_insert(pos[v]);
            }
        }
        l.sort_by(|&x, &y| {
            let kx = groups[x].as_deref().map_or(pos[x], |gname| first_pos[gname]);
            let ky = groups[y].as_deref().map_or(pos[y], |gname| first_pos[gname]);
            kx.total_cmp(&ky).then(pos[x].total_cmp(&pos[y]))
        });
    }

    let sizes: Vec<[f32; 2]> = kids.iter().map(|&k| box_size(g, k)).collect();
    let area: f32 = sizes.iter().map(|s| (s[0] + GAP_X) * (s[1] + GAP_Y)).sum();
    let widest = sizes.iter().map(|s| s[0]).fold(0.0, f32::max);
    let max_row = (area.sqrt() * 1.6).max(widest);

    let mut rows: Vec<(u32, Vec<usize>)> = Vec::new();
    for (li, l) in layers.iter().enumerate() {
        let mut row: Vec<usize> = Vec::new();
        let mut w = 0.0;
        for &v in l {
            let add = sizes[v][0] + if row.is_empty() { 0.0 } else { GAP_X };
            if !row.is_empty() && w + add > max_row {
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
            let dir = if e.back {
                Direction::Up
            } else if layer[e.a] == layer[e.b] || row_of[&e.a] == row_of[&e.b] {
                Direction::Sideways
            } else {
                Direction::Down
            };
            let points = match dir {
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
            LaidEdge { a: kids[e.a], b: kids[e.b], calls: e.calls, deps: e.deps, dir, undeclared: e.undeclared, test_only: e.test_only, points }
        })
        .collect();

    ContainerLayout {
        size: [content_w + 2.0 * PAD, height.max(HEADER + 2.0 * PAD)],
        header: HEADER,
        children: (0..n).map(|v| ChildBox { node: kids[v], rect: rects[v], layer: layer[v] }).collect(),
        edges: laid_edges,
        frames: frame_boxes,
    }
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
