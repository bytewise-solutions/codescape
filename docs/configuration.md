# Configuring Codescape

Everything here is optional: Codescape works on a project without any file of its own.

## Project settings

Settings come from four layers; a later one overrides an earlier one:

1. built-in defaults (tests, test deps, details, errors and wiring hidden; build output, `vendor`, dot folders left out),
2. your defaults for every project: `~/.config/codescape/settings.toml`,
3. the project's `codescape.toml` (or `--config FILE`), shared with the team,
4. your settings for this project: `~/.config/codescape/projects/<hash>.toml`, written by the viewer.

All four files take the same keys:

```toml
name = "Sample"
exclude = ["**/generated/**"]
index = "/somewhere/else.codescape"

[packs]
disable = ["libgdx"]
dirs = ["../my-rule-packs"]

[test]
paths = ["**/it/**"]           # also test code, on top of the packs' test detection

[[category]]
glob = "**/cmd/migrate/**"
category = "dev-tool"          # application, dev-tool or test

[[filter]]
name = "migrations"
paths = ["**/migrations/**"]   # globs on the file path
names = ["Migration$"]         # regexes on type and function names
shown = false                  # default for View ▸ migrations

[view]
details = true                 # tests, test_deps, details, errors, wiring, legend
filters = { migrations = true }

[remove]                       # drop entries inherited from an earlier layer
exclude = ["**/vendor/**"]
category = []
test_paths = []
filter = []
```

Single values take the latest layer that sets them. Lists add up across layers; a filter or category with the
same name or glob replaces the earlier one, and `[remove]` drops one. A file or function matched by any path or
name of a filter is tagged `filter:<name>`, with its members and any package or module holding nothing else, and
hidden while the filter is unticked in **View**. Relative paths resolve against the folder of the file that
names them (the project folder for the two files under `~/.config`).

**File ▸ Project Settings…** shows every value with the layer it comes from. Display values apply at once and land
in your file for this project; index values (left out, test code, packs, categories, filters) wait for **Apply
and re-index**. For a value of yours, *reset* falls back to the layer below, *use for all projects* moves it to
your defaults and *save to codescape.toml* moves it to the shared file, keeping that file's comments. The viewer
writes nothing into the project unless you press that last button.

## Rule packs

Framework knowledge (which methods a framework calls back, what registers an HTTP route, how things are coloured)
lives in **rule packs**, not in Codescape's code. The built-in packs in `rules/` use exactly the format you would
write yourself: `java-core`, `libgdx`, `spring-boot`, `dagger`, `go-core`, `fiber`, `rabbitmq-amqp091`,
`google-wire`, `ts-core`, `angular`, `express`, `rust-core`, `layers`, `significance`, `default-visuals`.

Packs load in this order, later ones overriding earlier rules with the same `id`:
built-in → `~/.config/codescape/rules/<pack>/` → `<project>/.codescape/rules/<pack>/` → `[packs] dirs`.

A pack is a folder with a `pack.toml` (and optional tree-sitter `.scm` query files):

```toml
name = "my-pack"
version = "0.1.0"
languages = ["java"]          # empty = all languages

[[rule]]
id = "my-pack.repositories"
name_tag = { kinds = ["type"], pattern = ".*Repository$", tag = "layer:repository" }

[[rule]]
id = "my-pack.handlers"
query = { language = "java", file = "handlers.scm", capture = "entry", kind = "custom", label = "{name}" }

[visual]
[[visual.tag_style]]
tag = "layer:repository"
color = [120, 200, 255]
```

Each rule has exactly one matcher:

| Matcher | Produces |
|---|---|
| `main = { category, label }` | an entry point for every language-level `main`; `label` may use `{module}`, `{package}`, `{name}` |
| `main_call = { methods, file_imports, label, handoff, category }` | makes the function that issues one of these calls the main (e.g. Angular's `bootstrapApplication`) |
| `object_route = { var_types, calls, path_key, target_keys, children_key, kind, label }` | routes declared as object literals (e.g. Angular `Routes`), nested `children` joining their paths |
| `path = { glob, category }` | the category of mains under matching paths |
| `supertype = { types, callbacks = [{ method, lifecycle, order }] }` | framework callbacks on subclasses, nested under the main that reaches them |
| `annotation = { names, handoff }` | marks a main as handing off to a framework (e.g. Spring Boot) |
| `annotation_route = { names, method, prefix_from_type }` | HTTP routes from annotated methods |
| `call_route = { methods, group_methods, receiver_types, file_imports, path_arg, handler_arg, label }` | HTTP routes from registration calls, joining group prefixes across functions |
| `call_handler = { methods, receiver_types, file_imports, kind, label }` | entry points such as message consumers |
| `instantiates = { types, handoff }` | marks the main that creates a framework object |
| `dispatch = { from_override, to_override }` | an implied call the framework makes (e.g. `Game.render` → `Screen.render`) |
| `name_tag = { kinds, pattern, tag }` | tags on modules, packages, types or members |
| `query = { language, file, capture, kind, label }` | entry points from a tree-sitter query |
| `wiring = { file_markers, file_imports, calls, annotations, type_prefixes, requires_imports, caller_prefixes }` | marks dependency-injection code as wiring |
| `test_code = { file_imports, dir_names, paths }` | marks test helpers as test code |

Lifecycles: `once`, `repeating`, `on-event`, `later`, `unknown`. Visual rules (`tag_style`, `frame` with
`auto_prefix` or `pattern`, `hide_tags`, `fold_tags`, `entry_group`) apply without re-indexing; detection rules
trigger a new index automatically.

Without a pack, a method that overrides a method of an external type, on a type a main creates, is still listed as
a callback with lifecycle `unknown`.
