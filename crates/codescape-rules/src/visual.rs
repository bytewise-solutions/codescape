use codescape_format::Category;

#[derive(Clone, Debug, PartialEq)]
pub struct TagStyle {
    pub tag: String,
    pub color: [u8; 3],
    pub icon: Option<String>,
    pub badge: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FrameRule {
    AutoPrefix { sep: String },
    Pattern { glob: String, label: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct EntryGroup {
    pub label: String,
    pub kinds: Vec<String>,
    pub categories: Vec<Category>,
    pub collapsed: bool,
}

impl EntryGroup {
    pub fn matches(&self, kind: &str, category: Category) -> bool {
        (self.kinds.is_empty() || self.kinds.iter().any(|k| k == kind)) && (self.categories.is_empty() || self.categories.contains(&category))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VisualRules {
    pub tag_styles: Vec<TagStyle>,
    pub frames: Vec<FrameRule>,
    pub hide_tags: Vec<String>,
    pub entry_groups: Vec<EntryGroup>,
}

impl VisualRules {
    pub fn style_for(&self, tag: &str) -> Option<&TagStyle> {
        self.tag_styles.iter().find(|s| s.tag == tag)
    }

    pub fn group_for(&self, kind: &str, category: Category) -> Option<usize> {
        self.entry_groups.iter().position(|g| g.matches(kind, category))
    }
}
