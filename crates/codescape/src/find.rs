#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub line: usize,
    pub col: usize,
    pub len: usize,
}

pub fn find_in(lines: &[String], query: &str) -> Vec<Hit> {
    if query.is_empty() {
        return Vec::new();
    }
    let sensitive = query.chars().any(char::is_uppercase);
    let fold = |c: char| if sensitive { c } else { c.to_lowercase().next().unwrap_or(c) };
    let needle: Vec<char> = query.chars().map(fold).collect();
    let mut hits = Vec::new();
    for (line, text) in lines.iter().enumerate() {
        let hay: Vec<char> = text.chars().map(fold).collect();
        let mut col = 0;
        while col + needle.len() <= hay.len() {
            if hay[col..col + needle.len()] == needle[..] {
                hits.push(Hit { line, col, len: needle.len() });
                col += needle.len();
            } else {
                col += 1;
            }
        }
    }
    hits
}

pub fn step_hit(len: usize, current: Option<usize>, forward: bool) -> Option<usize> {
    match (len, current) {
        (0, _) => None,
        (_, None) => Some(0),
        (_, Some(c)) if forward => Some((c + 1) % len),
        (_, Some(c)) => Some((c + len - 1) % len),
    }
}

pub fn hit_from(hits: &[Hit], line: usize) -> Option<usize> {
    hits.iter().position(|h| h.line >= line).or((!hits.is_empty()).then_some(0))
}
