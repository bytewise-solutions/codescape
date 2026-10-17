use srcrelief_format::SubKind;
use srcrelief_ts::parse::*;

fn ts(src: &str) -> TsFile {
    parse_ts(src.as_bytes(), Dialect::TypeScript)
}

fn ty<'a>(f: &'a TsFile, name: &str) -> &'a TsType {
    f.types.iter().find(|t| t.name == name).unwrap_or_else(|| panic!("no type {name}"))
}

fn method<'a>(t: &'a TsType, name: &str) -> &'a TsFunc {
    t.methods.iter().find(|m| m.name == name).unwrap_or_else(|| panic!("no method {name}"))
}

#[test]
fn imports_and_reexports() {
    let f = ts(
        "import { A, B as C } from './a';\nimport D from 'x';\nimport * as ns from 'y';\nimport type { T } from './t';\nexport { E } from './e';\nexport * from './f';\nconst r = require('z');\nconst { p, q: s } = require('w');\n",
    );
    let spec = |s: &str| f.imports.iter().find(|i| i.spec == s).unwrap_or_else(|| panic!("no import {s}"));
    assert_eq!(spec("./a").named, [("A".to_string(), "A".to_string()), ("B".into(), "C".into())]);
    assert_eq!(spec("x").default.as_deref(), Some("D"));
    assert_eq!(spec("y").namespace.as_deref(), Some("ns"));
    assert_eq!(spec("./t").named, [("T".to_string(), "T".to_string())]);
    assert_eq!(spec("z").default.as_deref(), Some("r"));
    assert_eq!(spec("w").named, [("p".to_string(), "p".to_string()), ("q".into(), "s".into())]);
    assert_eq!(f.reexports, [ReExport { spec: "./e".into(), names: Some(vec![("E".into(), "E".into())]) }, ReExport { spec: "./f".into(), names: None }]);
    assert!(f.top.calls.is_empty(), "require is an import, not a call: {:?}", f.top.calls);
}

#[test]
fn decorated_class_with_heritage_fields_and_members() {
    let f = ts(r#"
@Component({ selector: "app-x", imports: [Child, Other] })
export class Foo extends Base<T> implements I, J {
  @Input() name: string;
  private svc = inject(FooService);
  handle = (x: number) => this.svc.go(x);
  constructor(private readonly http: HttpClient, @Inject(TOKEN) public tok: Tok, plain: number) { super(); }
  @HostListener("click") onClick(): void { this.svc?.run("a", b); new Bar(); }
  get value(): string { return ""; }
  set value(v: string) {}
  static make(): Foo { return new Foo(); }
}
"#);
    let t = ty(&f, "Foo");
    assert_eq!(t.kind, SubKind::Class);
    assert_eq!(t.extends, ["Base"]);
    assert_eq!(t.implements, ["I", "J"]);
    assert_eq!(t.decorators[0].name, "Component");
    assert_eq!(t.decorators[0].args, ["app-x"]);
    assert_eq!(t.decorator_refs, ["Child", "Other"]);
    assert_eq!(t.start_line, 2, "a decorated class starts at its decorator");
    let fields: Vec<(&str, &TypeExpr)> = t.fields.iter().map(|(n, ty)| (n.as_str(), ty)).collect();
    assert!(fields.contains(&("name", &TypeExpr::Named("string".into()))));
    assert!(fields.contains(&("svc", &TypeExpr::Named("FooService".into()))));
    assert!(fields.contains(&("http", &TypeExpr::Named("HttpClient".into()))));
    assert!(fields.contains(&("tok", &TypeExpr::Named("Tok".into()))));
    assert!(!fields.iter().any(|(n, _)| *n == "plain"));
    let mut names: Vec<&str> = t.methods.iter().map(|m| m.name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["constructor", "handle", "make", "onClick", "value"]);
    let ctor = method(t, "constructor");
    assert_eq!(ctor.sub, SubKind::Constructor);
    assert!(ctor.decorators.iter().any(|d| d.name == "Inject"));
    assert_eq!(ctor.calls[0].method, "inject", "field initialisers join the explicit constructor");
    assert_eq!(ctor.params.len(), 3);
    let click = method(t, "onClick");
    assert_eq!(click.decorators[0].name, "HostListener");
    assert_eq!(click.calls[0].receiver.as_deref(), Some("this.svc"));
    assert_eq!(click.calls[0].method, "run");
    assert_eq!(click.calls[0].string_args, [Some("a".to_string()), None]);
    assert_eq!(click.calls[0].ident_args, [None, Some("b".to_string())]);
    assert!(click.calls[1].is_new && click.calls[1].method == "Bar");
    assert_eq!(method(t, "handle").calls[0].receiver.as_deref(), Some("this.svc"));
    assert_eq!(method(t, "make").returns.as_deref(), Some("Foo"));
}

#[test]
fn field_initialisers_run_in_the_constructor() {
    let f = ts("class A {\n  b = this.make();\n  constructor() { this.c(); }\n}\nclass B {\n  x = signal(1);\n}\n");
    let ctor = method(ty(&f, "A"), "constructor");
    let called: Vec<&str> = ctor.calls.iter().map(|c| c.method.as_str()).collect();
    assert_eq!(called, ["make", "c"]);
    assert_eq!(method(ty(&f, "B"), "constructor").calls[0].method, "signal");
}

#[test]
fn top_level_functions_and_module_code() {
    let f = ts(r#"
import { bootstrapApplication } from "@angular/platform-browser";
export default function () { helper(); }
export function named(a: A): B { return a.b(); }
export const arrow = async (x: Foo): Promise<Bar> => { const y = await x.load(); return y; };
const notFn = 1;
bootstrapApplication(AppComponent, appConfig).catch((err) => console.error(err));
"#);
    let names: Vec<&str> = f.funcs.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, ["default", "named", "arrow"]);
    assert_eq!(f.default_export.as_deref(), Some("default"));
    let arrow = &f.funcs[2];
    assert_eq!(arrow.params, [("x".to_string(), "Foo".to_string())]);
    assert_eq!(arrow.returns.as_deref(), Some("Promise<Bar>"));
    assert_eq!(arrow.locals, [("y".to_string(), TypeExpr::Await(Box::new(TypeExpr::CallOf("x.load".into()))))]);
    let top: Vec<(Option<&str>, &str)> = f.top.calls.iter().map(|c| (c.receiver.as_deref(), c.method.as_str())).collect();
    assert_eq!(top, [(Some("bootstrapApplication()"), "catch"), (None, "bootstrapApplication"), (Some("console"), "error")]);
    assert_eq!(f.top.calls[1].ident_args, [Some("AppComponent".to_string()), Some("appConfig".to_string())]);
    assert!(f.vars.contains(&("notFn".to_string(), TypeExpr::Named("number".into()))));
}

#[test]
fn interfaces_enums_aliases_and_abstract_classes() {
    let f = ts(
        "interface I extends K, L { m(a: string): number; p: Foo; }\nenum E { A }\ntype T = { a: string };\nexport abstract class Q { abstract run(): void; }\n",
    );
    let i = ty(&f, "I");
    assert_eq!(i.kind, SubKind::Interface);
    assert_eq!(i.extends, ["K", "L"]);
    assert_eq!(method(i, "m").params, [("a".to_string(), "string".to_string())]);
    assert_eq!(i.fields, [("p".to_string(), TypeExpr::Named("Foo".into()))]);
    assert_eq!(ty(&f, "E").kind, SubKind::Enum);
    assert_eq!(ty(&f, "T").kind, SubKind::NamedType);
    assert_eq!(ty(&f, "Q").kind, SubKind::Class);
    assert_eq!(method(ty(&f, "Q"), "run").sub, SubKind::Method);
}

#[test]
fn locals_are_typed_from_annotations_new_inject_and_casts() {
    let f = ts(
        "function f(p: P) {\n  const a: A = x;\n  const b = new B();\n  const c = inject(C);\n  const d = y as D;\n  let e = this.svc;\n  items.forEach((it: Item) => it.go());\n}\n",
    );
    let l = &f.funcs[0].locals;
    for (name, want) in [
        ("a", TypeExpr::Named("A".into())),
        ("b", TypeExpr::Named("B".into())),
        ("c", TypeExpr::Named("C".into())),
        ("d", TypeExpr::Named("D".into())),
        ("e", TypeExpr::Path("this.svc".into())),
        ("it", TypeExpr::Named("Item".into())),
    ] {
        assert!(l.contains(&(name.to_string(), want.clone())), "{name}: {l:?}");
    }
}

#[test]
fn types_normalise_arrays_unions_and_generics() {
    let f = ts("class A { a: B[]; b: Array<C>; c: D | null; d: Map<string, E>; e: ns.F; }\n");
    let fields = &ty(&f, "A").fields;
    let want = [("a", "[]"), ("b", "[]"), ("c", "D"), ("d", "Map"), ("e", "ns.F")];
    for (name, t) in want {
        assert!(fields.contains(&(name.to_string(), TypeExpr::Named(t.into()))), "{name}: {fields:?}");
    }
}

#[test]
fn javascript_classes_commonjs_exports_and_dynamic_imports() {
    let f = parse_ts(
        b"const express = require('express');\nclass Svc extends Base { constructor() { super(); this.repo = new Repo(); }\n  handle = (req) => this.run(req);\n  run(x) { return import('./lazy.js'); }\n}\nmodule.exports.start = function () { app.listen(3000); };\nexports.stop = () => {};\n",
        Dialect::JavaScript,
    );
    let t = ty(&f, "Svc");
    assert_eq!(t.extends, ["Base"]);
    let names: Vec<&str> = t.methods.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["constructor", "handle", "run"]);
    let fns: Vec<&str> = f.funcs.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(fns, ["start", "stop"]);
    assert!(f.imports.iter().any(|i| i.spec == "express" && i.default.as_deref() == Some("express")));
}

#[test]
fn chained_receivers_keep_the_call_path() {
    let f = ts(
        "class A {\n  ref = inject(MatDialogRef<A>);\n  run() {\n    this.http.get(url).pipe(map(toX)).subscribe();\n    (a || b).go();\n    const local = (x: number) => x;\n    local(1);\n  }\n}\n",
    );
    let t = ty(&f, "A");
    assert!(t.fields.contains(&("ref".to_string(), TypeExpr::Named("MatDialogRef".into()))));
    let run = method(t, "run");
    let recv = |m: &str| run.calls.iter().find(|c| c.method == m).and_then(|c| c.receiver.clone());
    assert_eq!(recv("subscribe").as_deref(), Some("this.http.get().pipe()"));
    assert_eq!(recv("pipe").as_deref(), Some("this.http.get()"));
    assert_eq!(recv("go").as_deref(), Some(CHAIN));
    assert!(run.locals.contains(&("local".to_string(), TypeExpr::Named("Function".into()))));
}

#[test]
fn tsx_parses_with_jsx() {
    let f = parse_ts(b"export function View() { const s = useState(0); return <div onClick={() => go()}>{s}</div>; }\n", Dialect::Tsx);
    let calls: Vec<&str> = f.funcs[0].calls.iter().map(|c| c.method.as_str()).collect();
    assert_eq!(calls, ["useState", "go"]);
}
