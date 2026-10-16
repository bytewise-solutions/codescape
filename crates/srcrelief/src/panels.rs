use srcrelief_format::ArchivedGraph;
use srcrelief_rules::VisualRules;

#[derive(Clone, Debug, PartialEq)]
pub struct EntryItem {
    pub entry: u32,
    pub children: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EntryGroupView {
    pub label: String,
    pub collapsed: bool,
    pub items: Vec<EntryItem>,
}

pub fn group_entries(g: &ArchivedGraph, vis: &VisualRules) -> Vec<EntryGroupView> {
    let mut groups: Vec<EntryGroupView> =
        vis.entry_groups.iter().map(|grp| EntryGroupView { label: grp.label.clone(), collapsed: grp.collapsed, items: Vec::new() }).collect();
    groups.push(EntryGroupView { label: "Other".into(), collapsed: true, items: Vec::new() });
    let n = g.entries().len();
    let mut children: Vec<Vec<u32>> = vec![Vec::new(); n];
    for i in 0..n {
        if let Some(p) = g.entry_parent(i) {
            if p < n {
                children[p].push(i as u32);
            }
        }
    }
    for i in (0..n).filter(|&i| g.entry_parent(i).is_none()) {
        let mut kids = std::mem::take(&mut children[i]);
        kids.sort_by(|&a, &b| {
            let (a, b) = (a as usize, b as usize);
            g.entries()[a].order.to_native().cmp(&g.entries()[b].order.to_native()).then_with(|| g.entry_label(a).cmp(g.entry_label(b)))
        });
        let slot = vis.group_for(g.entry_kind(i), g.entry_category(i)).unwrap_or(groups.len() - 1);
        groups[slot].items.push(EntryItem { entry: i as u32, children: kids });
    }
    for grp in &mut groups {
        grp.items.sort_by(|a, b| g.entry_label(a.entry as usize).cmp(g.entry_label(b.entry as usize)));
    }
    groups.retain(|grp| !grp.items.is_empty());
    groups
}
