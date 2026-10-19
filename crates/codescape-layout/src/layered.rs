use crate::fold::{display_children, folded_into};
use crate::order::{Ordering, minimize};
use crate::{ChildBox, ContainerLayout, Direction, FrameBox, LaidEdge, LayoutOptions, Rect, box_size, card_rows_bottom};
use codescape_format::{ArchivedGraph, Category, NodeFlags, NodeId, NodeKind, SCOPE_MAIN, SCOPE_TEST, SubKind};
use codescape_rules::FrameRule;
use globset::Glob;
use std::collections::HashMap;
use std::collections::HashSet;

pub const HEADER: f32 = 30.0;
pub const PAD: f32 = 24.0;
const GAP_X: f32 = 40.0;
const GAP_Y: f32 = 80.0;
const FRAME_PAD: f32 = 12.0;
const FRAME_LABEL: f32 = 16.0;
const MIN_LAYER_CAP: usize = 8;
const MIN_ASPECT: f32 = 0.6;
const HEADER_SHARE: f32 = 0.045;
const BOX_ASPECT: f32 = 1.6;
const PINNED_TOGETHER: usize = 8;
const ROLE_LAYERS: usize = 4;
const FOLDED_ROWS: usize = 3;

struct SibEdge {
    a: usize,
    b: usize,
    calls: u32,
    deps: u32,
    wiring: u32,
    implements: u32,
    links: u32,
    undeclared: bool,
    guessed: bool,
    test_only: bool,
    layering: bool,
    back: bool,
}

const ROLE_ORDER: [&str; 4] = ["layer:handler", "layer:service", "layer:repository", "layer:model"];

fn role_rank(g: &ArchivedGraph, n: NodeId) -> Option<usize> {
    g.tags(n).filter_map(|t| ROLE_ORDER.iter().position(|r| *r == t)).min()
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
    let mut base = layout_structure(g, node, frames, &structure_options(opts));
    let tests = if opts.show_tests { NodeFlags::TEST.0 | NodeFlags::MOCK.0 } else { 0 };
    let other = NodeFlags(opts.hidden.0 & !NodeFlags::DETAIL.0 & !tests);
    let details = display_children(g, node)
        .into_iter()
        .filter(|&c| g.flags(c).contains(NodeFlags::DETAIL) && !g.flags(c).intersects(other) && !opts.tag_hidden(g, c))
        .count();
    let shown = opts.details_shown_in(node);
    base.hidden_details = if shown { 0 } else { details };
    base.shown_details = if shown && !opts.show_details { details } else { 0 };
    let laid = if !opts.show_wiring && !opts.show_noise && !shown { base } else { add_wiring(g, node, base, opts) };
    if g.kind(node) == NodeKind::Type { laid } else { room_for_title(laid) }
}

fn room_for_title(mut l: ContainerLayout) -> ContainerLayout {
    let body = l.size[1] - l.header;
    let by_width = l.size[0] * HEADER_SHARE;
    let by_height = BOX_ASPECT * HEADER_SHARE * body / (1.0 - BOX_ASPECT * HEADER_SHARE);
    let wanted = by_width.max(by_height).max(l.header);
    let shift = wanted - l.header;
    if shift <= 0.0 {
        return l;
    }
    for c in &mut l.children {
        c.rect.y += shift;
    }
    for f in &mut l.frames {
        f.rect.y += shift;
    }
    for e in &mut l.edges {
        for p in &mut e.points {
            p[1] += shift;
        }
    }
    l.header = wanted;
    l.size[1] += shift;
    l
}

fn add_wiring(g: &ArchivedGraph, node: NodeId, mut l: ContainerLayout, opts: &LayoutOptions) -> ContainerLayout {
    let extra: Vec<NodeId> = display_children(g, node)
        .into_iter()
        .filter(|&c| g.flags(c).intersects(NodeFlags::WIRING | NodeFlags::NOISE | NodeFlags::DETAIL) && !opts.hides(g, c))
        .collect();
    let mut top = if l.children.is_empty() { l.header + PAD } else { l.size[1] - PAD + GAP_Y };
    let layer = l.children.iter().map(|c| c.layer + 1).max().unwrap_or(0);
    let sizes: Vec<[f32; 2]> = extra.iter().map(|&c| box_size(g, c, opts)).collect();
    let area: f32 = l.children.iter().map(|c| (c.rect.w + GAP_X) * (c.rect.h + GAP_Y)).chain(sizes.iter().map(|s| (s[0] + GAP_X) * (s[1] + GAP_Y))).sum();
    let widest = sizes.iter().map(|s| s[0]).fold(0.0, f32::max);
    let max_row = (l.size[0] - 2.0 * PAD).max(area.sqrt() * 1.6).max(widest);
    let (mut x, mut row_h, mut right) = (PAD, 0.0f32, 0.0f32);
    for (&c, &[w, h]) in extra.iter().zip(&sizes) {
        if x > PAD && x + w - PAD > max_row {
            top += row_h + GAP_Y;
            (x, row_h) = (PAD, 0.0);
        }
        l.children.push(ChildBox { node: c, rect: Rect::new(x, top, w, h), layer, pinned: false });
        right = right.max(x + w);
        x += w + GAP_X;
        row_h = row_h.max(h);
    }
    if row_h > 0.0 {
        l.size = [l.size[0].max(right + PAD), top + row_h + PAD];
    }
    let rect_of: HashMap<NodeId, Rect> = l.children.iter().map(|c| (c.node, c.rect)).collect();
    for agg in g.aggregates(node) {
        let wiring = agg.wiring.to_native() as u32;
        let (a, b) = (agg.a.to_native(), agg.b.to_native());
        let (Some(ra), Some(rb)) = (rect_of.get(&a), rect_of.get(&b)) else { continue };
        if wiring == 0 {
            continue;
        }
        if let Some(e) = l.edges.iter_mut().find(|e| e.a == a && e.b == b) {
            e.wiring = wiring;
            continue;
        }
        let (from, to) =
            if ra.y <= rb.y { ([ra.center()[0], ra.bottom()], [rb.center()[0], rb.y]) } else { ([ra.center()[0], ra.y], [rb.center()[0], rb.bottom()]) };
        l.edges.push(LaidEdge {
            a,
            b,
            calls: 0,
            deps: 0,
            wiring,
            implements: 0,
            links: 0,
            dir: Direction::Down,
            undeclared: false,
            guessed: false,
            test_only: false,
            points: vec![from, to],
        });
    }
    l
}

struct Layering {
    kids: Vec<NodeId>,
    pinned: Vec<bool>,
    edges: Vec<SibEdge>,
    raw_layer: Vec<u32>,
}

fn layering(g: &ArchivedGraph, node: NodeId, opts: &LayoutOptions) -> Option<Layering> {
    let card = g.kind(node) == NodeKind::Type;
    let kids: Vec<NodeId> = display_children(g, node).into_iter().filter(|&c| !opts.hides(g, c) && !(card && g.kind(c) == NodeKind::Member)).collect();
    let n = kids.len();
    if n == 0 {
        return None;
    }
    let owners = entry_owners(g);
    let pinned: Vec<bool> = kids.iter().map(|k| owners.contains(k)).collect();
    let mut index: HashMap<NodeId, usize> = kids.iter().enumerate().map(|(i, &k)| (k, i)).collect();
    for c in g.children(node) {
        if let Some(i) = folded_into(g, c).and_then(|i| index.get(&i).copied()) {
            index.insert(c, i);
        }
    }
    let mut edges: Vec<SibEdge> = Vec::new();
    for agg in g.aggregates(node) {
        let (Some(&a), Some(&b)) = (index.get(&agg.a.to_native()), index.get(&agg.b.to_native())) else { continue };
        if a == b {
            continue;
        }
        let test_only = agg.scope_mask & SCOPE_MAIN == 0 && agg.scope_mask & SCOPE_TEST != 0;
        if test_only && !opts.include_test_edges {
            continue;
        }
        let (calls, deps, wiring, implements) =
            (agg.calls.to_native() as u32, agg.deps.to_native() as u32, agg.wiring.to_native() as u32, agg.implements.to_native() as u32);
        let structural = calls + deps + implements > 0;
        if !structural && !opts.show_wiring {
            continue;
        }
        let wiring = if opts.show_wiring { wiring } else { 0 };
        edges.push(SibEdge {
            a,
            b,
            calls,
            deps,
            wiring,
            implements,
            links: agg.links.to_native() as u32,
            undeclared: agg.undeclared(),
            guessed: agg.guessed(),
            test_only,
            layering: structural && !pinned[b],
            back: false,
        });
    }

    let rank: Vec<Option<usize>> = kids.iter().map(|&k| role_rank(g, k)).collect();
    for e in edges.iter_mut() {
        if let (Some(ra), Some(rb)) = (rank[e.a], rank[e.b])
            && ra > rb
        {
            e.back = true;
            e.layering = false;
        }
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

    let mut preds: Vec<Vec<usize>> = vec![Vec::new(); n];
    for e in edges.iter().filter(|e| e.layering && !e.back) {
        preds[e.b].push(e.a);
    }
    if rank.iter().any(|r| r.is_some()) {
        for _ in 0..n + ROLE_ORDER.len() {
            let mut floor = [0u32; ROLE_ORDER.len()];
            for (r, slot) in floor.iter_mut().enumerate() {
                *slot = (0..n).filter(|&v| rank[v].is_some_and(|rv| rv < r)).map(|v| layer[v] + 1).max().unwrap_or(0);
            }
            let mut changed = false;
            for &v in &queue {
                if pinned[v] {
                    continue;
                }
                let mut want = rank[v].map_or(layer[v], |r| layer[v].max(floor[r]));
                for &p in &preds[v] {
                    want = want.max(layer[p] + 1);
                }
                if want > layer[v] {
                    layer[v] = want;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }
    Some(Layering { kids, pinned, edges, raw_layer: layer })
}

fn drawn(opts: &LayoutOptions, e: &SibEdge) -> bool {
    (opts.guessed_calls || !e.guessed) && (opts.implements_arrows || e.calls + e.deps > 0) && (opts.import_arrows || e.calls + e.implements > 0 || e.wiring > 0)
}

fn direction(e: &SibEdge, raw_layer: &[u32]) -> Direction {
    if e.back || raw_layer[e.a] > raw_layer[e.b] {
        Direction::Up
    } else if raw_layer[e.a] == raw_layer[e.b] {
        Direction::Sideways
    } else {
        Direction::Down
    }
}

pub fn upward_edges(g: &ArchivedGraph, node: NodeId, opts: &LayoutOptions) -> Vec<(NodeId, NodeId, u32)> {
    let Some(l) = layering(g, node, &structure_options(opts)) else { return Vec::new() };
    l.edges.iter().filter(|e| drawn(opts, e) && direction(e, &l.raw_layer) == Direction::Up).map(|e| (l.kids[e.a], l.kids[e.b], e.calls + e.deps)).collect()
}

fn structure_options(opts: &LayoutOptions) -> LayoutOptions {
    LayoutOptions { show_wiring: false, show_noise: false, show_details: false, details_in: Vec::new(), ..opts.clone() }
}

fn layout_structure(g: &ArchivedGraph, node: NodeId, frames: &[FrameRule], opts: &LayoutOptions) -> ContainerLayout {
    let card = g.kind(node) == NodeKind::Type;
    let header = if card { card_rows_bottom(g, node, opts).max(HEADER) } else { HEADER };
    let Some(Layering { kids, pinned, edges, raw_layer }) = layering(g, node, opts) else {
        return ContainerLayout { size: [0.0, 0.0], header, ..Default::default() };
    };
    let n = kids.len();
    let raw_layers = raw_layer.iter().max().map_or(0, |m| *m as usize + 1);
    let sizes: Vec<[f32; 2]> = kids.iter().map(|&k| box_size(g, k, opts)).collect();
    let area: f32 = sizes.iter().map(|s| (s[0] + GAP_X) * (s[1] + GAP_Y)).sum();
    let widest = sizes.iter().map(|s| s[0]).fold(0.0, f32::max);
    let max_row = (area.sqrt() * 1.6).max(widest);
    let cap = ((n as f32).sqrt().ceil() as usize).max(MIN_LAYER_CAP).min(raw_layers.max(1));
    let squeeze = |layers_wanted: usize| -> Vec<u32> {
        if raw_layers > layers_wanted { raw_layer.iter().map(|&l| (l as usize * layers_wanted / raw_layers) as u32).collect() } else { raw_layer.clone() }
    };
    let aspect_with = |layers_wanted: usize| {
        let squeezed = squeeze(layers_wanted);
        let mut grouped: Vec<Vec<usize>> = vec![Vec::new(); layers_wanted.max(1)];
        for (v, &l) in squeezed.iter().enumerate() {
            grouped[l as usize].push(v);
        }
        let rows = wrap_rows(&grouped, &pinned, &sizes, max_row);
        let (w, h) = rows_extent(&rows, &sizes);
        w / h.max(1.0)
    };
    let chosen = if cap > ROLE_LAYERS { (FOLDED_ROWS..=cap).rev().find(|&l| aspect_with(l) >= MIN_ASPECT).unwrap_or(FOLDED_ROWS) } else { cap };
    let layer = squeeze(chosen);
    let layer_count = layer.iter().max().map_or(0, |m| *m as usize + 1);
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); layer_count];
    for i in 0..n {
        layers[layer[i] as usize].push(i);
    }

    let groups = group_names(g, &kids, frames);
    let mut group_ids: HashMap<&str, u32> = HashMap::new();
    let group_keys: Vec<Option<u32>> = groups
        .iter()
        .map(|gname| {
            gname.as_deref().map(|s| {
                let next = group_ids.len() as u32;
                *group_ids.entry(s).or_insert(next)
            })
        })
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

    let rows = wrap_rows(&layers, &pinned, &sizes, max_row);
    let row_widths: Vec<f32> = rows.iter().map(|(_, r)| r.iter().map(|&v| sizes[v][0]).sum::<f32>() + GAP_X * (r.len() as f32 - 1.0)).collect();
    let mut content_w = row_widths.iter().copied().fold(0.0, f32::max);
    if card {
        content_w = content_w.max(box_size(g, node, opts)[0] - 2.0 * PAD);
    }
    let mut rects = vec![Rect::default(); n];
    let mut y = header + PAD;
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
        let mut start = 0;
        while start < row.len() {
            let Some(label) = groups[row[start]].as_deref() else {
                start += 1;
                continue;
            };
            let mut end = start + 1;
            while end < row.len() && groups[row[end]].as_deref() == Some(label) {
                end += 1;
            }
            let bbox = row[start + 1..end].iter().fold(rects[row[start]], |acc, &v| acc.union(&rects[v]));
            let r = bbox.expand(FRAME_PAD);
            frame_boxes.push(FrameBox { label: label.to_string(), rect: Rect::new(r.x, r.y - FRAME_LABEL, r.w, r.h + FRAME_LABEL) });
            start = end;
        }
    }

    let row_of: HashMap<usize, usize> = rows.iter().enumerate().flat_map(|(ri, (_, r))| r.iter().map(move |&v| (v, ri))).collect();
    let laid_edges = edges
        .iter()
        .filter(|e| drawn(opts, e))
        .map(|e| {
            let (ra, rb) = (rects[e.a], rects[e.b]);
            let dir = direction(e, &raw_layer);
            let same_row = row_of[&e.a] == row_of[&e.b];
            let side_points = || {
                if ra.x < rb.x {
                    vec![[ra.right(), ra.center()[1]], [rb.x, rb.center()[1]]]
                } else {
                    vec![[ra.x, ra.center()[1]], [rb.right(), rb.center()[1]]]
                }
            };
            let points = match dir {
                Direction::Down if same_row => side_points(),
                Direction::Down => vec![[toward(&ra, rb.center()[0]), ra.bottom()], [toward(&rb, ra.center()[0]), rb.y]],
                Direction::Up => {
                    let (from, to) = ([toward(&ra, rb.center()[0]), ra.y], [toward(&rb, ra.center()[0]), rb.bottom()]);
                    let mid = [(from[0] + to[0]) / 2.0 + GAP_X, (from[1] + to[1]) / 2.0];
                    vec![from, mid, to]
                }
                Direction::Sideways => {
                    let (from, to) = if ra.x < rb.x {
                        ([ra.right(), ra.center()[1]], [rb.x, rb.center()[1]])
                    } else {
                        ([ra.x, ra.center()[1]], [rb.right(), rb.center()[1]])
                    };
                    vec![from, to]
                }
            };
            LaidEdge {
                a: kids[e.a],
                b: kids[e.b],
                calls: e.calls,
                deps: e.deps,
                wiring: e.wiring,
                implements: e.implements,
                links: e.links,
                dir,
                undeclared: e.undeclared,
                guessed: e.guessed,
                test_only: e.test_only,
                points,
            }
        })
        .collect();

    ContainerLayout {
        size: [content_w + 2.0 * PAD, height.max(header + 2.0 * PAD)],
        header,
        children: (0..n).map(|v| ChildBox { node: kids[v], rect: rects[v], layer: layer[v], pinned: pinned[v] }).collect(),
        edges: laid_edges,
        frames: frame_boxes,
        hidden_details: 0,
        shown_details: 0,
    }
}

fn wrap_rows(layers: &[Vec<usize>], pinned: &[bool], sizes: &[[f32; 2]], max_row: f32) -> Vec<(u32, Vec<usize>)> {
    let mut rows: Vec<(u32, Vec<usize>)> = Vec::new();
    for (li, l) in layers.iter().enumerate() {
        for keep_pinned in [true, false] {
            let together: f32 = l.iter().filter(|&&v| pinned[v]).take(PINNED_TOGETHER).map(|&v| sizes[v][0] + GAP_X).sum::<f32>() - GAP_X;
            let limit = if keep_pinned { max_row.max(together) } else { max_row };
            let mut row: Vec<usize> = Vec::new();
            let mut w = 0.0;
            for &v in l.iter().filter(|&&v| pinned[v] == keep_pinned) {
                let add = sizes[v][0] + if row.is_empty() { 0.0 } else { GAP_X };
                if !row.is_empty() && w + add > limit {
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
    }
    rows
}

fn rows_extent(rows: &[(u32, Vec<usize>)], sizes: &[[f32; 2]]) -> (f32, f32) {
    let w = rows.iter().map(|(_, r)| r.iter().map(|&v| sizes[v][0]).sum::<f32>() + GAP_X * (r.len() as f32 - 1.0)).fold(0.0, f32::max);
    let h = rows.iter().map(|(_, r)| r.iter().map(|&v| sizes[v][1]).fold(0.0, f32::max) + GAP_Y).sum::<f32>() - GAP_Y;
    (w + 2.0 * PAD, h + 2.0 * PAD)
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
                    if let Some((prefix, _)) = name.split_once(sep.as_str())
                        && counts[prefix] >= 2
                        && counts[prefix] < names.len()
                    {
                        out[i] = Some(prefix.to_string());
                    }
                }
            }
        }
    }
    out
}
