use srcrelief::source::*;
use srcrelief_format::Lang;

#[test]
fn spans_are_read_from_disk_dedented_and_cached() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("svc")).unwrap();
    std::fs::write(dir.path().join("svc/a.go"), "package a\n\nfunc (s *S) Run() {\n\tif ok {\n\t\treturn\n\t}\n}\n").unwrap();
    let mut cache = SourceCache::new(dir.path());
    let code = cache.span("svc/a.go", 4, 6).unwrap();
    assert_eq!(code.first_line, 4);
    assert_eq!(code.lines, ["if ok {", "    return", "}"], "tabs expand and the shared indent goes");
    std::fs::remove_file(dir.path().join("svc/a.go")).unwrap();
    assert!(cache.span("svc/a.go", 4, 6).is_some(), "served from the cache");
    assert!(cache.span("missing.go", 1, 2).is_none());
    assert!(cache.span("svc/a.go", 0, 0).is_none(), "no span, no code");
}

#[test]
fn code_is_drawn_only_once_readable() {
    assert_eq!(code_font(100.0, 40), None, "40 lines in 100 px is noise");
    let f = code_font(600.0, 40).unwrap();
    assert!((6.0..=13.0).contains(&f), "{f}");
    assert_eq!(code_font(2000.0, 3), Some(13.0), "never larger than normal text");
}

#[test]
fn highlighting_marks_keywords_strings_and_comments() {
    let lines = ["func Run(name string) error { // starts", "s := \"if not a keyword\" /* open", "still comment */ return nil"];
    let h = highlight(&lines, Lang::Go);
    let kinds = |i: usize| h[i].iter().map(|(t, k)| (t.as_str(), *k)).filter(|(t, _)| !t.trim().is_empty()).collect::<Vec<_>>();
    let first = kinds(0);
    assert_eq!(first[0], ("func", Token::Keyword));
    assert!(first.contains(&("string", Token::Keyword)));
    assert_eq!(*first.last().unwrap(), ("// starts", Token::Comment));
    assert!(kinds(1).contains(&("\"if not a keyword\"", Token::Text)) == false);
    assert!(kinds(1).contains(&("\"if not a keyword\"", Token::String)));
    assert_eq!(kinds(2)[0], ("still comment */", Token::Comment), "block comments carry over lines");
    assert!(kinds(2).contains(&("return", Token::Keyword)));
    let java = highlight(&["public final class A extends B {"], Lang::Java);
    assert!(java[0].iter().any(|(t, k)| t == "extends" && *k == Token::Keyword));
}

#[test]
fn spare_room_shows_the_lines_around_the_span() {
    assert_eq!(context_window(100, 40, 49, 10), (40, 49), "just the span");
    assert_eq!(context_window(100, 40, 49, 20), (35, 54), "five more on each side");
    assert_eq!(context_window(100, 3, 12, 20), (1, 20), "the top of the file takes the missing lines below");
    assert_eq!(context_window(100, 95, 99, 20), (81, 100));
    assert_eq!(context_window(30, 10, 20, 500), (1, 30), "the whole file once it fits");
    assert_eq!(context_window(100, 40, 49, 4), (40, 49), "never cuts the span itself");
}

#[test]
fn a_window_keeps_the_span_indent_and_marks_its_lines() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.go"), "package a\n\ntype S struct{}\n\nfunc (s *S) Run() {\n\treturn\n}\n\nfunc other() {}\n").unwrap();
    let mut cache = SourceCache::new(dir.path());
    let w = cache.window("a.go", 5, 7, 20).unwrap();
    assert_eq!(w.code.first_line, 1);
    assert_eq!(w.own, 4..7, "lines 5-7 of the file");
    assert_eq!(w.code.lines[4], "func (s *S) Run() {");
    assert_eq!(w.code.lines[5], "    return");
    assert_eq!(w.code.lines.len(), 9);
}
