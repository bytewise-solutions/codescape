use codescape_format::SubKind;
use codescape_go::parse::*;

fn func<'a>(f: &'a GoFile, name: &str) -> &'a GoFunc {
    f.funcs.iter().find(|x| x.name == name).unwrap_or_else(|| panic!("no func {name}"))
}

#[test]
fn parses_funcs_methods_types() {
    let src = r#"package handler

import (
	"fmt"
	fiber "github.com/gofiber/fiber/v2"
	"example.com/mono/shared"
)

type Handler struct {
	svc  *Service
	repo, cache Repo
	shared.Base
}

type Repo interface {
	Get(id string) (*Item, error)
	List() []Item
}

type ID string

func main() {}

func (h *Handler) List(c *fiber.Ctx) error {
	return nil
}

func NewHandler(s *Service) *Handler { return &Handler{svc: s} }
"#;
    let f = parse_go(src.as_bytes());
    assert_eq!(f.package, "handler");
    assert_eq!(f.imports.len(), 3);
    assert_eq!(f.imports[1].alias.as_deref(), Some("fiber"));
    assert_eq!(f.imports[1].path, "github.com/gofiber/fiber/v2");
    let h = f.types.iter().find(|t| t.name == "Handler").unwrap();
    assert_eq!(h.kind, SubKind::Struct);
    assert!(h.fields.contains(&("svc".into(), "Service".into())));
    assert!(h.fields.contains(&("cache".into(), "Repo".into())));
    assert_eq!(h.embeds, ["shared.Base"]);
    let repo = f.types.iter().find(|t| t.name == "Repo").unwrap();
    assert_eq!(repo.kind, SubKind::Interface);
    assert_eq!(repo.methods, ["Get", "List"]);
    assert_eq!(f.types.iter().find(|t| t.name == "ID").unwrap().kind, SubKind::NamedType);
    let list = func(&f, "List");
    assert_eq!(list.receiver.as_deref(), Some("Handler"));
    assert_eq!(list.params, [("c".to_string(), "fiber.Ctx".to_string())]);
    assert_eq!(func(&f, "NewHandler").results, ["Handler"]);
    assert!(func(&f, "main").receiver.is_none());
}

#[test]
fn captures_calls_assignments_and_args() {
    let src = r#"package p

func (h *H) RegisterRoutes(apiGroup fiber.Router) {
	group := apiGroup.Group("/statistics")
	group.Get("/rates", h.GetRates)
	msgs, err := channel.Consume(q, "")
	svc := &Service{repo: r}
	pkg.Func()
	h.svc.Do()
	local()
	go func() { inner.Run() }()
	_ = err
	_ = msgs
	_ = svc
}
"#;
    let f = parse_go(src.as_bytes());
    let calls = &func(&f, "RegisterRoutes").calls;
    let by = |m: &str| calls.iter().find(|c| c.method == m).unwrap_or_else(|| panic!("no call {m}"));
    let g = by("Group");
    assert_eq!(g.receiver.as_deref(), Some("apiGroup"));
    assert_eq!(g.assigned_to.as_deref(), Some("group"));
    assert_eq!(g.string_args, [Some("/statistics".to_string())]);
    let get = by("Get");
    assert_eq!(get.string_args[0].as_deref(), Some("/rates"));
    assert_eq!(get.ident_args[1].as_deref(), Some("h.GetRates"));
    let consume = by("Consume");
    assert_eq!(consume.assigned_to.as_deref(), Some("msgs"));
    assert_eq!(consume.string_args, [None, Some(String::new())]);
    assert!(calls.iter().any(|c| c.composite.as_deref() == Some("Service")));
    assert_eq!(by("Func").receiver.as_deref(), Some("pkg"));
    assert_eq!(by("Do").receiver.as_deref(), Some("h.svc"));
    assert_eq!(by("local").receiver, None);
    assert_eq!(by("Run").receiver.as_deref(), Some("inner"));
}

#[test]
fn locals_typed_from_var_and_params() {
    let src = r#"package p

func f(r fiber.Router, n int, opts ...Option) {
	var x Foo
	var y, z = 1, 2
	s := &Service{}
	t := NewThing()
	u := pkg.Make(1)
	for _, it := range items {}
	_ = y
}
"#;
    let f = parse_go(src.as_bytes());
    let fun = func(&f, "f");
    assert_eq!(fun.params, [("r".to_string(), "fiber.Router".to_string()), ("n".into(), "int".into()), ("opts".into(), "Option".into())]);
    for local in [("x", "Foo"), ("s", "Service"), ("t", "()NewThing"), ("u", "()pkg.Make")] {
        assert!(fun.locals.contains(&(local.0.to_string(), local.1.to_string())), "missing {local:?} in {:?}", fun.locals);
    }
}

#[test]
fn syntax_error_still_yields_funcs() {
    let src = "package p\n\nfunc good() { a.B() }\n\nfunc bad( { x(\n\nfunc alsoGood() {}\n";
    let f = parse_go(src.as_bytes());
    assert!(f.funcs.iter().any(|x| x.name == "good"));
}

#[test]
fn an_else_if_chain_does_not_count_as_nesting() {
    let src = "package c\n\nfunc F(x int) int {\n\tif x == 1 {\n\t\treturn 1\n\t} else if x == 2 {\n\t\treturn 2\n\t} else if x == 3 {\n\t\treturn 3\n\t}\n\treturn 0\n}\n";
    let f = parse_go(src.as_bytes());
    let c = &func(&f, "F").complexity;
    assert_eq!((c.total, c.nesting), (4, 1));
}

#[test]
fn mocks_are_told_by_folder_file_and_type_name() {
    use codescape_go::{is_mock_dir, is_mock_type};
    assert!(is_mock_dir("internal/repo/mocks") && is_mock_dir("core/rawemail/rawemailmock") && is_mock_dir("mock"));
    assert!(!is_mock_dir("internal/mockingbird") && !is_mock_dir("internal/repo"));
    assert!(is_mock_type("MockRawEmailStore") && is_mock_type("MockX"));
    assert!(!is_mock_type("Mockingbird") && !is_mock_type("Mock") && !is_mock_type("mockStore"));
}
