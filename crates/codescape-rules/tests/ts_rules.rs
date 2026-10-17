use codescape_facts::*;
use codescape_format::*;
use codescape_rules::*;
use std::collections::HashMap;

struct Fx {
    b: GraphBuilder,
    facts: ProjectFacts,
    sources: HashMap<String, Vec<u8>>,
}

impl Fx {
    fn new() -> Self {
        Fx { b: GraphBuilder::new("p"), facts: ProjectFacts::default(), sources: HashMap::new() }
    }

    fn file(&mut self, module: &str, rel: &str, imports: &[&str]) -> usize {
        let root = self.b.root();
        let m = self.b.child(root, module).unwrap_or_else(|| self.b.add_node(root, NodeKind::Module, SubKind::None, module, Lang::TypeScript));
        let dir = rel.split_once('/').map(|(_, r)| r).unwrap_or("").rsplit_once('/').map(|(d, _)| d).unwrap_or("(root)");
        let pkg = self.b.child(m, dir).unwrap_or_else(|| self.b.add_node(m, NodeKind::Package, SubKind::None, dir, Lang::TypeScript));
        let lang = if rel.ends_with(".js") { Lang::JavaScript } else { Lang::TypeScript };
        self.facts.files.push(FileFacts {
            rel_path: rel.into(),
            module: m,
            package: Some(pkg),
            import_targets: Vec::new(),
            lang,
            is_test: false,
            is_dev: false,
            imports: imports.iter().map(|p| Import { path: p.to_string(), ..Default::default() }).collect(),
            types: Vec::new(),
            functions: Vec::new(),
            calls: Vec::new(),
        });
        self.facts.files.len() - 1
    }

    fn ty(&mut self, file: usize, name: &str, annotations: &[&str]) -> NodeId {
        let pkg = self.facts.files[file].package.unwrap();
        let node = self.b.add_node(pkg, NodeKind::Type, SubKind::Class, name, Lang::TypeScript);
        let stem = self.facts.files[file].rel_path.trim_end_matches(".ts").to_string();
        self.facts.files[file].types.push(TypeFacts {
            node,
            fqn: format!("{stem}.{name}"),
            supertypes: Vec::new(),
            annotations: annotations.iter().map(|a| Annotation { name: a.to_string(), args: Vec::new() }).collect(),
        });
        node
    }

    fn func(&mut self, file: usize, name: &str, is_main: bool) -> NodeId {
        let pkg = self.facts.files[file].package.unwrap();
        let node = self.b.add_node(pkg, NodeKind::Member, SubKind::Function, name, Lang::TypeScript);
        let lines = self.sources.get(&self.facts.files[file].rel_path).map_or(1, |s| s.iter().filter(|&&c| c == b'\n').count() as u32 + 1);
        self.b.set_span(node, &self.facts.files[file].rel_path.clone(), 1, lines);
        self.facts.files[file].functions.push(FnFacts {
            node,
            owner: None,
            name: name.into(),
            params: Vec::new(),
            annotations: Vec::new(),
            is_main,
            overrides_external: None,
        });
        node
    }

    fn call(&mut self, file: usize, c: CallFacts) {
        self.facts.files[file].calls.push(c);
    }

    fn run(&mut self) {
        let packs = load_packs(None, None, &PackSelection::default()).unwrap();
        let sources = self.sources.clone();
        apply_detection(&packs, &self.facts, &move |p: &str| sources.get(p).cloned(), &mut self.b);
    }

    fn entry(&self, label: &str) -> &EntryPointSpec {
        self.b.entries().iter().find(|e| e.label == label).unwrap_or_else(|| panic!("no entry {label}: {:?}", self.labels()))
    }

    fn entry_index(&self, label: &str) -> u32 {
        self.b.entries().iter().position(|e| e.label == label).unwrap() as u32
    }

    fn labels(&self) -> Vec<String> {
        self.b.entries().iter().map(|e| e.label.clone()).collect()
    }
}

fn strings(a: &[Option<&str>]) -> Vec<Option<String>> {
    a.iter().map(|s| s.map(String::from)).collect()
}

fn angular_app(fx: &mut Fx) -> NodeId {
    let main = fx.file("web", "web/src/main.ts", &["@angular/platform-browser", "./app/app.component"]);
    let top = fx.func(main, "main.ts", false);
    fx.call(
        main,
        CallFacts { caller: top, method: "bootstrapApplication".into(), ident_args: strings(&[Some("AppComponent"), None]), line: 4, ..Default::default() },
    );
    top
}

#[test]
fn angular_bootstrap_call_is_the_application_entry() {
    let mut fx = Fx::new();
    let top = angular_app(&mut fx);
    fx.run();
    let e = fx.entry("web");
    assert_eq!(e.node, top);
    assert_eq!(e.kind, "main");
    assert_eq!(e.category, Category::Application);
    assert_eq!(e.handoff.as_deref(), Some("Angular"));
}

#[test]
fn package_main_entries_are_labelled_by_module() {
    let mut fx = Fx::new();
    let f = fx.file("server", "server/src/server.js", &["express"]);
    let top = fx.func(f, "server.js", true);
    fx.run();
    assert_eq!(fx.entry("server").node, top);
}

const ROUTES: &str = r#"import { Routes } from "@angular/router";
import { LayoutComponent } from "./layout/layout.component";
import { HomeComponent } from "@app/home/home.component";

const loadShell = () => import("./layout/shell.component").then((m) => m.ShellComponent);

export const routes: Routes = [
  {
    path: "",
    component: LayoutComponent,
    children: [
      { path: "", redirectTo: "login", pathMatch: "full" },
      { path: "login", loadComponent: () => import("./features/login/login.component").then((m) => m.LoginComponent) },
    ],
  },
  {
    path: "app",
    loadComponent: loadShell,
    children: [
      { path: "home", component: HomeComponent },
      { path: "settings", children: [{ path: "billing", loadComponent: () => import("./features/billing").then((m) => m.BillingComponent) }] },
    ],
  },
  { path: "**", redirectTo: "login" },
];
"#;

#[test]
fn angular_routes_become_route_entries_on_their_components() {
    let mut fx = Fx::new();
    angular_app(&mut fx);
    fx.sources.insert("web/src/app/app.routes.ts".into(), ROUTES.as_bytes().to_vec());
    let routes = fx.file("web", "web/src/app/app.routes.ts", &["@angular/router", "./layout/layout.component"]);
    fx.func(routes, "app.routes.ts", false);
    let layout_f = fx.file("web", "web/src/app/layout/layout.component.ts", &[]);
    let layout = fx.ty(layout_f, "LayoutComponent", &["Component"]);
    let shell_f = fx.file("web", "web/src/app/layout/shell.component.ts", &[]);
    let shell = fx.ty(shell_f, "ShellComponent", &["Component"]);
    let login_f = fx.file("web", "web/src/app/features/login/login.component.ts", &[]);
    let login = fx.ty(login_f, "LoginComponent", &["Component"]);
    let home_f = fx.file("web", "web/src/app/home/home.component.ts", &[]);
    let home = fx.ty(home_f, "HomeComponent", &["Component"]);
    let billing_f = fx.file("web", "web/src/app/features/billing/index.ts", &[]);
    let billing = fx.ty(billing_f, "BillingComponent", &["Component"]);
    fx.run();
    let app = fx.entry_index("web");
    for (label, node) in [("/", layout), ("/login", login), ("/app", shell), ("/app/home", home), ("/app/settings/billing", billing)] {
        let e = fx.entry(label);
        assert_eq!(e.node, node, "{label}");
        assert_eq!(e.kind, "route");
        assert_eq!(e.parent, Some(app), "{label}");
    }
    assert_eq!(fx.b.entries().iter().filter(|e| e.kind == "route").count(), 5, "{:?}", fx.labels());
}

#[test]
fn angular_components_and_services_take_layer_roles_and_providers_are_wiring() {
    let mut fx = Fx::new();
    let f = fx.file("web", "web/src/app/x.ts", &["@angular/core"]);
    let comp = fx.ty(f, "Widget", &["Component"]);
    let svc = fx.ty(f, "Api", &["Injectable"]);
    let module = fx.ty(f, "AppModule", &["NgModule"]);
    let cfg_f = fx.file("web", "web/src/app/app.config.ts", &["@angular/core", "@angular/router"]);
    let cfg = fx.func(cfg_f, "app.config.ts", false);
    fx.call(cfg_f, CallFacts { caller: cfg, method: "provideRouter".into(), ident_args: strings(&[Some("routes")]), line: 3, ..Default::default() });
    let plain_f = fx.file("web", "web/src/app/plain.ts", &["@angular/core"]);
    let plain = fx.func(plain_f, "helper", false);
    fx.call(plain_f, CallFacts { caller: plain, method: "format".into(), line: 2, ..Default::default() });
    fx.run();
    assert!(fx.b.has_tag(comp, "layer:handler"));
    assert!(fx.b.has_tag(svc, "layer:service"));
    assert!(fx.b.flags(module).contains(NodeFlags::WIRING));
    assert!(fx.b.flags(cfg).contains(NodeFlags::WIRING));
    assert!(!fx.b.flags(plain).contains(NodeFlags::WIRING));
    assert!(!fx.b.flags(svc).contains(NodeFlags::DETAIL));
}

#[test]
fn express_routes_from_app_and_router_calls() {
    let mut fx = Fx::new();
    let f = fx.file("server", "server/src/server.js", &["express"]);
    let top = fx.func(f, "server.js", true);
    let routes_f = fx.file("server", "server/src/routes.js", &[]);
    let list = fx.func(routes_f, "list", false);
    let call = |method: &str, recv: &str, ty: &str, path: &str, target: Option<NodeId>| CallFacts {
        caller: top,
        method: method.into(),
        receiver_var: Some(recv.into()),
        receiver_type: Some(ty.into()),
        string_args: strings(&[Some(path), None]),
        arg_targets: vec![None, target],
        line: 5,
        ..Default::default()
    };
    fx.call(f, call("get", "app", "express()", "/users", Some(list)));
    fx.call(f, call("post", "router", "express.Router()", "/items", None));
    fx.call(f, call("get", "http", "@angular/common/http.HttpClient", "/nope", None));
    fx.run();
    let users = fx.entry("GET /users");
    assert_eq!(users.node, list);
    assert_eq!(users.kind, "http-route");
    assert_eq!(users.parent, Some(fx.entry_index("server")));
    assert_eq!(fx.entry("POST /items").node, top);
    assert!(!fx.labels().iter().any(|l| l.contains("/nope")));
}

#[test]
fn test_folders_and_testing_imports_are_test_code() {
    let mut fx = Fx::new();
    let e2e = fx.file("web", "web/e2e/login.ts", &[]);
    let e2e_fn = fx.func(e2e, "login", false);
    let helper = fx.file("web", "web/src/testing/harness.ts", &["@angular/core/testing"]);
    let helper_fn = fx.func(helper, "harness", false);
    let app = fx.file("web", "web/src/app/a.ts", &["@angular/core"]);
    let app_fn = fx.func(app, "a", false);
    fx.run();
    assert!(fx.b.flags(e2e_fn).contains(NodeFlags::TEST));
    assert!(fx.b.flags(helper_fn).contains(NodeFlags::TEST));
    assert!(!fx.b.flags(app_fn).contains(NodeFlags::TEST));
}
