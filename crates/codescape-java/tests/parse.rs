use codescape_format::SubKind;
use codescape_java::parse::*;

fn ty<'a>(f: &'a JavaFile, name: &str) -> &'a JType {
    f.types.iter().find(|t| t.name == name).unwrap_or_else(|| panic!("no type {name}"))
}

fn method<'a>(t: &'a JType, name: &str) -> &'a JMethod {
    t.methods.iter().find(|m| m.name == name).unwrap_or_else(|| panic!("no method {name}"))
}

#[test]
fn parses_types_methods_and_main() {
    let src = r#"
package com.example.game.desktop.main;

import com.example.game.client.main.ClientApp;
import static java.util.Arrays.asList;
import com.badlogic.gdx.*;

public class DesktopLauncher {
    public static void main(String[] arg) {
        run();
    }
    private void run() {}
}
interface Shape { double area(); }
enum Color { RED, GREEN; void paint() {} }
record Point(int x, int y) {}
"#;
    let f = parse_java(src.as_bytes());
    assert_eq!(f.package, "com.example.game.desktop.main");
    assert_eq!(f.imports.len(), 3);
    assert_eq!(f.imports[0].path, "com.example.game.client.main.ClientApp");
    assert!(f.imports[1].is_static);
    assert!(f.imports[2].wildcard && f.imports[2].path == "com.badlogic.gdx");
    let c = ty(&f, "DesktopLauncher");
    assert_eq!(c.kind, SubKind::Class);
    let m = method(c, "main");
    assert!(m.is_static && m.is_public);
    assert_eq!(m.params, [("arg".to_string(), "String".to_string())]);
    assert_eq!((c.start_line, c.end_line), (8, 13));
    assert_eq!(ty(&f, "Shape").kind, SubKind::Interface);
    assert_eq!(ty(&f, "Color").kind, SubKind::Enum);
    assert!(ty(&f, "Color").methods.iter().any(|m| m.name == "paint"));
    assert_eq!(ty(&f, "Point").kind, SubKind::Record);
}

#[test]
fn captures_calls_with_receivers() {
    let src = r#"
class A extends Base implements Runnable, java.io.Closeable {
    private ViewManager viewManager;
    void go() {
        this.a.run();
        viewManager.update();
        Util.go(1);
        super.render();
        new Lwjgl3Application(new ClientApp(x), cfg);
        var q = svc.make("/x", handler);
        list.stream().map(y -> y.tick());
    }
}
"#;
    let f = parse_java(src.as_bytes());
    let a = ty(&f, "A");
    assert_eq!(a.extends, ["Base"]);
    assert_eq!(a.implements, ["Runnable", "java.io.Closeable"]);
    let calls = &method(a, "go").calls;
    let by = |name: &str| calls.iter().find(|c| c.method == name).unwrap_or_else(|| panic!("no call {name}"));
    assert_eq!(by("run").receiver, Receiver::Field("a".into()));
    assert_eq!(by("update").receiver, Receiver::Ident("viewManager".into()));
    assert_eq!(by("go").receiver, Receiver::Ident("Util".into()));
    assert_eq!(by("render").receiver, Receiver::Super);
    let news: Vec<&JCall> = calls.iter().filter(|c| c.is_new).collect();
    assert_eq!(news.len(), 2);
    assert!(news.iter().any(|c| c.method == "Lwjgl3Application"));
    assert!(news.iter().any(|c| c.method == "ClientApp"));
    let make = by("make");
    assert_eq!(make.assigned_to.as_deref(), Some("q"));
    assert_eq!(make.string_args[0].as_deref(), Some("/x"));
    assert_eq!(make.ident_args[1].as_deref(), Some("handler"));
    assert_eq!(by("map").receiver, Receiver::Chain(Box::new(Receiver::Ident("list".into())), "stream".into()));
    assert_eq!(by("tick").receiver, Receiver::Ident("y".into()));
}

#[test]
fn captures_annotations_with_args() {
    let src = r#"
@SpringBootApplication
@RequestMapping(value = "/api")
class App {
    @GetMapping("/users")
    @Override
    public List<User> users() { return null; }
    @PostMapping(path = {"/a", "/b"})
    void post() {}
}
"#;
    let f = parse_java(src.as_bytes());
    let app = ty(&f, "App");
    let names: Vec<&str> = app.annotations.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["SpringBootApplication", "RequestMapping"]);
    assert_eq!(app.annotations[1].args, ["/api"]);
    let users = method(app, "users");
    assert_eq!(users.annotations[0].name, "GetMapping");
    assert_eq!(users.annotations[0].args, ["/users"]);
    assert_eq!(users.return_type, "List");
    assert_eq!(method(app, "post").annotations[0].args, ["/a", "/b"]);
}

#[test]
fn locals_and_fields_typed() {
    let src = r#"
class B {
    private ViewManager viewManager;
    protected final Map<String, List<Foo>> cache = new HashMap<>();
    void m(Bar bar, int n) {
        MessageQueue q = make();
        var s = new Service();
        for (Item it : items) { it.use(); }
        try (Reader r = open()) { r.read(); } catch (IOException e) { e.log(); }
    }
}
"#;
    let f = parse_java(src.as_bytes());
    let b = ty(&f, "B");
    assert!(b.fields.contains(&("viewManager".into(), "ViewManager".into())));
    assert!(b.fields.contains(&("cache".into(), "Map".into())));
    let m = method(b, "m");
    assert_eq!(m.params, [("bar".to_string(), "Bar".to_string()), ("n".into(), "int".into())]);
    for local in [("q", "MessageQueue"), ("s", "Service"), ("it", "Item"), ("r", "Reader"), ("e", "IOException")] {
        assert!(m.locals.contains(&(local.0.into(), local.1.into())), "missing local {local:?} in {:?}", m.locals);
    }
}

#[test]
fn nested_and_anonymous_types() {
    let src = r#"
class Outer {
    static class Inner { void i() {} }
    void make() {
        new Thread(new Runnable() { public void run() { work(); } }).start();
    }
}
"#;
    let f = parse_java(src.as_bytes());
    let outer = ty(&f, "Outer");
    assert_eq!(outer.nested.len(), 1);
    assert_eq!(outer.nested[0].name, "Inner");
    let make = method(outer, "make");
    assert!(make.calls.iter().any(|c| c.method == "work"));
    assert!(make.calls.iter().any(|c| c.method == "start"));
}

#[test]
fn constructors_are_methods() {
    let src = "class C { C(Dep d) { d.init(); } }";
    let f = parse_java(src.as_bytes());
    let ctor = method(ty(&f, "C"), "<init>");
    assert_eq!(ctor.calls[0].method, "init");
}

#[test]
fn syntax_error_still_yields_types() {
    let src = r#"
class Broken {
    void good() { a.b(); }
    void bad() { if (x { y(); }
    void alsoGood() {}
}
class Fine {}
"#;
    let f = parse_java(src.as_bytes());
    let broken = ty(&f, "Broken");
    assert!(broken.methods.iter().any(|m| m.name == "good"));
    assert!(f.types.iter().any(|t| t.name == "Fine"));
}

#[test]
fn methods_record_what_makes_them_complex() {
    let src = r#"
class C {
    int hard(int x, boolean a) {
        if (x > 0) { x--; }
        for (int i = 0; i < x; i++) {
            if (a && i > 1) { return 1; }
        }
        return 0;
    }
    int easy() { return 1; }
}
"#;
    let f = parse_java(src.as_bytes());
    let t = ty(&f, "C");
    let hard = &method(t, "hard").complexity;
    assert_eq!(hard.total, 5);
    assert_eq!(hard.decisions, [("if", 2), ("for", 1), ("&&", 1)]);
    assert_eq!(hard.nesting, 2);
    let easy = &method(t, "easy").complexity;
    assert_eq!((easy.total, easy.decisions.len(), easy.nesting), (1, 0, 0));
}

#[test]
fn an_else_if_chain_does_not_count_as_nesting() {
    let src = r#"
class C {
    int flat(int x) {
        if (x == 1) { return 1; }
        else if (x == 2) { return 2; }
        else if (x == 3) { return 3; }
        else { return 0; }
    }
    int deep(int x) {
        if (x == 1) {
            if (x == 2) { return 1; } else if (x == 3) { return 2; }
        }
        return 0;
    }
}
"#;
    let f = parse_java(src.as_bytes());
    let t = ty(&f, "C");
    let flat = &method(t, "flat").complexity;
    assert_eq!((flat.total, flat.nesting), (4, 1));
    assert_eq!(method(t, "deep").complexity.nesting, 2);
}
