use srcrelief_facts::{CallFacts, ProjectFacts};
use srcrelief_format::NodeId;
use std::collections::HashMap;

pub type Prefix = Option<String>;

pub fn join(prefix: &Prefix, seg: &str) -> String {
    let base = prefix.as_deref().unwrap_or("…");
    if seg.is_empty() {
        return if base.is_empty() { "/".into() } else { base.to_string() };
    }
    match (base.ends_with('/'), seg.starts_with('/')) {
        (true, true) => format!("{base}{}", &seg[1..]),
        (false, false) if !base.is_empty() && base != "…" => format!("{base}/{seg}"),
        _ => format!("{base}{seg}"),
    }
}

pub struct PrefixFlow<'a> {
    params: HashMap<NodeId, Vec<&'a str>>,
    vars: HashMap<(NodeId, String), Prefix>,
    param_prefix: HashMap<(NodeId, usize), Prefix>,
}

impl<'a> PrefixFlow<'a> {
    pub fn compute(facts: &'a ProjectFacts, is_group: &dyn Fn(usize, &CallFacts) -> bool) -> PrefixFlow<'a> {
        let mut flow = PrefixFlow { params: HashMap::new(), vars: HashMap::new(), param_prefix: HashMap::new() };
        for f in &facts.files {
            for func in &f.functions {
                flow.params.entry(func.node).or_insert_with(|| func.params.iter().map(|(n, _)| n.as_str()).collect());
            }
        }
        let mut calls: Vec<(usize, &CallFacts)> = facts.files.iter().enumerate().flat_map(|(fi, f)| f.calls.iter().map(move |c| (fi, c))).collect();
        calls.sort_by_key(|(_, c)| (c.caller, c.line));
        for _ in 0..8 {
            let before = (flow.vars.clone(), flow.param_prefix.clone());
            for &(fi, c) in &calls {
                if !is_group(fi, c) {
                    continue;
                }
                let Some(var) = &c.assigned_to else { continue };
                let base = flow.prefix_of(c.caller, c.receiver_var.as_deref().unwrap_or(""));
                let value = match c.string_args.first() {
                    Some(Some(seg)) => base.as_ref().map(|_| join(&base, seg)),
                    _ => None,
                };
                flow.vars.insert((c.caller, var.clone()), value);
            }
            let mut sites: HashMap<(NodeId, usize), Vec<Prefix>> = HashMap::new();
            for &(_, c) in &calls {
                let Some(target) = c.target else { continue };
                let Some(params) = flow.params.get(&target) else { continue };
                for (i, arg) in c.ident_args.iter().enumerate().take(params.len()) {
                    let p = match arg {
                        Some(var) => flow.prefix_of(c.caller, var),
                        None => None,
                    };
                    sites.entry((target, i)).or_default().push(p);
                }
            }
            flow.param_prefix = sites
                .into_iter()
                .map(|(k, v)| {
                    let first = v[0].clone();
                    let agreed = v.iter().all(|x| *x == first);
                    (k, if agreed { first } else { None })
                })
                .collect();
            if (flow.vars.clone(), flow.param_prefix.clone()) == before {
                break;
            }
        }
        flow
    }

    pub fn prefix_of(&self, func: NodeId, var: &str) -> Prefix {
        if let Some(p) = self.vars.get(&(func, var.to_string())) {
            return p.clone();
        }
        if let Some(i) = self.params.get(&func).and_then(|ps| ps.iter().position(|p| *p == var)) {
            return self.param_prefix.get(&(func, i)).cloned().flatten();
        }
        Some(String::new())
    }
}
