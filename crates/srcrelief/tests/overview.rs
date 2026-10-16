use srcrelief::overview::*;
use srcrelief_format::*;

struct Fx {
    _dir: tempfile::TempDir,
    file: ReliefFile,
}

struct B {
    b: GraphBuilder,
}

impl B {
    fn node(&mut self, parent: NodeId, kind: NodeKind, sub: SubKind, name: &str) -> NodeId {
        let n = self.b.add_node(parent, kind, sub, name, Lang::Go);
        self.b.set_span(n, "f.go", 1, 10);
        n
    }

    fn calls(&mut self, a: NodeId, c: NodeId) {
        self.b.add_edge(a, c, EdgeKind::Calls, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    }

    fn entry(&mut self, node: NodeId, kind: &str, label: &str, parent: Option<u32>) -> u32 {
        self.b.add_entry(EntryPointSpec { node, kind: kind.into(), category: Category::Application, label: label.into(), parent, lifecycle: Lifecycle::OnEvent, order: 0, rule: "r".into(), handoff: None })
    }
}

fn fixture() -> Fx {
    let mut x = B { b: GraphBuilder::new("p") };
    let root = x.b.root();
    let ee = x.node(root, NodeKind::Module, SubKind::None, "job-engine");
    let cmd = x.node(ee, NodeKind::Package, SubKind::None, "cmd");
    let main = x.node(cmd, NodeKind::Member, SubKind::Function, "main");
    let pkg = x.node(ee, NodeKind::Package, SubKind::None, "events");
    let producer = x.node(pkg, NodeKind::Type, SubKind::Struct, "jobEventProducer");
    let publish = x.node(producer, NodeKind::Member, SubKind::Method, "Publish");
    x.b.add_tag(producer, "channel:out:jobevent");
    let job = x.node(pkg, NodeKind::Type, SubKind::Struct, "jobConsumer");
    let job_run = x.node(job, NodeKind::Member, SubKind::Method, "consume");
    let disp = x.node(pkg, NodeKind::Type, SubKind::Struct, "jobEventDispatcherConsumer");
    let disp_run = x.node(disp, NodeKind::Member, SubKind::Method, "consume");
    x.b.add_tag(disp, "channel:in:jobdispatcher");
    let iface = x.node(pkg, NodeKind::Type, SubKind::Interface, "EventPublisher");
    let iface_publish = x.node(iface, NodeKind::Member, SubKind::Method, "Send");
    let gcp = x.node(pkg, NodeKind::Type, SubKind::Struct, "CloudPublisherService");
    let gcp_publish = x.node(gcp, NodeKind::Member, SubKind::Method, "Send");
    x.b.add_tag(gcp, "external:GCP Pub/Sub");
    x.b.add_tag(gcp_publish, "external:GCP Pub/Sub");
    x.b.add_edge(gcp, iface, EdgeKind::Implements, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    x.calls(job_run, publish);
    let audit = x.node(pkg, NodeKind::Type, SubKind::Struct, "auditProducer");
    let audit_send = x.node(audit, NodeKind::Member, SubKind::Method, "Send");
    x.b.add_tag(audit, "channel:out:audit");
    x.calls(publish, audit_send);
    let ctor = x.node(job, NodeKind::Member, SubKind::Constructor, "NewJobConsumer");
    let unrelated = x.node(pkg, NodeKind::Type, SubKind::Struct, "metricsProducer");
    let metrics_send = x.node(unrelated, NodeKind::Member, SubKind::Method, "Send");
    x.b.add_tag(unrelated, "channel:out:metrics");
    x.calls(ctor, metrics_send);
    x.calls(disp_run, iface_publish);
    let _ = gcp_publish;
    let m = x.entry(main, "main", "job-engine", None);
    x.entry(job_run, "message-handler", "consume in jobConsumer.consume", Some(m));
    x.entry(disp_run, "message-handler", "consume in jobEventDispatcherConsumer.consume", Some(m));

    let pa = x.node(root, NodeKind::Module, SubKind::None, "portal-api");
    let pcmd = x.node(pa, NodeKind::Package, SubKind::None, "cmd");
    let pmain = x.node(pcmd, NodeKind::Member, SubKind::Function, "main");
    let api = x.node(pa, NodeKind::Package, SubKind::None, "api");
    let users = x.node(api, NodeKind::Type, SubKind::Struct, "UserHandler");
    let list = x.node(users, NodeKind::Member, SubKind::Method, "List");
    let get = x.node(users, NodeKind::Member, SubKind::Method, "Get");
    let db = x.node(api, NodeKind::Member, SubKind::Function, "query");
    x.b.add_tag(db, "external:PostgreSQL");
    x.calls(list, db);
    let tool = x.node(pa, NodeKind::Package, SubKind::None, "tool");
    let tmain = x.node(tool, NodeKind::Member, SubKind::Function, "main");
    x.entry(tmain, "main", "a-tool", None);
    let pm = x.entry(pmain, "main", "portal-api", None);
    x.entry(list, "http-route", "GET /users", Some(pm));
    x.entry(get, "http-route", "GET /users/:id", Some(pm));

    let dir = tempfile::tempdir().unwrap();
    write_graph(&x.b.finish(), &dir.path().join("g")).unwrap();
    let file = ReliefFile::open(&dir.path().join("g")).unwrap();
    Fx { _dir: dir, file }
}

fn has(o: &Overview, from: &str, to: &str) -> Option<bool> {
    o.links.iter().find(|l| o.label(l.from) == from && o.label(l.to) == to).map(|l| l.inferred)
}

#[test]
fn applications_hold_their_entry_components() {
    let fx = fixture();
    let o = build(fx.file.graph());
    let apps: Vec<&str> = o.apps.iter().map(|a| a.label.as_str()).collect();
    assert_eq!(apps, ["job-engine", "portal-api", "a-tool"], "apps without components go last");
    let comps: Vec<&str> = o.apps[0].components.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(comps, ["jobEventDispatcherConsumer", "jobConsumer"]);
    let api: Vec<(&str, &str)> = o.apps[1].components.iter().map(|c| (c.label.as_str(), c.detail.as_str())).collect();
    assert_eq!(api, [("HTTP API", "2 routes")]);
}

#[test]
fn channels_link_publishers_to_consumers_by_name() {
    let fx = fixture();
    let o = build(fx.file.graph());
    let channels: Vec<&str> = o.channels.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(channels, ["audit", "email_event"]);
    assert_eq!(has(&o, "jobConsumer", "email_event"), Some(false), "publishing is read from code");
    assert_eq!(has(&o, "email_event", "jobEventDispatcherConsumer"), Some(true), "consuming is inferred from names");
}

#[test]
fn external_systems_are_reached_through_interfaces() {
    let fx = fixture();
    let o = build(fx.file.graph());
    assert_eq!(o.externals, ["GCP Pub/Sub", "PostgreSQL"]);
    assert_eq!(has(&o, "jobEventDispatcherConsumer", "GCP Pub/Sub"), Some(false));
    assert_eq!(has(&o, "HTTP API", "PostgreSQL"), Some(false));
    assert_eq!(has(&o, "jobConsumer", "GCP Pub/Sub"), None);
    assert_eq!(has(&o, "HTTP clients", "HTTP API"), Some(false));
}

#[test]
fn layout_stacks_clients_apps_channels_and_systems() {
    let fx = fixture();
    let o = build(fx.file.graph());
    let p = layout(&o, 700.0, 500.0);
    let top = |e: End| p.rect(e).unwrap().y;
    assert!(top(End::Clients) < top(End::App(0)));
    assert!(top(End::App(0)) < top(End::Channel(0)));
    assert!(top(End::Channel(0)) < top(End::External(0)));
    let comps: Vec<_> = p.boxes.iter().filter(|(e, _)| matches!(e, End::Component(..))).collect();
    for (i, (_, a)) in comps.iter().enumerate() {
        for (_, b) in &comps[i + 1..] {
            assert!(a.right() <= b.x || b.right() <= a.x || a.bottom() <= b.y || b.bottom() <= a.y, "components overlap");
        }
    }
    let (a0, a1, tool) = (p.rect(End::App(0)).unwrap(), p.rect(End::App(1)).unwrap(), p.rect(End::App(2)).unwrap());
    assert!(a0.y == a1.y && a1.y == tool.y, "apps share one row");
    assert!(tool.h < a0.h);
    let app = p.rect(End::App(0)).unwrap();
    let c = p.rect(End::Component(0, 0)).unwrap();
    assert!(c.x >= app.x && c.right() <= app.right() && c.y >= app.y && c.bottom() <= app.bottom());
}

#[test]
fn reach_goes_through_publishers_but_not_constructors() {
    let fx = fixture();
    let o = build(fx.file.graph());
    assert_eq!(has(&o, "jobConsumer", "audit"), Some(false));
    assert_eq!(has(&o, "jobConsumer", "metrics"), None, "constructors only set things up");
}

#[test]
fn spare_height_goes_between_the_bands() {
    let fx = fixture();
    let o = build(fx.file.graph());
    let (short, tall) = (layout(&o, 1200.0, 300.0), layout(&o, 1200.0, 1400.0));
    let gap = |p: &Placed| p.rect(End::Channel(0)).unwrap().y - p.rect(End::App(0)).unwrap().bottom();
    assert!(gap(&tall) > gap(&short));
    assert!(tall.size[1] <= 1400.0);
}

#[test]
fn dispatch_stays_inside_the_modules_an_app_can_see() {
    let mut x = B { b: GraphBuilder::new("p") };
    let root = x.b.root();
    let core = x.node(root, NodeKind::Module, SubKind::None, "core");
    let cp = x.node(core, NodeKind::Package, SubKind::None, "net");
    let store = x.node(cp, NodeKind::Type, SubKind::Interface, "Store");
    let save = x.node(store, NodeKind::Member, SubKind::Method, "save");
    let server = x.node(root, NodeKind::Module, SubKind::None, "server");
    let sp = x.node(server, NodeKind::Package, SubKind::None, "db");
    let mongo = x.node(sp, NodeKind::Type, SubKind::Class, "MongoStore");
    let msave = x.node(mongo, NodeKind::Member, SubKind::Method, "save");
    x.b.add_tag(msave, "external:MongoDB");
    x.b.add_edge(mongo, store, EdgeKind::Implements, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    let smain = x.node(sp, NodeKind::Member, SubKind::Function, "main");
    let client = x.node(root, NodeKind::Module, SubKind::None, "client");
    let kp = x.node(client, NodeKind::Package, SubKind::None, "app");
    let game = x.node(kp, NodeKind::Type, SubKind::Class, "ClientApp");
    let render = x.node(game, NodeKind::Member, SubKind::Method, "render");
    let main = x.node(kp, NodeKind::Member, SubKind::Function, "main");
    x.calls(render, save);
    x.calls(smain, save);
    for (a, b) in [(client, core), (server, core)] {
        x.b.add_edge(a, b, EdgeKind::DependsOn, Scope::Main, Confidence::Resolved, EdgeFlags::NONE);
    }
    let m = x.entry(main, "main", "client", None);
    x.entry(render, "callback", "render", Some(m));
    x.entry(smain, "main", "server", None);
    let dir = tempfile::tempdir().unwrap();
    write_graph(&x.b.finish(), &dir.path().join("g")).unwrap();
    let file = ReliefFile::open(&dir.path().join("g")).unwrap();
    let o = build(file.graph());
    assert_eq!(has(&o, "ClientApp", "MongoDB"), None, "client cannot load the server's store");
    assert_eq!(has(&o, "server", "MongoDB"), Some(false));
}
