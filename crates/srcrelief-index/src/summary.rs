use srcrelief_format::*;
use srcrelief_rules::VisualRules;
use std::fmt;
use std::time::Duration;

const MAX_CHILDREN: usize = 6;

#[derive(Clone, Debug)]
pub struct ModuleLine {
    pub name: String,
    pub lines: u64,
    pub depends_on: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct EntryLine {
    pub label: String,
    pub kind: String,
    pub lifecycle: Lifecycle,
    pub handoff: Option<String>,
    pub children: Vec<String>,
    pub child_count: usize,
}

#[derive(Clone, Debug)]
pub struct Summary {
    pub project: String,
    pub modules: Vec<ModuleLine>,
    pub groups: Vec<(String, Vec<EntryLine>)>,
    pub skipped: Vec<(String, String)>,
    pub files: u32,
    pub lines: u64,
    pub resolved: u64,
    pub guessed: u64,
    pub unresolved: u64,
    pub index_bytes: u64,
    pub source_bytes: u64,
    pub elapsed: Duration,
}

impl Summary {
    pub fn from_graph(g: &ArchivedGraph, visual: &VisualRules, index_bytes: u64, elapsed: Duration) -> Summary {
        let modules = g
            .children(g.root())
            .map(|m| {
                let mut depends_on: Vec<String> = g
                    .aggregates(g.root())
                    .iter()
                    .filter(|a| a.a.to_native() == m && a.scope_mask & SCOPE_MAIN != 0)
                    .map(|a| g.name(a.b.to_native()).to_string())
                    .collect();
                depends_on.sort();
                ModuleLine { name: g.name(m).to_string(), lines: g.subtree_lines(m), depends_on }
            })
            .collect();
        let mut groups: Vec<(String, Vec<EntryLine>)> = visual.entry_groups.iter().map(|grp| (grp.label.clone(), Vec::new())).collect();
        groups.push(("Ungrouped".into(), Vec::new()));
        let n = g.entries().len();
        for i in (0..n).filter(|&i| g.entry_parent(i).is_none()) {
            let mut kids: Vec<usize> = (0..n).filter(|&k| g.entry_parent(k) == Some(i)).collect();
            kids.sort_by_key(|&k| (g.entries()[k].order.to_native(), g.entry_label(k).to_string()));
            let line = EntryLine {
                label: g.entry_label(i).to_string(),
                kind: g.entry_kind(i).to_string(),
                lifecycle: g.entry_lifecycle(i),
                handoff: g.entry_handoff(i).map(String::from),
                children: kids.iter().take(MAX_CHILDREN).map(|&k| format!("{} ({})", g.entry_label(k), g.entry_lifecycle(k).label())).collect(),
                child_count: kids.len(),
            };
            let slot = visual.group_for(g.entry_kind(i), g.entry_category(i)).unwrap_or(groups.len() - 1);
            groups[slot].1.push(line);
        }
        groups.retain(|(_, v)| !v.is_empty());
        for (_, v) in &mut groups {
            v.sort_by(|a, b| a.label.cmp(&b.label));
        }
        let s = g.stats();
        Summary {
            project: g.name(g.root()).to_string(),
            modules,
            groups,
            skipped: g.skipped_roots().map(|(k, p)| (k.to_string(), p.to_string())).collect(),
            files: s.files.to_native(),
            lines: s.lines.to_native(),
            resolved: s.calls_resolved.to_native(),
            guessed: s.calls_guessed.to_native(),
            unresolved: s.calls_unresolved.to_native(),
            index_bytes,
            source_bytes: s.source_bytes.to_native(),
            elapsed,
        }
    }
}

fn pct(part: u64, total: u64) -> f64 {
    if total == 0 { 0.0 } else { part as f64 * 100.0 / total as f64 }
}

impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{} — {} files, {} lines, indexed in {:.2?}", self.project, self.files, self.lines, self.elapsed)?;
        writeln!(f, "\nModules")?;
        for m in &self.modules {
            let deps = if m.depends_on.is_empty() { String::new() } else { format!(" → {}", m.depends_on.join(", ")) };
            writeln!(f, "  {:<28} {:>8} lines{deps}", m.name, m.lines)?;
        }
        writeln!(f, "\nEntry points")?;
        for (label, entries) in &self.groups {
            writeln!(f, "  {label} ({})", entries.len())?;
            for e in entries {
                let handoff = e.handoff.as_ref().map(|h| format!(" → hands off to {h}")).unwrap_or_default();
                writeln!(f, "    {} [{}]{handoff}", e.label, e.kind)?;
                for c in &e.children {
                    writeln!(f, "      {c}")?;
                }
                if e.child_count > e.children.len() {
                    writeln!(f, "      … {} more", e.child_count - e.children.len())?;
                }
            }
        }
        if !self.skipped.is_empty() {
            let list: Vec<String> = self.skipped.iter().map(|(k, p)| format!("{p} ({k})")).collect();
            writeln!(f, "\nNot indexed: {}", list.join(", "))?;
        }
        let total = self.resolved + self.guessed + self.unresolved;
        writeln!(
            f,
            "\nCalls: {:.0}% resolved, {:.0}% guessed, {:.0}% unresolved",
            pct(self.resolved, total),
            pct(self.guessed, total),
            pct(self.unresolved, total)
        )?;
        writeln!(f, "Index: {} KiB ({:.0}% of {} KiB of source)", self.index_bytes / 1024, pct(self.index_bytes, self.source_bytes), self.source_bytes / 1024)
    }
}
