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

    fn module(&mut self, name: &str) -> NodeId {
        let root = self.b.root();
        self.b.child(root, name).unwrap_or_else(|| self.b.add_node(root, NodeKind::Module, SubKind::None, name, Lang::Java))
    }

    fn file(&mut self, module: &str, rel: &str, lang: Lang, is_test: bool, imports: &[&str]) -> usize {
        let m = self.module(module);
        self.facts.files.push(FileFacts {
            rel_path: rel.into(),
            module: m,
            package: None,
            import_targets: Vec::new(),
            lang,
            is_test,
            is_dev: false,
            imports: imports.iter().map(|p| Import { path: p.to_string(), ..Default::default() }).collect(),
            types: Vec::new(),
            functions: Vec::new(),
            calls: Vec::new(),
        });
        self.facts.files.len() - 1
    }

    fn ty(&mut self, file: usize, fqn: &str, supers: &[&str], annotations: &[&str]) -> NodeId {
        let module = self.facts.files[file].module;
        let pkg = self.b.add_node(module, NodeKind::Package, SubKind::None, "pkg", Lang::Java);
        let name = fqn.rsplit('.').next().unwrap();
        let node = self.b.add_node(pkg, NodeKind::Type, SubKind::Class, name, Lang::Java);
        self.facts.files[file].types.push(TypeFacts {
            node,
            fqn: fqn.into(),
            supertypes: supers.iter().map(|s| s.to_string()).collect(),
            annotations: annotations.iter().map(|a| Annotation { name: a.to_string(), args: Vec::new() }).collect(),
        });
        node
    }

    fn func(&mut self, file: usize, owner: Option<NodeId>, name: &str, is_main: bool, overrides: Option<&str>, params: &[(&str, &str)]) -> NodeId {
        let parent = owner.unwrap_or_else(|| {
            let module = self.facts.files[file].module;
            let rel = self.facts.files[file].rel_path.clone();
            let dir = rel.split_once('/').map(|(_, rest)| rest).unwrap_or("").rsplit_once('/').map(|(d, _)| d).unwrap_or("(root)");
            self.b.add_node(module, NodeKind::Package, SubKind::None, dir, Lang::Go)
        });
        let node = self.b.add_node(parent, NodeKind::Member, SubKind::Method, name, self.facts.files[file].lang);
        self.facts.files[file].functions.push(FnFacts {
            node,
            owner,
            name: name.into(),
            params: params.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
            annotations: Vec::new(),
            is_main,
            overrides_external: overrides.map(String::from),
        });
        node
    }

    fn call(&mut self, file: usize, c: CallFacts) {
        if let Some(t) = c.target {
            self.b.add_edge(c.caller, t, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
        }
        self.facts.files[file].calls.push(c);
    }

    fn run(&mut self, packs: &Packs) -> DetectStats {
        let sources = self.sources.clone();
        apply_detection(packs, &self.facts, &move |p: &str| sources.get(p).cloned(), &mut self.b)
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

fn builtins() -> Packs {
    load_packs(None, None, &PackSelection::default()).unwrap()
}

fn without(pack: &str) -> Packs {
    load_packs(None, None, &PackSelection { disable: vec![pack.into()], ..Default::default() }).unwrap()
}

fn str_args(a: &[Option<&str>]) -> Vec<Option<String>> {
    a.iter().map(|s| s.map(String::from)).collect()
}

#[test]
fn mains_become_entries_with_path_categories() {
    let mut fx = Fx::new();
    let f1 = fx.file("app", "app/src/main/java/a/Main.java", Lang::Java, false, &[]);
    let t1 = fx.ty(f1, "a.Main", &[], &[]);
    fx.func(f1, Some(t1), "main", true, None, &[]);
    let f2 = fx.file("app", "app/src/test/java/a/LoadTest.java", Lang::Java, true, &[]);
    let t2 = fx.ty(f2, "a.LoadTest", &[], &[]);
    fx.func(f2, Some(t2), "main", true, None, &[]);
    let f3 = fx.file("core", "core/src/a/tools/Gen.java", Lang::Java, false, &[]);
    let t3 = fx.ty(f3, "a.tools.Gen", &[], &[]);
    fx.func(f3, Some(t3), "main", true, None, &[]);
    let f4 = fx.file("svc", "svc/cmd/svc/main.go", Lang::Go, false, &[]);
    fx.func(f4, None, "main", true, None, &[]);
    fx.run(&builtins());
    assert_eq!(fx.entry("Main").category, Category::Application);
    assert_eq!(fx.entry("Main").kind, "main");
    assert_eq!(fx.entry("LoadTest").category, Category::Test);
    assert_eq!(fx.entry("Gen").category, Category::DevTool);
    assert_eq!(fx.entry("svc").category, Category::Application);
}

fn gdx_app(fx: &mut Fx) -> (NodeId, NodeId, NodeId) {
    let f = fx.file("desktop", "desktop/src/d/Desktop.java", Lang::Java, false, &[]);
    let desktop = fx.ty(f, "d.Desktop", &[], &[]);
    let main = fx.func(f, Some(desktop), "main", true, None, &[]);
    let g = fx.file("client", "client/src/c/ClientApp.java", Lang::Java, false, &[]);
    let app = fx.ty(g, "c.ClientApp", &["com.badlogic.gdx.Game"], &[]);
    let create = fx.func(g, Some(app), "create", false, Some("com.badlogic.gdx.Game"), &[]);
    let render = fx.func(g, Some(app), "render", false, Some("com.badlogic.gdx.Game"), &[]);
    fx.func(g, Some(app), "helper", false, None, &[]);
    fx.b.add_edge(main, app, EdgeKind::Instantiates, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    fx.call(f, CallFacts { caller: main, method: "ClientApp".into(), instantiates: Some("c.ClientApp".into()), ..Default::default() });
    let _ = create;
    (main, app, render)
}

#[test]
fn supertype_callbacks_nest_under_reaching_main() {
    let mut fx = Fx::new();
    gdx_app(&mut fx);
    fx.run(&builtins());
    let main = fx.entry_index("Desktop");
    let create = fx.entry("ClientApp.create()");
    assert_eq!(create.lifecycle, Lifecycle::Once);
    assert_eq!(create.parent, Some(main));
    assert_eq!(create.kind, "framework-callback");
    let render = fx.entry("ClientApp.render()");
    assert_eq!(render.lifecycle, Lifecycle::Repeating);
    assert!(create.order < render.order);
    assert!(fx.b.entries().iter().all(|e| e.label != "ClientApp.helper()"));
}

#[test]
fn fallback_callbacks_without_pack() {
    let mut fx = Fx::new();
    gdx_app(&mut fx);
    fx.run(&without("libgdx"));
    let render = fx.entry("ClientApp.render()");
    assert_eq!(render.lifecycle, Lifecycle::Unknown);
    assert_eq!(render.rule, "builtin.fallback-callback");
    assert_eq!(render.parent, Some(fx.entry_index("Desktop")));
}

#[test]
fn instantiates_sets_handoff() {
    let mut fx = Fx::new();
    let (main, _, _) = gdx_app(&mut fx);
    fx.call(
        0,
        CallFacts {
            caller: main,
            method: "Lwjgl3Application".into(),
            instantiates: Some("com.badlogic.gdx.backends.lwjgl3.Lwjgl3Application".into()),
            ..Default::default()
        },
    );
    fx.run(&builtins());
    assert_eq!(fx.entry("Desktop").handoff.as_deref(), Some("libGDX"));
}

#[test]
fn annotation_handoff_merges_into_main() {
    let mut fx = Fx::new();
    let f = fx.file("auth", "auth/src/main/java/a/Application.java", Lang::Java, false, &[]);
    let t = fx.ty(f, "a.Application", &[], &["SpringBootApplication"]);
    fx.func(f, Some(t), "main", true, None, &[]);
    fx.run(&builtins());
    assert_eq!(fx.b.entries().len(), 1);
    assert_eq!(fx.entry("Application").handoff.as_deref(), Some("Spring Boot"));
}

#[test]
fn annotation_routes_with_type_prefix() {
    let mut fx = Fx::new();
    let f = fx.file("auth", "auth/src/main/java/a/Application.java", Lang::Java, false, &[]);
    let t = fx.ty(f, "a.Application", &[], &["SpringBootApplication"]);
    fx.func(f, Some(t), "main", true, None, &[]);
    let g = fx.file("auth", "auth/src/main/java/a/UserController.java", Lang::Java, false, &[]);
    let c = fx.ty(g, "a.UserController", &[], &[]);
    fx.facts.files[g].types[0].annotations.push(Annotation { name: "RequestMapping".into(), args: vec!["/users".into()] });
    let list = fx.func(g, Some(c), "list", false, None, &[]);
    fx.facts.files[g].functions[0].annotations.push(Annotation { name: "GetMapping".into(), args: vec!["/all".into()] });
    fx.run(&builtins());
    let e = fx.entry("GET /users/all");
    assert_eq!(e.node, list);
    assert_eq!(e.kind, "http-route");
    assert_eq!(e.parent, Some(fx.entry_index("Application")), "same-module main when nothing calls the controller");
}

#[test]
fn call_route_joins_group_prefixes_across_functions() {
    let mut fx = Fx::new();
    let fiber = &["github.com/gofiber/fiber/v2"];
    let f = fx.file("portal-api", "portal-api/cmd/portal-api/main.go", Lang::Go, false, fiber);
    let main = fx.func(f, None, "main", true, None, &[]);
    let g = fx.file("portal-api", "portal-api/internal/h/h.go", Lang::Go, false, fiber);
    let h = fx.ty(g, "x/internal/h.H", &[], &[]);
    let register = fx.func(g, Some(h), "RegisterRoutes", false, None, &[("r", "fiber.Router")]);
    let rates = fx.func(g, Some(h), "Rates", false, None, &[("c", "fiber.Ctx")]);
    fx.call(
        f,
        CallFacts { caller: main, method: "New".into(), receiver_var: Some("fiber".into()), assigned_to: Some("app".into()), line: 1, ..Default::default() },
    );
    fx.call(
        f,
        CallFacts {
            caller: main,
            method: "Group".into(),
            receiver_var: Some("app".into()),
            string_args: str_args(&[Some("/api/v1")]),
            assigned_to: Some("api".into()),
            line: 2,
            ..Default::default()
        },
    );
    fx.call(
        f,
        CallFacts {
            caller: main,
            method: "Group".into(),
            receiver_var: Some("api".into()),
            string_args: str_args(&[Some("/tenants/:id"), None]),
            assigned_to: Some("t".into()),
            line: 3,
            ..Default::default()
        },
    );
    fx.call(
        f,
        CallFacts {
            caller: main,
            method: "RegisterRoutes".into(),
            receiver_var: Some("h".into()),
            ident_args: str_args(&[Some("t")]),
            target: Some(register),
            line: 4,
            ..Default::default()
        },
    );
    fx.call(
        g,
        CallFacts {
            caller: register,
            method: "Group".into(),
            receiver_var: Some("r".into()),
            receiver_type: Some("fiber.Router".into()),
            string_args: str_args(&[Some("/stats")]),
            assigned_to: Some("g".into()),
            line: 10,
            ..Default::default()
        },
    );
    fx.call(
        g,
        CallFacts {
            caller: register,
            method: "Get".into(),
            receiver_var: Some("g".into()),
            string_args: str_args(&[Some("/rates"), None]),
            ident_args: str_args(&[None, Some("h.Rates")]),
            arg_targets: vec![None, Some(rates)],
            line: 11,
            ..Default::default()
        },
    );
    fx.run(&builtins());
    let e = fx.entry("GET /api/v1/tenants/:id/stats/rates");
    assert_eq!(e.node, rates);
    assert_eq!(e.kind, "http-route");
    assert_eq!(e.lifecycle, Lifecycle::OnEvent);
    assert_eq!(e.parent, Some(fx.entry_index("portal-api")));
    assert_eq!(fx.b.entries().iter().filter(|e| e.kind == "http-route").count(), 1, "Group calls are not routes");
}

#[test]
fn call_route_unknown_param_prefix_is_ellipsis() {
    let mut fx = Fx::new();
    let fiber = &["github.com/gofiber/fiber/v2"];
    let g = fx.file("api", "api/h.go", Lang::Go, false, fiber);
    let reg = fx.func(g, None, "Register", false, None, &[("r", "fiber.Router")]);
    fx.call(
        g,
        CallFacts {
            caller: reg,
            method: "Post".into(),
            receiver_var: Some("r".into()),
            receiver_type: Some("fiber.Router".into()),
            string_args: str_args(&[Some("/x"), None]),
            ..Default::default()
        },
    );
    fx.run(&builtins());
    fx.entry("POST …/x");
}

#[test]
fn call_route_ignores_non_route_get() {
    let mut fx = Fx::new();
    let f = fx.file("svc", "svc/cfg.go", Lang::Go, false, &["github.com/spf13/viper"]);
    let func = fx.func(f, None, "load", false, None, &[]);
    fx.call(f, CallFacts { caller: func, method: "Get".into(), receiver_var: Some("cfg".into()), string_args: str_args(&[Some("key")]), ..Default::default() });
    let g = fx.file("svc", "svc/routes.go", Lang::Go, false, &["github.com/gofiber/fiber/v2"]);
    let r = fx.func(g, None, "routes", false, None, &[]);
    fx.call(g, CallFacts { caller: r, method: "Get".into(), receiver_var: Some("m".into()), string_args: str_args(&[Some("key")]), ..Default::default() });
    fx.run(&builtins());
    assert!(fx.b.entries().iter().all(|e| e.kind != "http-route"), "{:?}", fx.labels());
}

#[test]
fn call_handler_message_consumer() {
    let mut fx = Fx::new();
    let f = fx.file("job-engine", "job-engine/internal/c/consumer.go", Lang::Go, false, &["github.com/rabbitmq/amqp091-go"]);
    let consume = fx.func(f, None, "Start", false, None, &[]);
    fx.call(
        f,
        CallFacts {
            caller: consume,
            method: "Consume".into(),
            receiver_var: Some("channel".into()),
            string_args: str_args(&[None, Some("")]),
            ident_args: str_args(&[Some("queueName"), None]),
            ..Default::default()
        },
    );
    fx.run(&builtins());
    let e = fx.b.entries().iter().find(|e| e.kind == "message-handler").expect("consumer entry");
    assert_eq!(e.node, consume);
    assert_eq!(e.lifecycle, Lifecycle::OnEvent);
}

#[test]
fn dispatch_edge_added() {
    let mut fx = Fx::new();
    let (_, _, render) = gdx_app(&mut fx);
    let f = fx.file("client", "client/src/c/screens.java", Lang::Java, false, &[]);
    let abs = fx.ty(f, "c.BaseView", &["com.badlogic.gdx.Screen"], &[]);
    let abs_render = fx.func(f, Some(abs), "render", false, Some("com.badlogic.gdx.Screen"), &[]);
    let game = fx.ty(f, "c.PlayView", &["c.BaseView"], &[]);
    let game_render = fx.func(f, Some(game), "render", false, None, &[]);
    fx.run(&builtins());
    let dispatch: Vec<NodeId> = fx.b.edges().iter().filter(|e| e.kind == EdgeKind::FrameworkDispatch && e.from == render).map(|e| e.to).collect();
    assert!(dispatch.contains(&abs_render));
    assert!(dispatch.contains(&game_render));
}

#[test]
fn name_tag_and_query_rules() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("q.scm"), "(method_declaration name: (identifier) @entry (#match? @entry \"^handle\"))").unwrap();
    let pack = r#"
name = "custom"
version = "1"
[[rule]]
id = "custom.repo"
name_tag = { kinds = ["type"], pattern = ".*Repository$", tag = "layer:repository" }
[[rule]]
id = "custom.handlers"
query = { language = "java", file = "q.scm", capture = "entry", kind = "custom", label = "handler {name}" }
"#;
    let mut list = vec![Pack::parse(pack, "custom/pack.toml", Some(dir.path().to_path_buf())).unwrap()];
    list.insert(0, Pack::parse(include_str!("../../../rules/java-core/pack.toml"), "java-core", None).unwrap());
    let packs = Packs::from_packs(list, &PackSelection::default());
    let mut fx = Fx::new();
    let src = "class OrderRepository {\n  void handleOrder() {}\n  void other() {}\n}\n";
    let f = fx.file("core", "core/OrderRepository.java", Lang::Java, false, &[]);
    fx.sources.insert("core/OrderRepository.java".into(), src.as_bytes().to_vec());
    let repo = fx.ty(f, "OrderRepository", &[], &[]);
    let handle = fx.func(f, Some(repo), "handleOrder", false, None, &[]);
    fx.b.set_span(handle, "core/OrderRepository.java", 2, 2);
    let other = fx.func(f, Some(repo), "other", false, None, &[]);
    fx.b.set_span(other, "core/OrderRepository.java", 3, 3);
    fx.run(&packs);
    assert!(fx.b.has_tag(repo, "layer:repository"));
    let e = fx.entry("handler handleOrder");
    assert_eq!(e.node, handle);
    assert_eq!(e.kind, "custom");
    assert!(fx.b.entries().iter().all(|e| e.node != other));
}

#[test]
fn callbacks_go_to_the_nearest_main() {
    let mut fx = Fx::new();
    let fb = fx.file("bot", "bot/src/b/ToolLauncher.java", Lang::Java, false, &[]);
    let bot_t = fx.ty(fb, "b.ToolLauncher", &[], &[]);
    let bot_main = fx.func(fb, Some(bot_t), "main", true, None, &[]);
    let fbc = fx.file("bot", "bot/src/b/BotApp.java", Lang::Java, false, &[]);
    let bot_app = fx.ty(fbc, "b.BotApp", &["c.ClientApp"], &[]);
    let bot_render = fx.func(fbc, Some(bot_app), "render", false, None, &[]);
    let (_, client_app, client_render) = gdx_app(&mut fx);
    fx.b.add_edge(bot_render, client_app, EdgeKind::Instantiates, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    fx.b.add_edge(bot_main, bot_app, EdgeKind::Instantiates, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    fx.b.add_edge(bot_render, client_render, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    fx.run(&builtins());
    assert_eq!(fx.entry("ClientApp.create()").parent, Some(fx.entry_index("Desktop")));
    assert_eq!(fx.entry("BotApp.render()").parent, Some(fx.entry_index("ToolLauncher")));
}

#[test]
fn routes_in_test_and_mock_code_are_ignored() {
    let mut fx = Fx::new();
    let f = fx.file("api", "api/server_test.go", Lang::Go, true, &["github.com/gofiber/fiber/v2"]);
    let helper = fx.func(f, None, "TestServer", false, None, &[]);
    fx.b.add_flags(helper, NodeFlags::TEST);
    fx.call(
        f,
        CallFacts { caller: helper, method: "Get".into(), receiver_var: Some("app".into()), string_args: str_args(&[Some("/x"), None]), ..Default::default() },
    );
    let g = fx.file("api", "api/mocks/m.go", Lang::Go, false, &["github.com/gofiber/fiber/v2"]);
    let mock = fx.func(g, None, "Mock", false, None, &[]);
    fx.b.add_flags(mock, NodeFlags::MOCK);
    fx.call(
        g,
        CallFacts { caller: mock, method: "Post".into(), receiver_var: Some("app".into()), string_args: str_args(&[Some("/y"), None]), ..Default::default() },
    );
    fx.run(&builtins());
    assert!(fx.b.entries().is_empty(), "{:?}", fx.labels());
}

fn edge_flags(fx: &Fx, from: NodeId, to: NodeId) -> Option<RawEdge> {
    fx.b.edges().iter().find(|e| e.from == from && e.to == to).copied()
}

#[test]
fn wire_files_and_calls_are_wiring() {
    let mut fx = Fx::new();
    let f = fx.file("portal-api", "portal-api/cmd/portal-api/wire_gen.go", Lang::Go, false, &[]);
    fx.sources.insert(f_rel(&fx, f), b"// Code generated by Wire. DO NOT EDIT.\npackage main\n".to_vec());
    let inject = fx.func(f, None, "initializeApp", false, None, &[]);
    let g = fx.file("portal-api", "portal-api/internal/billing/billing.go", Lang::Go, false, &[]);
    let svc = fx.ty(g, "x/billing.checkoutService", &[], &[]);
    let ctor = fx.func(g, Some(svc), "NewCheckoutService", false, None, &[]);
    fx.call(f, CallFacts { caller: inject, method: "NewCheckoutService".into(), target: Some(ctor), ..Default::default() });
    let h = fx.file("portal-api", "portal-api/internal/billing/wire_providers.go", Lang::Go, false, &["github.com/google/wire"]);
    let provider = fx.func(h, None, "ProvideRepository", false, None, &[]);
    let k = fx.file("portal-api", "portal-api/internal/app/app.go", Lang::Go, false, &[]);
    let app = fx.func(k, None, "registerRoutes", false, None, &[]);
    fx.call(k, CallFacts { caller: app, method: "NewSet".into(), receiver_var: Some("wire".into()), target: Some(ctor), ..Default::default() });
    fx.run(&builtins());
    assert!(fx.b.flags(inject).contains(NodeFlags::WIRING));
    assert!(fx.b.flags(provider).contains(NodeFlags::WIRING));
    assert!(!fx.b.flags(ctor).contains(NodeFlags::WIRING));
    assert!(!fx.b.flags(app).contains(NodeFlags::WIRING));
    assert!(edge_flags(&fx, app, ctor).unwrap().flags.contains(EdgeFlags::WIRING), "wire.NewSet call is wiring");
}

fn f_rel(fx: &Fx, f: usize) -> String {
    fx.facts.files[f].rel_path.clone()
}

#[test]
fn dagger_annotations_and_static_builders_are_wiring() {
    let mut fx = Fx::new();
    let f = fx.file("client", "client/src/main/java/c/dagger/AppComponent.java", Lang::Java, false, &["dagger.Component"]);
    let comp = fx.ty(f, "c.dagger.AppComponent", &[], &["Component"]);
    let g = fx.file("client", "client/src/main/java/c/dagger/AppModule.java", Lang::Java, false, &["dagger.Module", "dagger.Provides"]);
    let module = fx.ty(g, "c.dagger.AppModule", &[], &["Module"]);
    let provides = fx.func(g, Some(module), "provideClock", false, None, &[]);
    fx.facts.files[g].functions[0].annotations.push(Annotation { name: "Provides".into(), args: Vec::new() });
    let h = fx.file("client", "client/src/main/java/c/Service.java", Lang::Java, false, &[]);
    let service = fx.ty(h, "c.Service", &[], &["Singleton"]);
    fx.facts.files[h].types[0].annotations.push(Annotation { name: "Inject".into(), args: Vec::new() });
    fx.run(&builtins());
    assert!(fx.b.flags(comp).contains(NodeFlags::WIRING));
    assert!(fx.b.flags(module).contains(NodeFlags::WIRING));
    assert!(fx.b.flags(provides).contains(NodeFlags::WIRING));
    assert!(!fx.b.flags(service).contains(NodeFlags::WIRING), "@Inject classes are real code");
}

#[test]
fn import_only_from_wiring_files_is_wiring() {
    let mut fx = Fx::new();
    let (api, core) = (fx.module("api"), fx.module("core"));
    let main_pkg = fx.b.add_node(api, NodeKind::Package, SubKind::None, "cmd/api", Lang::Go);
    let raw = fx.b.add_node(core, NodeKind::Package, SubKind::None, "rawtext", Lang::Go);
    let shared = fx.b.add_node(core, NodeKind::Package, SubKind::None, "logging", Lang::Go);
    let w = fx.file("api", "api/cmd/api/wire_gen.go", Lang::Go, false, &[]);
    fx.sources.insert(f_rel(&fx, w), b"// Code generated by Wire. DO NOT EDIT.\n".to_vec());
    let m = fx.file("api", "api/cmd/api/main.go", Lang::Go, false, &[]);
    for f in [w, m] {
        fx.facts.files[f].package = Some(main_pkg);
    }
    fx.facts.files[w].import_targets = vec![raw, shared];
    fx.facts.files[m].import_targets = vec![shared];
    for to in [raw, shared] {
        fx.b.add_edge(main_pkg, to, EdgeKind::DependsOn, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    }
    fx.run(&builtins());
    assert!(edge_flags(&fx, main_pkg, raw).unwrap().flags.contains(EdgeFlags::WIRING));
    assert!(!edge_flags(&fx, main_pkg, shared).unwrap().flags.contains(EdgeFlags::WIRING));
}

#[test]
fn test_helpers_by_import_and_dir_are_test() {
    let mut fx = Fx::new();
    let a = fx.file("core", "core/pgsql/testutil/integration/db.go", Lang::Go, false, &[]);
    let in_dir = fx.func(a, None, "OpenDB", false, None, &[]);
    let b = fx.file("core", "core/email/fake.go", Lang::Go, false, &["github.com/stretchr/testify/require"]);
    let by_import = fx.func(b, None, "FakeSender", false, None, &[]);
    let c = fx.file("core", "core/email/send.go", Lang::Go, false, &["net/smtp"]);
    let real = fx.func(c, None, "Send", false, None, &[]);
    let d = fx.file("core", "core/src/main/java/x/Fixtures.java", Lang::Java, false, &["org.junit.jupiter.api.Test"]);
    let t = fx.ty(d, "x.Fixtures", &[], &[]);
    let e = fx.file("core", "core/latest/Version.go", Lang::Go, false, &[]);
    let latest = fx.func(e, None, "Version", false, None, &[]);
    fx.run(&builtins());
    assert!(fx.b.flags(in_dir).contains(NodeFlags::TEST));
    assert!(fx.b.flags(by_import).contains(NodeFlags::TEST));
    assert!(fx.b.flags(t).contains(NodeFlags::TEST));
    assert!(!fx.b.flags(real).contains(NodeFlags::TEST));
    assert!(!fx.b.flags(latest).contains(NodeFlags::TEST), "a dir named latest is not a test dir");
}

#[test]
fn mains_in_test_helpers_are_tests() {
    let mut fx = Fx::new();
    let f = fx.file("tools", "tools/testutil/seed/main.go", Lang::Go, false, &[]);
    fx.func(f, None, "main", true, None, &[]);
    fx.run(&builtins());
    assert_eq!(fx.entry("seed").category, Category::Test);
}

#[test]
fn spring_component_is_not_dagger_wiring() {
    let mut fx = Fx::new();
    let f = fx.file("svc", "svc/src/main/java/s/OrderService.java", Lang::Java, false, &["org.springframework.stereotype.Component"]);
    let spring = fx.ty(f, "s.OrderService", &[], &["Component"]);
    let g = fx.file("client", "client/src/main/java/c/AppComponent.java", Lang::Java, false, &["dagger.Component"]);
    let dagger = fx.ty(g, "c.AppComponent", &[], &["Component"]);
    fx.run(&builtins());
    assert!(!fx.b.flags(spring).contains(NodeFlags::WIRING), "Spring @Component is real code");
    assert!(fx.b.flags(dagger).contains(NodeFlags::WIRING));
}

#[test]
fn wire_import_beside_real_types_keeps_the_file() {
    let mut fx = Fx::new();
    let f = fx.file("svc", "svc/internal/data/data.go", Lang::Go, false, &["github.com/google/wire"]);
    let data = fx.ty(f, "x/data.Data", &[], &[]);
    let query = fx.func(f, Some(data), "Query", false, None, &[]);
    let g = fx.file("svc", "svc/internal/billing/wire_providers.go", Lang::Go, false, &["github.com/google/wire"]);
    let provider = fx.func(g, None, "ProvideRepository", false, None, &[]);
    fx.run(&builtins());
    assert!(!fx.b.flags(data).contains(NodeFlags::WIRING));
    assert!(!fx.b.flags(query).contains(NodeFlags::WIRING));
    assert!(fx.b.flags(provider).contains(NodeFlags::WIRING), "a provider-only file is still wiring");
}

#[test]
fn error_types_are_noise() {
    let mut fx = Fx::new();
    let j = fx.file("core", "core/src/main/java/c/Errors.java", Lang::Java, false, &[]);
    let not_found = fx.ty(j, "c.NotFoundException", &["java.lang.RuntimeException"], &[]);
    let custom = fx.ty(j, "c.Fault", &["c.NotFoundException"], &[]);
    let service = fx.ty(j, "c.Service", &[], &[]);
    let gofile = fx.file("svc", "svc/errors.go", Lang::Go, false, &[]);
    let by_method = fx.ty(gofile, "x.validation", &[], &[]);
    fx.func(gofile, Some(by_method), "Error", false, None, &[]);
    let by_name = fx.ty(gofile, "x.QuotaError", &[], &[]);
    let plain = fx.ty(gofile, "x.Handler", &[], &[]);
    fx.run(&builtins());
    for n in [not_found, custom, by_method, by_name] {
        assert!(fx.b.flags(n).contains(NodeFlags::NOISE), "{} should be noise", fx.b.name(n));
    }
    for n in [service, plain] {
        assert!(!fx.b.flags(n).contains(NodeFlags::NOISE), "{} is real", fx.b.name(n));
    }
    assert!(fx.b.has_tag(not_found, "why:error:name ends with Exception"));
    assert!(fx.b.has_tag(custom, "why:error:extends an error type"));
    assert!(fx.b.has_tag(by_method, "why:error:has an Error() method"));
    assert!(fx.b.has_tag(by_name, "why:error:name ends with Error"));
}

#[test]
fn architectural_roles_are_tagged() {
    let mut fx = Fx::new();
    let f = fx.file("api", "api/internal/billing/billing.go", Lang::Go, false, &["github.com/gofiber/fiber/v2"]);
    let handler = fx.ty(f, "x.AccountHandler", &[], &[]);
    let service = fx.ty(f, "x.checkoutService", &[], &[]);
    let service_impl = fx.ty(f, "x.InvoiceServiceImpl", &[], &[]);
    let repo = fx.ty(f, "x.planRepository", &[], &[]);
    let store = fx.ty(f, "x.planStore", &[], &[]);
    let model = fx.ty(f, "x.InvoiceResponse", &[], &[]);
    let plain = fx.ty(f, "x.Price", &[], &[]);
    let routes = fx.ty(f, "x.webhooks", &[], &[]);
    let hook = fx.func(f, Some(routes), "Receive", false, None, &[]);
    let reg = fx.func(f, None, "Register", false, None, &[]);
    fx.call(
        f,
        CallFacts {
            caller: reg,
            method: "Post".into(),
            receiver_var: Some("app".into()),
            string_args: str_args(&[Some("/hook"), None]),
            arg_targets: vec![None, Some(hook)],
            ..Default::default()
        },
    );
    let j = fx.file("auth", "auth/src/main/java/a/Users.java", Lang::Java, false, &[]);
    let rest = fx.ty(j, "a.Users", &[], &["RestController"]);
    let svc = fx.ty(j, "a.Accounts", &[], &["Service"]);
    let api = fx.module("api");
    let pkg = fx.b.add_node(api, NodeKind::Package, SubKind::None, "internal/repository", Lang::Go);
    fx.run(&builtins());
    let has = |n: NodeId, t: &str| fx.b.has_tag(n, t);
    assert!(has(handler, "layer:handler") && has(rest, "layer:handler") && has(routes, "layer:handler"));
    assert!(has(service, "layer:service") && has(service_impl, "layer:service") && has(svc, "layer:service"));
    assert!(has(repo, "layer:repository") && has(store, "layer:repository") && has(pkg, "layer:repository"));
    assert!(has(model, "layer:model"));
    for t in ["layer:handler", "layer:service", "layer:repository", "layer:model"] {
        assert!(!has(plain, t), "Price has no role");
    }
}

fn member(fx: &mut Fx, file: usize, owner: NodeId, name: &str, sub: SubKind) -> NodeId {
    let n = fx.b.add_node(owner, NodeKind::Member, sub, name, fx.facts.files[file].lang);
    fx.facts.files[file].functions.push(FnFacts {
        node: n,
        owner: Some(owner),
        name: name.into(),
        params: Vec::new(),
        annotations: Vec::new(),
        is_main: false,
        overrides_external: None,
    });
    n
}

fn ty_sub(fx: &mut Fx, file: usize, fqn: &str, sub: SubKind) -> NodeId {
    let t = fx.ty(file, fqn, &[], &[]);
    let pkg = fx.b.parent(t).unwrap();
    let name = fqn.rsplit('.').next().unwrap();
    let node = fx.b.add_node(pkg, NodeKind::Type, sub, name, fx.facts.files[file].lang);
    fx.facts.files[file].types.last_mut().unwrap().node = node;
    node
}

#[test]
fn significance_keeps_components_and_sets_details_aside() {
    let mut fx = Fx::new();
    let f = fx.file("api", "api/internal/billing/billing.go", Lang::Go, false, &[]);
    let handler = ty_sub(&mut fx, f, "x.CheckoutHandler", SubKind::Struct);
    member(&mut fx, f, handler, "Create", SubKind::Method);
    let iface = ty_sub(&mut fx, f, "x.Notifier", SubKind::Interface);
    let dto = ty_sub(&mut fx, f, "x.Price", SubKind::Struct);
    member(&mut fx, f, dto, "NewPrice", SubKind::Constructor);
    let response = ty_sub(&mut fx, f, "x.InvoiceResponse", SubKind::Struct);
    member(&mut fx, f, response, "Validate", SubKind::Method);
    let local = ty_sub(&mut fx, f, "x.ratio", SubKind::Struct);
    let used = member(&mut fx, f, local, "apply", SubKind::Method);
    let hub = ty_sub(&mut fx, f, "x.Clock", SubKind::Struct);
    let now = member(&mut fx, f, hub, "Now", SubKind::Method);
    let built = ty_sub(&mut fx, f, "x.mailer", SubKind::Struct);
    let send = member(&mut fx, f, built, "Send", SubKind::Method);
    let ctor = member(&mut fx, f, built, "NewMailer", SubKind::Constructor);
    let w = fx.file("api", "api/cmd/api/wire_gen.go", Lang::Go, false, &[]);
    fx.sources.insert(f_rel(&fx, w), b"// Code generated by Wire. DO NOT EDIT.\n".to_vec());
    let inject = fx.func(w, None, "initializeApp", false, None, &[]);
    fx.call(w, CallFacts { caller: inject, method: "NewMailer".into(), target: Some(ctor), ..Default::default() });
    for (i, pkg) in ["a", "b"].iter().enumerate() {
        let o = fx.file("api", &format!("api/internal/{pkg}/x.go"), Lang::Go, false, &[]);
        let user = fx.ty(o, &format!("{pkg}.User{i}"), &[], &[]);
        let call = fx.func(o, Some(user), "run", false, None, &[]);
        fx.b.add_edge(call, now, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    }
    let _ = (used, send);
    let j = fx.file("core", "core/src/main/java/c/Model.java", Lang::Java, false, &[]);
    let record = ty_sub(&mut fx, j, "c.Amount", SubKind::Record);
    let kind = ty_sub(&mut fx, j, "c.Kind", SubKind::Enum);
    let bean = ty_sub(&mut fx, j, "c.Customer", SubKind::Class);
    for m in ["getName", "setName", "isActive", "toString", "<init>"] {
        member(&mut fx, j, bean, m, SubKind::Method);
    }
    let comp = ty_sub(&mut fx, j, "c.Billing", SubKind::Class);
    member(&mut fx, j, comp, "charge", SubKind::Method);
    fx.facts.files[j].types.last_mut().unwrap().annotations.push(Annotation { name: "Component".into(), args: Vec::new() });
    let injected = ty_sub(&mut fx, j, "c.Sync", SubKind::Class);
    member(&mut fx, j, injected, "run", SubKind::Method);
    let init = member(&mut fx, j, injected, "<init>", SubKind::Method);
    fx.facts.files[j].functions.iter_mut().find(|f| f.node == init).unwrap().annotations.push(Annotation { name: "Inject".into(), args: Vec::new() });
    let nested = fx.b.add_node(comp, NodeKind::Type, SubKind::Class, "Cache", Lang::Java);
    fx.facts.files[j].types.push(TypeFacts { node: nested, fqn: "c.Billing.Cache".into(), supertypes: Vec::new(), annotations: Vec::new() });
    fx.run(&builtins());
    let detail = |n: NodeId| fx.b.flags(n).contains(NodeFlags::DETAIL);
    for (n, why) in [
        (handler, "role"),
        (iface, "interface"),
        (hub, "used from two packages"),
        (built, "assembled by DI"),
        (comp, "@Component"),
        (injected, "@Inject constructor"),
    ] {
        assert!(!detail(n), "{} is a component ({why})", fx.b.name(n));
    }
    for (n, why) in [
        (dto, "no methods"),
        (response, "a model"),
        (local, "only used locally"),
        (record, "record"),
        (kind, "enum"),
        (bean, "accessors only"),
        (nested, "nested"),
    ] {
        assert!(detail(n), "{} is a detail ({why})", fx.b.name(n));
    }
    for (n, why) in [
        (dto, "has no methods"),
        (response, "a model (DTO, request, response…)"),
        (local, "used from fewer than 2 other packages"),
        (record, "a record, enum or annotation"),
        (bean, "only getters and setters"),
        (nested, "nested in another type"),
    ] {
        assert!(fx.b.has_tag(n, &format!("why:detail:{why}")), "{} should say why: {why}", fx.b.name(n));
    }
    let bean_rows: Vec<(String, bool)> = fx.b.children(bean).iter().map(|&m| (fx.b.name(m).to_string(), fx.b.has_tag(m, "accessor"))).collect();
    assert_eq!(bean_rows, [("getName".into(), true), ("setName".into(), true), ("isActive".into(), true), ("toString".into(), true), ("<init>".into(), false)]);
    let charge = fx.b.children(comp).iter().copied().find(|&m| fx.b.name(m) == "charge").unwrap();
    assert!(!fx.b.has_tag(charge, "accessor"));
}

#[test]
fn package_helper_functions_are_details_unless_shared() {
    let mut fx = Fx::new();
    let f = fx.file("api", "api/internal/billing/util.go", Lang::Go, false, &[]);
    let helper = fx.func(f, None, "derefStr", false, None, &[]);
    let shared = fx.func(f, None, "FormatMoney", false, None, &[]);
    let main = fx.func(f, None, "main", true, None, &[]);
    for (i, pkg) in ["a", "b"].iter().enumerate() {
        let o = fx.file("api", &format!("api/internal/{pkg}/x.go"), Lang::Go, false, &[]);
        let user = fx.ty(o, &format!("{pkg}.U{i}"), &[], &[]);
        let call = fx.func(o, Some(user), "run", false, None, &[]);
        fx.b.add_edge(call, shared, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    }
    fx.run(&builtins());
    assert!(fx.b.flags(helper).contains(NodeFlags::DETAIL));
    assert!(!fx.b.flags(shared).contains(NodeFlags::DETAIL), "used from two packages");
    assert!(!fx.b.flags(main).contains(NodeFlags::DETAIL), "entry point");
}

#[test]
fn classes_other_classes_extend_are_components() {
    let mut fx = Fx::new();
    let j = fx.file("server", "server/src/main/java/s/ai/MoveTask.java", Lang::Java, false, &[]);
    let base = ty_sub(&mut fx, j, "s.ai.MoveTask", SubKind::Class);
    let lone = ty_sub(&mut fx, j, "s.ai.Helper", SubKind::Class);
    member(&mut fx, j, lone, "help", SubKind::Method);
    let child = ty_sub(&mut fx, j, "s.ai.RangedMoveTask", SubKind::Class);
    member(&mut fx, j, child, "run", SubKind::Method);
    fx.b.add_edge(child, base, EdgeKind::Inherits, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    fx.run(&builtins());
    let detail = |n: NodeId| fx.b.flags(n).contains(NodeFlags::DETAIL);
    assert!(!detail(base), "a base class with no methods of its own is still part of the design");
    assert!(detail(lone), "an unshared class nobody extends stays a detail");
}
