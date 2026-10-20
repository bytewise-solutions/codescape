use codescape_format::{ArchivedGraph, Category};
use codescape_rules::VisualRules;

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
        if let Some(p) = g.entry_parent(i)
            && p < n
        {
            children[p].push(i as u32);
        }
    }
    for i in (0..n).filter(|&i| g.entry_parent(i).is_none() && g.entry_category(i) != Category::Test) {
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

pub const PANEL_TEXT: std::ops::RangeInclusive<f32> = 0.8..=2.0;

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub panel_text: f32,
}

impl Default for Prefs {
    fn default() -> Prefs {
        Prefs { panel_text: 1.0 }
    }
}

impl Prefs {
    pub fn parse(text: &str) -> Prefs {
        let p: Prefs = toml::from_str(text).unwrap_or_default();
        Prefs { panel_text: p.panel_text.clamp(*PANEL_TEXT.start(), *PANEL_TEXT.end()) }
    }

    pub fn to_toml(&self) -> String {
        toml::to_string(self).unwrap_or_default()
    }

    fn path() -> Option<std::path::PathBuf> {
        dirs::config_dir().map(|d| d.join("codescape").join("prefs.toml"))
    }

    pub fn load() -> Prefs {
        Prefs::path().and_then(|p| std::fs::read_to_string(p).ok()).map_or_else(Prefs::default, |t| Prefs::parse(&t))
    }

    pub fn save(&self) {
        let Some(path) = Prefs::path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, self.to_toml());
    }
}
