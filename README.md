# SrcRelief

See a whole codebase as one zoomable map: entry points first, modules laid out top-down by dependency, and
packages, types and members appearing as you zoom in. Arrows that point upward (a lower layer depending on a
higher one) are drawn red and counted, so you can tell at a glance whether a project is layered cleanly.

This is the proof of concept described in `docs/superpowers/specs/2026-10-16-srcrelief-design.md`. It reads
**Java (Gradle)** and **Go (go.mod / go.work)** projects.

## Run it

```sh
cargo run --release -p srcrelief -- ~/path/to/project      # open a project folder
cargo run --release -p srcrelief                           # start screen with "Open folder…" and recent projects
```

Point it at the project folder. Every module under it is discovered and linked by itself:

- **Gradle**: modules from `settings.gradle(.kts)`, links from `project(':x')` dependencies (test-scope links are
  kept apart and hidden by default, toggle *test deps* in the status bar).
- **Go**: modules from `go.work`, or every `go.mod` under the folder; a module depends on another when its
  `require` names it.
- Other build roots found inside (`package.json`, `pom.xml`, …) are listed in the status bar as not indexed.

Nothing is written into the project. The index lives in the user cache folder
(`~/.cache/srcrelief/<hash>.relief` on Linux) and is rebuilt when sources or detection rules change.

### Navigating

| Action | How |
|---|---|
| Pan | drag, or two-finger swipe on a trackpad |
| Scroll | mouse wheel (vertical), `Shift` + wheel (horizontal) |
| Zoom | `Ctrl` + wheel or pinch, around the pointer |
| Zoom into a box | double-click it |
| Inspect a box | click it (right panel) |
| Focus an entry point or a box | click it: the view frames it, what it calls turns green, what calls or sets it up turns blue, the rest dims |
| Walk the code | click a row under *Set up / called by* or *Calls* in the inspector; `Alt+Left` or **Back** retraces |
| See everything involved | inspector **Show**: *neighbours*, *everything it reaches*, *everywhere it's used* |
| Jump to a name | `Ctrl+K`, type, `Enter` |
| Go back up | click a breadcrumb segment, or the minimap |

A container opens to show its children once it is big enough; arrows inside appear once the children are large
enough to read.

### What you see

- **Entry points on top.** Inside every container, the children that hold an application entry point sit on the
  top row, alphabetically, so a project looks the same from one day to the next. Everything else layers below by
  dependency; red arrows point upward (a layering violation), amber ones join two boxes of the same layer.
- **Box colours by kind:** interface (teal), class/struct (blue), enum/record (violet), function/method (grey),
  constructor (dim blue). A legend sits above the minimap.
- **Implements / inherits** arrows are dotted with a hollow triangle at the interface, like UML.
- **Constructors live inside their type.** A Go `func NewX(...) *X` is shown inside `X`, not as a box of its own.
- **Dependency injection is set aside.** Code recognised as wiring (Google Wire's generated injectors and provider
  sets, Dagger components/modules) is hidden with its arrows, so it never changes the layering. Tick **wiring** in
  the status bar to show it muted, with faint dotted arrows. Highlighting an entry point still follows wiring.
- **Architecture first.** Only architectural types are drawn by default: handlers, services, repositories,
  interfaces, entry-point owners, what the DI framework assembles (Wire/Dagger, Spring `@Component`/`@Service`,
  `@Inject` constructors) and types used from two or more other packages. Data types (structs without methods,
  records, enums, getter/setter classes, `*Request`/`*Response`/models), nested types and helpers in
  `util`/`common` packages are *details*: an open box shows `+ N details`; click it or tick **details** to show them.
  The `significance` pack holds these rules.
- **Implementations live inside their interface.** At first glance you see the interface; open it to see the
  structs or classes that implement it.
- **Error and exception types are set aside** (Java `*Exception` and subclasses of Exception/Throwable/Error, Go
  types with an `Error()` method or named `*Error`); tick **errors** to show them in their own row.
- **Test code is out:** test sources, test helpers (files importing `testing`/testify or JUnit/Mockito, folders such
  as `testutil`/`testdata`) and test mains are not drawn or listed. Tick **test deps** to see test-only dependencies.

### Overview (experimental)

**Overview** in the top bar swaps the map for a runtime picture: each application with its components (one per type
that owns message handlers or framework callbacks, plus one *HTTP API* box for its routes), the channels between them
and the external systems they use. Solid lines are read from the code: a component publishes to a channel or uses a
system when its code reaches the publishing call or a file that imports the system's client library. Dashed lines are
inferred from names: a consumer is linked to the channel whose publisher name it shares (`jobEventProducer` →
`email_event` → `jobEventConsumer`). Click a component to open it in the map. Channels and systems come from the
`rabbitmq-amqp091` and `externals` packs.

### Index from the command line

```sh
cargo run --release -p srcrelief-index -- ~/path/to/project [--config FILE] [--out FILE] [--force]
```

Prints modules, their links, entry points grouped as in the viewer, the call-resolution ratio and the index size.

### Screenshots without a person at the window

```sh
srcrelief <dir> --screenshot out.png [--focus NAME] [--entry LABEL] [--overview]
```

Renders one frame after zooming to the first node matching `NAME` and/or selecting an entry point, writes a PNG
and exits.

## Project settings (optional)

A `srcrelief.toml` at the project root, or passed with `--config`:

```toml
name = "Sample"
exclude = ["**/generated/**"]
index = "/somewhere/else.relief"

[packs]
disable = ["libgdx"]
dirs = ["../my-rule-packs"]

[[category]]
glob = "**/cmd/migrate/**"
category = "dev-tool"          # application, dev-tool or test
```

## Rule packs

Framework knowledge (which methods a framework calls back, what registers an HTTP route, how things are coloured)
lives in **rule packs**, not in SrcRelief's code. The built-in packs in `rules/` use exactly the format you would
write yourself: `java-core`, `libgdx`, `spring-boot`, `dagger`, `go-core`, `fiber`, `rabbitmq-amqp091`,
`google-wire`, `externals`, `layers`, `significance`, `default-visuals`.

Packs load in this order, later ones overriding earlier rules with the same `id`:
built-in → `~/.config/srcrelief/rules/<pack>/` → `<project>/.srcrelief/rules/<pack>/` → `[packs] dirs`.

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
| `main = { category }` | an entry point for every language-level `main` |
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
| `wiring = { file_markers, file_imports, calls, annotations, type_prefixes }` | marks dependency-injection code as wiring |
| `test_code = { file_imports, dir_names }` | marks test helpers as test code |
| `channel_publisher = { methods, receiver_types, file_imports }` | tags the types that publish to a channel; through a wrapper whose caller picks the destination, its callers |
| `channel_consumer = { kinds }` | tags the types owning entry points of these kinds as channel consumers |
| `external_system = { file_imports, name, project_imports }` | tags code in files importing a client library as using that system, optionally only when some file of the project imports one of `project_imports` |

Lifecycles: `once`, `repeating`, `on-event`, `later`, `unknown`. Visual rules (`tag_style`, `frame` with
`auto_prefix` or `pattern`, `hide_tags`, `fold_tags`, `entry_group`) apply without re-indexing; detection rules
trigger a new index automatically.

Without a pack, a method that overrides a method of an external type, on a type a main creates, is still listed as
a callback with lifecycle `unknown`.

## Layout of the repository

```
crates/srcrelief-format   the .relief file: graph model, builder, memory-mapped reader
crates/srcrelief-build    module discovery (Gradle, go.mod, go.work)
crates/srcrelief-java     Java extractor (tree-sitter)
crates/srcrelief-go       Go extractor (tree-sitter)
crates/srcrelief-facts    language-neutral facts handed from extractors to rule packs
crates/srcrelief-rules    rule pack loader and engine
crates/srcrelief-index    indexing pipeline, project settings, cache, CLI
crates/srcrelief-layout   layered top-down layout per container
crates/srcrelief          the viewer (egui on wgpu)
crates/srcrelief-synth    generator for large synthetic projects
rules/                    built-in rule packs
```

## Tests

```sh
cargo test --workspace                     # everything that needs no external project
cargo test --workspace -- --include-ignored  # also the acceptance checks on the two reference projects
```

Scale numbers are in `docs/benchmarks.md`.

## Known limits of the POC

- Calls are resolved by name from what the source states (imports, declared types); chained calls are mostly
  unresolved. The status bar shows the ratio (about 60–70% resolved on the reference projects).
- Overloaded methods share one box; fields are not drawn.
- No source view and no tunnel (sequence) view yet; the index already records spans, lifecycles and call edges
  for them.
- JS/TS, Maven and other languages are not indexed.
