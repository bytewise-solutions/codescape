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
| Pan | drag |
| Zoom | mouse wheel or pinch, around the pointer |
| Zoom into a box | double-click it |
| Inspect a box | click it (right panel) |
| Trace an entry point | click it in the left panel: everything it reaches turns green |
| Jump to a name | `Ctrl+K`, type, `Enter` |
| Go back up | click a breadcrumb segment, or the minimap |

A container opens to show its children once it is big enough; arrows inside appear once the children are large
enough to read.

### Index from the command line

```sh
cargo run --release -p srcrelief-index -- ~/path/to/project [--config FILE] [--out FILE] [--force]
```

Prints modules, their links, entry points grouped as in the viewer, the call-resolution ratio and the index size.

### Screenshots without a person at the window

```sh
srcrelief <dir> --screenshot out.png [--focus NAME] [--entry LABEL]
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
write yourself: `java-core`, `libgdx`, `spring-boot`, `go-core`, `fiber`, `rabbitmq-amqp091`, `default-visuals`.

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
