#[derive(Debug, Default, PartialEq)]
pub struct GoMod {
    pub module: Option<String>,
    pub requires: Vec<String>,
    pub replaces: Vec<(String, String)>,
}

fn strip_comment(line: &str) -> &str {
    line.split("//").next().unwrap_or("").trim()
}

pub fn parse_go_mod(src: &str) -> GoMod {
    let mut out = GoMod::default();
    let mut block: Option<&str> = None;
    for raw in src.lines() {
        let line = strip_comment(raw);
        if line.is_empty() {
            continue;
        }
        if let Some(kind) = block {
            if line == ")" {
                block = None;
                continue;
            }
            directive(&mut out, kind, line);
            continue;
        }
        let (kw, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let rest = rest.trim();
        if rest == "(" {
            block = Some(match kw {
                "require" => "require",
                "replace" => "replace",
                _ => "other",
            });
            continue;
        }
        match kw {
            "module" => out.module = Some(rest.trim_matches('"').to_string()),
            "require" | "replace" => directive(&mut out, kw, rest),
            _ => {}
        }
    }
    out
}

fn directive(out: &mut GoMod, kind: &str, line: &str) {
    match kind {
        "require" => {
            if let Some(path) = line.split_whitespace().next() {
                out.requires.push(path.to_string());
            }
        }
        "replace" => {
            if let Some((from, to)) = line.split_once("=>") {
                let from = from.split_whitespace().next().unwrap_or("").to_string();
                let to = to.split_whitespace().next().unwrap_or("").to_string();
                out.replaces.push((from, to));
            }
        }
        _ => {}
    }
}

pub fn parse_go_work(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_use = false;
    for raw in src.lines() {
        let line = strip_comment(raw);
        if in_use {
            if line == ")" {
                in_use = false;
            } else if !line.is_empty() {
                out.push(line.trim_matches('"').to_string());
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("use") {
            let rest = rest.trim();
            if rest == "(" {
                in_use = true;
            } else if !rest.is_empty() {
                out.push(rest.trim_matches('"').to_string());
            }
        }
    }
    out
}
