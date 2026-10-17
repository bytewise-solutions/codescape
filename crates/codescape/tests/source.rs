use codescape::source::*;
use codescape_format::Lang;

#[test]
fn files_are_read_once_with_tabs_expanded() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("svc")).unwrap();
    std::fs::write(dir.path().join("svc/a.go"), "package a\n\nfunc Run() {\n\treturn\n}\n").unwrap();
    let mut cache = SourceCache::new(dir.path());
    let lines = cache.file("svc/a.go").unwrap();
    assert_eq!(lines.len(), 5);
    assert_eq!(lines[3], "    return");
    std::fs::remove_file(dir.path().join("svc/a.go")).unwrap();
    assert!(cache.file("svc/a.go").is_some(), "served from the cache");
    assert!(cache.file("missing.go").is_none());
}

#[test]
fn the_file_replaces_the_box_once_its_text_is_readable() {
    assert_eq!(file_font(300.0), None, "a box this narrow keeps its members");
    let f = file_font(800.0).unwrap();
    assert!((FILE_MIN_PT..14.0).contains(&f), "{f}");
    assert!(file_font(4000.0).unwrap() > f, "zooming in keeps enlarging the text");
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
    assert!(!kinds(1).contains(&("\"if not a keyword\"", Token::Text)));
    assert!(kinds(1).contains(&("\"if not a keyword\"", Token::String)));
    assert_eq!(kinds(2)[0], ("still comment */", Token::Comment), "block comments carry over lines");
    assert!(kinds(2).contains(&("return", Token::Keyword)));
    let java = highlight(&["public final class A extends B {"], Lang::Java);
    assert!(java[0].iter().any(|(t, k)| t == "extends" && *k == Token::Keyword));
}

#[test]
fn overlapping_spans_merge_into_one_band() {
    assert_eq!(merge_spans(vec![(32, 172), (50, 57), (59, 80)]), [(32, 172)]);
    assert_eq!(merge_spans(vec![(80, 90), (29, 32), (33, 40), (0, 0)]), [(29, 40), (80, 90)], "adjacent lines join, empty spans go");
}

#[test]
fn a_file_opens_on_its_type_and_scrolls_within_its_lines() {
    assert_eq!(initial_scroll(40, 300.0, 100.0, 20.0), 29.0, "line 40 sits where the box top was, 10 lines down");
    assert_eq!(initial_scroll(3, 300.0, 100.0, 20.0), 0.0, "never above the first line");
    assert_eq!(scroll_by(10.0, -60.0, 20.0, 200, 30), 13.0, "wheel down by three lines");
    assert_eq!(scroll_by(1.0, 200.0, 20.0, 200, 30), 0.0);
    assert_eq!(scroll_by(165.0, -400.0, 20.0, 200, 30), 172.0, "stops with the last line still in view, plus a margin");
    assert_eq!(scroll_by(0.0, -100.0, 20.0, 10, 30), 0.0, "a short file does not scroll");
}

#[test]
fn the_scrollbar_thumb_shows_the_visible_share_and_position() {
    let (top, len) = thumb(0.0, 200, 50, 400.0);
    assert_eq!((top, len), (0.0, 100.0), "a quarter of the file on screen is a quarter of the track");
    let (top, len) = thumb(max_scroll(200, 50), 200, 50, 400.0);
    assert!((top + len - 400.0).abs() < 0.01, "at the end the thumb touches the bottom");
    assert_eq!(thumb(0.0, 10000, 50, 400.0).1, 24.0, "never thinner than a grabbable minimum");
    assert_eq!(thumb(0.0, 20, 50, 400.0), (0.0, 400.0), "a short file fills the track");
    assert_eq!(scroll_at(0.0, 200, 50, 400.0), 0.0);
    assert_eq!(scroll_at(400.0, 200, 50, 400.0), max_scroll(200, 50));
    assert!((scroll_at(200.0, 200, 50, 400.0) - max_scroll(200, 50) / 2.0).abs() < 0.01, "the thumb centre follows the pointer");
}

#[test]
fn rust_and_typescript_keywords_highlight_and_lifetimes_are_not_strings() {
    let rs = highlight(&["pub fn get<'a>(s: &'a str) -> Option<&'a str> { let c = 'x'; None }"], Lang::Rust);
    let kinds: Vec<(&str, Token)> = rs[0].iter().map(|(t, k)| (t.as_str(), *k)).collect();
    assert!(kinds.contains(&("pub", Token::Keyword)) && kinds.contains(&("let", Token::Keyword)));
    assert!(kinds.contains(&("'x'", Token::String)));
    assert!(kinds.iter().all(|(t, k)| *k != Token::String || *t == "'x'"), "lifetimes stay text: {kinds:?}");
    let ts = highlight(&["export async function load(id: string): Promise<void> { const x = `a${id}`; }"], Lang::TypeScript);
    let kinds: Vec<(&str, Token)> = ts[0].iter().map(|(t, k)| (t.as_str(), *k)).collect();
    for k in ["export", "async", "function", "const", "string", "void"] {
        assert!(kinds.contains(&(k, Token::Keyword)), "{k} in {kinds:?}");
    }
}

#[test]
fn sideways_scrolling_stops_once_the_longest_line_is_in_view() {
    assert_eq!(hscroll_by(0.0, -50.0, 200, 5.0, 600.0), 50.0, "wheel to the right moves the text left");
    assert_eq!(hscroll_by(10.0, 100.0, 200, 5.0, 600.0), 0.0);
    assert_eq!(hscroll_by(390.0, -100.0, 200, 5.0, 600.0), 400.0 + 5.0 * 4.0, "stops with the longest line's end visible, plus a margin");
    assert_eq!(hscroll_by(0.0, -100.0, 50, 5.0, 600.0), 0.0, "short lines do not scroll");
}
