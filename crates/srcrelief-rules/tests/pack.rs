use srcrelief_rules::*;

const A: &str = r#"
name = "a"
version = "1"
[[rule]]
id = "shared.tools"
path = { glob = "**/tools/**", category = "dev-tool" }
[[rule]]
id = "a.only"
name_tag = { kinds = ["type"], pattern = "Repo$", tag = "layer:repo" }
"#;

const B: &str = r#"
name = "b"
version = "1"
[[rule]]
id = "shared.tools"
path = { glob = "**/scripts/**", category = "dev-tool" }
"#;

fn packs(srcs: &[&str], sel: &PackSelection) -> Packs {
    let list = srcs.iter().enumerate().map(|(i, s)| Pack::parse(s, &format!("p{i}.toml"), None).unwrap()).collect();
    Packs::from_packs(list, sel)
}

#[test]
fn later_pack_overrides_rule_by_id() {
    let p = packs(&[A, B], &PackSelection::default());
    let rules: Vec<(&str, &Rule)> = p.rules().collect();
    assert_eq!(rules.len(), 2);
    let tools = rules.iter().find(|(id, _)| *id == "shared.tools").unwrap().1;
    assert!(matches!(tools, Rule::Path { glob, .. } if glob == "**/scripts/**"));
}

#[test]
fn disable_removes_pack() {
    let sel = PackSelection { disable: vec!["b".into()], ..Default::default() };
    let p = packs(&[A, B], &sel);
    assert_eq!(p.names(), ["a"]);
    let tools = p.rules().find(|(id, _)| *id == "shared.tools").unwrap().1;
    assert!(matches!(tools, Rule::Path { glob, .. } if glob == "**/tools/**"));
}

#[test]
fn project_dir_pack_loaded() {
    let dir = tempfile::tempdir().unwrap();
    let pack_dir = dir.path().join(".srcrelief/rules/mine");
    std::fs::create_dir_all(&pack_dir).unwrap();
    std::fs::write(pack_dir.join("pack.toml"), "name = \"mine\"\nversion = \"0.1\"\n[[rule]]\nid = \"mine.t\"\nname_tag = { kinds = [\"type\"], pattern = \"X\", tag = \"x\" }\n").unwrap();
    let p = load_packs(Some(dir.path()), None, &PackSelection::default()).unwrap();
    assert!(p.names().contains(&"mine"));
    assert!(p.names().contains(&"libgdx"), "built-ins load too: {:?}", p.names());
    assert_eq!(p.names().last(), Some(&"mine"));
}

#[test]
fn unknown_matcher_is_an_error_naming_the_file() {
    let src = "name = \"x\"\nversion = \"1\"\n[[rule]]\nid = \"x.y\"\nbogus = { a = 1 }\n";
    let err = Pack::parse(src, "/tmp/x/pack.toml", None).unwrap_err().to_string();
    assert!(err.contains("/tmp/x/pack.toml"), "{err}");
    let two = "name = \"x\"\nversion = \"1\"\n[[rule]]\nid = \"x.y\"\nmain = {}\nname_tag = { kinds = [], pattern = \"a\", tag = \"t\" }\n";
    assert!(Pack::parse(two, "two.toml", None).unwrap_err().to_string().contains("exactly one matcher"));
}

#[test]
fn detection_hash_ignores_visual_changes() {
    let v1 = format!("{A}\n[visual]\nhide_tags = [\"a\"]\n");
    let v2 = format!("{A}\n[visual]\nhide_tags = [\"b\"]\n");
    let sel = PackSelection::default();
    assert_eq!(detection_hash(&packs(&[&v1], &sel)), detection_hash(&packs(&[&v2], &sel)));
    assert_ne!(detection_hash(&packs(&[A], &sel)), detection_hash(&packs(&[B], &sel)));
}

#[test]
fn visual_rules_merge() {
    let a = "name = \"a\"\nversion = \"1\"\n[visual]\nhide_tags = [\"noise:logging\"]\n[[visual.tag_style]]\ntag = \"undeclared\"\ncolor = [1, 2, 3]\n[[visual.frame]]\nauto_prefix = \"-\"\n[[visual.entry_group]]\nlabel = \"Apps\"\nkinds = [\"main\"]\n";
    let b = "name = \"b\"\nversion = \"1\"\n[visual]\n[[visual.tag_style]]\ntag = \"undeclared\"\ncolor = [9, 9, 9]\n[[visual.frame]]\npattern = \"svc-*\"\nlabel = \"services\"\n";
    let v = packs(&[a, b], &PackSelection::default()).visual();
    assert_eq!(v.hide_tags, ["noise:logging"]);
    assert_eq!(v.tag_styles.len(), 1);
    assert_eq!(v.tag_styles[0].color, [9, 9, 9]);
    assert_eq!(v.frames, [FrameRule::AutoPrefix { sep: "-".into() }, FrameRule::Pattern { glob: "svc-*".into(), label: "services".into() }]);
    assert_eq!(v.entry_groups[0].label, "Apps");
}

#[test]
fn builtin_packs_parse() {
    let p = load_packs(None, None, &PackSelection::default()).unwrap();
    assert_eq!(p.names(), ["java-core", "libgdx", "spring-boot", "dagger", "go-core", "fiber", "rabbitmq-amqp091", "google-wire", "ts-core", "angular", "express", "rust-core", "layers", "significance", "default-visuals"]);
}

#[test]
fn test_code_takes_path_globs() {
    let src = "name = \"t\"\nversion = \"1\"\n[[rule]]\nid = \"t.it\"\ntest_code = { paths = [\"**/it/**\"] }\n";
    let pack = Pack::parse(src, "t", None).unwrap();
    assert_eq!(pack.rules[0].1, Rule::TestCode { file_imports: vec![], dir_names: vec![], paths: vec!["**/it/**".into()] });
    let bad = src.replace("**/it/**", "a/{b");
    assert!(Pack::parse(&bad, "t", None).unwrap_err().to_string().contains("t.it"));
}
