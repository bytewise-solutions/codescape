use codescape_format::NodeId;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloneGroup {
    pub tokens: u32,
    pub places: Vec<(NodeId, u32, u32)>,
}

const BASE: u64 = 0x0000_0100_0000_01b3;

fn mix(t: u32) -> u64 {
    (u64::from(t) + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15)
}

pub fn find_clones(bodies: &[(NodeId, &[u32], &[u32])], min_tokens: usize) -> Vec<CloneGroup> {
    let w = min_tokens.max(1);
    let top = BASE.wrapping_pow(w as u32 - 1);
    let mut windows: Vec<(u64, u32, u32)> = Vec::new();
    for (b, (_, toks, _)) in bodies.iter().enumerate() {
        if toks.len() < w {
            continue;
        }
        let mut h = toks[..w].iter().fold(0u64, |h, &t| h.wrapping_mul(BASE).wrapping_add(mix(t)));
        windows.push((h, b as u32, 0));
        for i in w..toks.len() {
            h = h.wrapping_sub(mix(toks[i - w]).wrapping_mul(top)).wrapping_mul(BASE).wrapping_add(mix(toks[i]));
            windows.push((h, b as u32, (i - w + 1) as u32));
        }
    }
    windows.sort_unstable();

    let tok = |b: u32, o: usize| bodies[b as usize].1.get(o).copied();
    let mut raw: Vec<(usize, Vec<(u32, usize)>)> = Vec::new();
    let mut run = 0;
    while run < windows.len() {
        let end = run + windows[run..].iter().take_while(|x| x.0 == windows[run].0).count();
        let mut left: Vec<(u32, usize)> = windows[run..end].iter().map(|&(_, b, o)| (b, o as usize)).collect();
        run = end;
        while left.len() > 1 {
            let (b0, o0) = left[0];
            let first = &bodies[b0 as usize].1[o0..o0 + w];
            let (mut places, rest): (Vec<_>, Vec<_>) = left.into_iter().partition(|&(b, o)| &bodies[b as usize].1[o..o + w] == first);
            left = rest;
            places.sort_by_key(|&(b, o)| (bodies[b as usize].0, b, o));
            places.dedup_by_key(|p| bodies[p.0 as usize].0);
            if places.len() < 2 {
                continue;
            }
            let before = |&(b, o): &(u32, usize)| if o == 0 { None } else { tok(b, o - 1) };
            let lead = before(&places[0]);
            if lead.is_some() && places.iter().all(|p| before(p) == lead) {
                continue;
            }
            let mut len = w;
            while let Some(next) = tok(places[0].0, places[0].1 + len)
                && places.iter().all(|&(b, o)| tok(b, o + len) == Some(next))
            {
                len += 1;
            }
            raw.push((len, places));
        }
    }
    raw.sort_by(|x, y| y.0.cmp(&x.0).then_with(|| x.1.cmp(&y.1)));

    let mut taken: HashMap<u32, Vec<(usize, usize)>> = HashMap::new();
    let mut out = Vec::new();
    for (len, places) in raw {
        let overlaps = |&(b, o): &(u32, usize)| taken.get(&b).is_some_and(|spans| spans.iter().any(|&(s, e)| s < o + len && o < e));
        if places.iter().all(overlaps) {
            continue;
        }
        for &(b, o) in &places {
            taken.entry(b).or_default().push((o, o + len));
        }
        let place = |(b, o): (u32, usize)| {
            let (node, _, lines) = bodies[b as usize];
            (node, lines[o], lines[o + len - 1])
        };
        out.push(CloneGroup { tokens: len as u32, places: places.into_iter().map(place).collect() });
    }
    out
}
