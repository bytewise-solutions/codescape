#[derive(Clone, Debug, Default)]
pub struct Ordering {
    pub first_keys: Vec<Option<u32>>,
    pub groups: Vec<Option<u32>>,
    pub max_sweeps: usize,
}

impl Ordering {
    fn first_key(&self, v: usize) -> Option<u32> {
        self.first_keys.get(v).copied().flatten()
    }

    fn group(&self, v: usize) -> Option<u32> {
        self.groups.get(v).copied().flatten()
    }

    fn apply(&self, layers: &mut [Vec<usize>]) {
        for l in layers.iter_mut() {
            let (mut first, rest): (Vec<usize>, Vec<usize>) = l.iter().partition(|&&v| self.first_key(v).is_some());
            first.sort_by_key(|&v| self.first_key(v));
            let mut grouped: Vec<usize> = Vec::with_capacity(rest.len());
            let mut placed = vec![false; rest.len()];
            for i in 0..rest.len() {
                if placed[i] {
                    continue;
                }
                placed[i] = true;
                grouped.push(rest[i]);
                if let Some(gid) = self.group(rest[i]) {
                    for j in i + 1..rest.len() {
                        if !placed[j] && self.group(rest[j]) == Some(gid) {
                            placed[j] = true;
                            grouped.push(rest[j]);
                        }
                    }
                }
            }
            first.extend(grouped);
            *l = first;
        }
    }

    fn may_swap(&self, u: usize, v: usize) -> bool {
        self.first_key(u).is_none() && self.first_key(v).is_none() && self.group(u) == self.group(v)
    }
}

fn positions(layers: &[Vec<usize>], n: usize) -> Vec<usize> {
    let mut pos = vec![0usize; n];
    for l in layers {
        for (p, &v) in l.iter().enumerate() {
            pos[v] = p;
        }
    }
    pos
}

fn node_count(layers: &[Vec<usize>]) -> usize {
    layers.iter().flatten().copied().max().map_or(0, |m| m + 1)
}

fn adjacent_pairs<'e>(layer_of: &'e [u32], edges: &'e [(usize, usize)]) -> impl Iterator<Item = (usize, usize)> + 'e {
    edges.iter().filter_map(move |&(a, b)| {
        let (la, lb) = (layer_of[a], layer_of[b]);
        if la + 1 == lb {
            Some((a, b))
        } else if lb + 1 == la {
            Some((b, a))
        } else {
            None
        }
    })
}

pub fn crossings(layers: &[Vec<usize>], layer_of: &[u32], edges: &[(usize, usize)]) -> usize {
    let n = node_count(layers).max(layer_of.len());
    let pos = positions(layers, n);
    let mut per_layer: Vec<Vec<(usize, usize)>> = vec![Vec::new(); layers.len()];
    for (u, v) in adjacent_pairs(layer_of, edges) {
        per_layer[layer_of[u] as usize].push((pos[u], pos[v]));
    }
    let mut total = 0;
    for (l, mut pairs) in per_layer.into_iter().enumerate() {
        if pairs.len() < 2 || l + 1 >= layers.len() {
            continue;
        }
        pairs.sort();
        let width = layers[l + 1].len() + 1;
        let mut tree = vec![0usize; width + 1];
        for (seen, &(_, pv)) in pairs.iter().enumerate() {
            let mut i = pv + 1;
            let mut not_greater = 0;
            while i > 0 {
                not_greater += tree[i];
                i -= i.isolate_lowest_one();
            }
            total += seen - not_greater;
            let mut i = pv + 1;
            while i <= width {
                tree[i] += 1;
                i += i.isolate_lowest_one();
            }
        }
    }
    total
}

fn neighbor_lists(n: usize, layer_of: &[u32], edges: &[(usize, usize)]) -> (Vec<Vec<usize>>, Vec<Vec<usize>>) {
    let mut up = vec![Vec::new(); n];
    let mut down = vec![Vec::new(); n];
    for (u, v) in adjacent_pairs(layer_of, edges) {
        down[u].push(v);
        up[v].push(u);
    }
    (up, down)
}

pub fn barycenter_poc(layers: &[Vec<usize>], layer_of: &[u32], edges: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let n = node_count(layers).max(layer_of.len());
    let mut layers = layers.to_vec();
    let mut pos: Vec<f32> = positions(&layers, n).into_iter().map(|p| p as f32).collect();
    let mut neighbors: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(a, b) in edges {
        neighbors[a].push(b);
        neighbors[b].push(a);
    }
    let count = layers.len();
    for sweep in 0..4 {
        let order: Vec<usize> = if sweep % 2 == 0 { (1..count).collect() } else { (0..count.saturating_sub(1)).rev().collect() };
        for li in order {
            let reference = if sweep % 2 == 0 { li - 1 } else { li + 1 };
            let mut keyed: Vec<(f32, usize)> = layers[li]
                .iter()
                .map(|&v| {
                    let adj: Vec<f32> = neighbors[v].iter().filter(|&&w| layer_of[w] as usize == reference).map(|&w| pos[w]).collect();
                    (if adj.is_empty() { pos[v] } else { adj.iter().sum::<f32>() / adj.len() as f32 }, v)
                })
                .collect();
            keyed.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
            layers[li] = keyed.into_iter().map(|(_, v)| v).collect();
            for (p, &v) in layers[li].iter().enumerate() {
                pos[v] = p as f32;
            }
        }
    }
    layers
}

fn initial_order(layers: &[Vec<usize>], n: usize, down: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let mut seen = vec![usize::MAX; n];
    let mut counter = 0;
    let mut stack: Vec<usize> = Vec::new();
    for l in layers {
        for &start in l {
            if seen[start] != usize::MAX {
                continue;
            }
            stack.push(start);
            while let Some(v) = stack.pop() {
                if seen[v] != usize::MAX {
                    continue;
                }
                seen[v] = counter;
                counter += 1;
                let mut next: Vec<usize> = down[v].iter().copied().filter(|&w| seen[w] == usize::MAX).collect();
                next.sort_unstable_by(|a, b| b.cmp(a));
                stack.extend(next);
            }
        }
    }
    layers
        .iter()
        .map(|l| {
            let mut l = l.clone();
            l.sort_by_key(|&v| seen[v]);
            l
        })
        .collect()
}

fn median(values: &mut [usize]) -> Option<f32> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    let m = values.len() / 2;
    Some(if values.len() % 2 == 1 { values[m] as f32 } else { (values[m - 1] + values[m]) as f32 / 2.0 })
}

fn pair_crossings(u: usize, v: usize, refs: &[Vec<usize>], pos: &[usize]) -> usize {
    let mut c = 0;
    for &a in &refs[u] {
        for &b in &refs[v] {
            if pos[a] > pos[b] {
                c += 1;
            }
        }
    }
    c
}

pub fn minimize(layers: &[Vec<usize>], layer_of: &[u32], edges: &[(usize, usize)], ord: &Ordering) -> Vec<Vec<usize>> {
    let n = node_count(layers).max(layer_of.len());
    let (up, down) = neighbor_lists(n, layer_of, edges);
    let starts = [initial_order(layers, n, &down), layers.to_vec(), barycenter_poc(layers, layer_of, edges)];
    let mut best: Option<(usize, Vec<Vec<usize>>)> = None;
    for start in starts {
        let candidate = refine(start, n, layer_of, edges, &up, &down, ord);
        let c = crossings(&candidate, layer_of, edges);
        if best.as_ref().is_none_or(|(bc, _)| c < *bc) {
            best = Some((c, candidate));
        }
    }
    best.map(|(_, l)| l).unwrap_or_default()
}

fn refine(
    mut current: Vec<Vec<usize>>,
    n: usize,
    layer_of: &[u32],
    edges: &[(usize, usize)],
    up: &[Vec<usize>],
    down: &[Vec<usize>],
    ord: &Ordering,
) -> Vec<Vec<usize>> {
    ord.apply(&mut current);
    let mut best = current.clone();
    let mut best_c = crossings(&best, layer_of, edges);
    let max_sweeps = if ord.max_sweeps == 0 { if n > 300 { 8 } else { 24 } } else { ord.max_sweeps };
    let mut stale = 0;
    for sweep in 0..max_sweeps {
        if best_c == 0 {
            break;
        }
        let downward = sweep % 2 == 0;
        let order: Vec<usize> = if downward { (1..current.len()).collect() } else { (0..current.len().saturating_sub(1)).rev().collect() };
        let mut pos = positions(&current, n);
        for li in order {
            let refs = if downward { up } else { down };
            let mut keyed: Vec<(f32, f32, usize, usize)> = current[li]
                .iter()
                .enumerate()
                .map(|(i, &v)| {
                    let mut ps: Vec<usize> = refs[v].iter().map(|&w| pos[w]).collect();
                    let mean = if ps.is_empty() { i as f32 } else { ps.iter().sum::<usize>() as f32 / ps.len() as f32 };
                    (median(&mut ps).unwrap_or(i as f32), mean, i, v)
                })
                .collect();
            keyed.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.total_cmp(&y.1)).then(x.2.cmp(&y.2)));
            current[li] = keyed.into_iter().map(|(_, _, _, v)| v).collect();
            for (p, &v) in current[li].iter().enumerate() {
                pos[v] = p;
            }
        }
        ord.apply(&mut current);
        let mut pos = positions(&current, n);
        for _ in 0..4 {
            let mut changed = false;
            for li in 0..current.len() {
                let mut i = 0;
                while i + 1 < current[li].len() {
                    let (u, v) = (current[li][i], current[li][i + 1]);
                    if ord.may_swap(u, v) {
                        let keep = pair_crossings(u, v, up, &pos) + pair_crossings(u, v, down, &pos);
                        let swap = pair_crossings(v, u, up, &pos) + pair_crossings(v, u, down, &pos);
                        if swap < keep {
                            current[li].swap(i, i + 1);
                            pos.swap(u, v);
                            changed = true;
                        }
                    }
                    i += 1;
                }
            }
            if !changed {
                break;
            }
        }
        let c = crossings(&current, layer_of, edges);
        if c < best_c {
            best_c = c;
            best = current.clone();
            stale = 0;
        } else {
            stale += 1;
            if stale >= 6 {
                break;
            }
        }
    }
    best
}
