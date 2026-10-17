# SrcRelief

See a whole codebase as one zoomable map: entry points first, modules laid out top-down by dependency, and
packages, types and members appearing as you zoom in. Arrows that point upward (a lower layer depending on a
higher one) are drawn red and counted, so you can tell at a glance whether a project is layered cleanly.

This is the proof of concept described in `docs/superpowers/specs/2026-10-16-srcrelief-design.md`. It reads
**Java (Gradle)**, **Go (go.mod / go.work)**, **TypeScript / JavaScript (package.json)** and **Rust (Cargo)**
projects.

## Run it

```sh
cargo run --release -p srcrelief -- ~/path/to/project      # open a project folder
cargo run --release -p srcrelief                           # start screen with "Open folder…" and recent projects
```

Point it at the project folder. Every module under it is discovered and linked by itself:

- **Gradle**: modules from `settings.gradle(.kts)`, links from `project(':x')` dependencies (test-scope links are
  kept apart and hidden by default, tick **View ▸ Test dependencies**).
- **Go**: modules from `go.work`, or every `go.mod` under the folder; a module depends on another when its
  `require` names it.
- **TypeScript / JavaScript**: every `package.json` folder with sources of its own, plus npm/yarn workspaces and
  `pnpm-workspace.yaml`; links from `dependencies` naming another package (`devDependencies` are test links). Imports
  follow relative paths, `tsconfig.json` `paths` and workspace package names. Only code is read (`.ts`, `.tsx`,
  `.js`, `.jsx`, `.mjs`, `.cjs`); HTML, Vue, Svelte and Astro templates are not.
- **Rust**: Cargo workspace members, or a lone crate; links from path and workspace dependencies. Packages follow the
  module tree (`(lib)`, `a::b`, `bin/x`, `examples/x`), and methods from every `impl` block land on their type.
- Other build roots found inside (`pom.xml`, `pyproject.toml`, …) are listed in the status bar as not indexed.

Nothing is written into the project. The index lives in the user cache folder
(`~/.cache/srcrelief/<hash>.relief` on Linux) and is rebuilt when sources, detection rules or index settings
change.

The menu bar sits at the top left: **File** (Open Folder… `Ctrl+O`, Open Recent, Re-index, Project Settings…
`Ctrl+,`, Close Project) and **View** (what to show, one item per filter, the legend, Jump to… `Ctrl+K`). What you
tick in View is remembered for each project.

### Navigating

| Action | How |
|---|---|
| Pan | drag, or two-finger swipe on a trackpad |
| Scroll | mouse wheel (vertical), `Shift` + wheel (horizontal) |
| Zoom | `Ctrl` + wheel or pinch, around the pointer |
| Zoom into a box | double-click it |
| Inspect a box | click it (right panel) |
| Focus an entry point or a box | click it: what it **uses** turns pink, what **uses it** turns blue, the rest dims |
| Walk the code | click a row under *Uses* or *Used by* in the inspector; `Alt+Left` or **Back** retraces |
| See everything involved | inspector **Show**: *neighbours*, *everything it reaches*, *everywhere it's used* |
| Jump to a name | `Ctrl+K`, type, `Enter` |
| Go back up | click a breadcrumb segment, or the minimap |

Keyboard (press `?` or `F1`, or **Help ▸ Shortcuts**, for the full cheatsheet with mouse gestures):

| Keys | Action |
|---|---|
| `←` `→` `↑` `↓` | select the nearest box in that direction |
| `Enter` / `Backspace` | zoom into the selected box / go up to its parent |
| `Esc` | clear the selection |
| `1` `2` `3` | show neighbours / everything it reaches / everywhere it's used |
| `+` `−` / `F` | zoom in / out / fit the whole project |
| `Page Up` `Page Down` `Home` `End` | scroll the file under the pointer or the selected one |
| `Alt+←` | back to the previous selection |
| `Ctrl+K` | jump to a name (`↑` `↓` to pick a hit, `Enter` to go) |

A container opens to show its children once it is big enough; arrows inside appear once the children are large
enough to read.

### What you see

- **Entry points on top.** Inside every container, the children that hold an application entry point sit on the
  top row, alphabetically, so a project looks the same from one day to the next. Everything else layers below by
  dependency; red arrows point upward (a layering violation), amber ones join two boxes of the same layer.
- **Box colours by kind:** interface (teal), class/struct (blue), enum/record (violet), function/method (grey),
  constructor (dim blue). A legend sits in the bottom-left corner; close it with its × or **View ▸ Legend**.
- **The file when you zoom in close.** The class, struct or top-level function under the pointer (the middle of the
  view from the keyboard) gives way to its whole file once it fills about half the view, as a page filling its
  column; only that one opens, and it stays open until you zoom back out. The wheel scrolls the file, sideways too
  (horizontal wheel or `Shift` + wheel). It opens with the
  type's first line where its box was; the wheel scrolls the file, not the map (`Page Up`/`Page Down`/`Home`/`End`
  too), `Ctrl` + wheel still zooms. The type's own lines and its methods sit on a light band. Types that hold other
  boxes (an interface with its implementations) stay boxes. Lines are read from disk.
- **Quiet arrows.** Every dependency is drawn, but faint, slightly stronger for the heaviest ones in a container. Hover
  or select a box and its own arrows light up at full strength, as thick as the number of classes they link. Turn
  **View ▸ Quiet arrows** off to draw every arrow by its weight instead.
- **Implements / inherits** arrows are dotted with a hollow triangle at the interface, like UML.
- **Types are UML cards.** A header with the stereotype («interface», «trait», «struct», «enumeration», «record»)
  and the name, then the fields (`- cache: Map<String, Integer>`), then the methods as left-aligned rows with their visibility (`+` public, `#` protected, `~` package,
  `-` private), constructors first (◇); a dot
  marks methods used from outside the type or that are entry points. Arrows join cards, not methods. Hover a row to
  see it, click it to select the method (its uses light up), double-click to open the file at that method. Functions
  that live directly in a package (Go, TypeScript, Rust) stay boxes.
- **Complexity hints.** Every function's cyclomatic complexity is counted while indexing (1 + each `if`, loop,
  non-default `case` or extra `match` arm, `catch`, ternary, `&&`, `||`, `??`). From 10 a method row or function box
  carries an amber number, red from 20; a card's header shows its highest. The right panel gives the exact value.
  **View ▸ Complexity hints** turns them off.
- **Review mode.** The **Overview | Review** switch at the top says which mode you are in (Review is greyed out
  outside a git repository). In Review an orange bar picks what to compare: **Base** (by default where the target
  left `main` or `master`) and **Target** (by default the working copy, uncommitted changes included), each from the
  branches or any commit or tag. When the target is another branch, the map and the files show that branch. Choose
  **Whole range** or **Commit by commit**: the latter steps through the commits from base to target (`<` `>` or
  `[` `]`), each compared with its parent, then the uncommitted changes. Every version is checked out in a git
  worktree under the cache folder and indexed once; your checkout is never touched. Blocks are matched by their
  names: new ones are outlined green with `NEW`, changed ones amber with `+added −removed` lines, the rest dimmed.
  Open boxes add up what changed inside them (`2 new · 3 changed · 1 removed`), card rows are tinted with their line
  counts and any complexity increase, and arrows the change introduced are drawn green. The left
  panel lists every change, riskiest first, then new and removed dependencies (upward ones flagged); click one to fly
  to it, or step through them with `N` / `P`. Each change says why it may deserve a look: `complexity 8 to 14`,
  `used by 12` (many callers), `signature` (a public block's first line changed), `made public`, `unused` (a new
  block nothing uses) and `untested` (no test reaches it). **By package** groups the list instead.
  Zoom into a changed file to see its added lines in green and red marks where lines were removed; hover a mark to
  read them. Renamed or moved blocks show as removed plus new.
- **Sequence diagrams.** Right-click a method (or select it and press `S`, or use the button in the right panel) to
  read it as a UML sequence diagram: one lifeline per class (or file for free functions), the chain of callers from
  the nearest entry point down to it, then its calls in the order they are written, two levels deep. `+ open`
  shows one level more; a method already on the stack is marked `recursive`. A call through an interface follows its
  only implementation, or offers the implementations to pick from; a caller in the chain with alternatives is a menu
  to pick another. Click an arrow to open the code at that call, double-click it to tell the called method instead,
  double-click a lifeline to see that class on the map; **Back to map** or `Esc` returns. In Review mode new and
  changed calls are coloured as on the map.
- **Reviewed marks.** Tick a change (or press `R`) once you have looked at it; `34 / 91 reviewed` tracks progress and
  **Hide reviewed** leaves the rest. A mark remembers the block's text: when the agent changes it again, the change
  comes back as `changed since review`. Marks are kept per project in the cache folder.
- **Unused code.** **View ▸ Unused code** marks blocks nothing in main code calls, creates or names outside their
  own lines, and lists them on the left: `unused`, `no caller in the project` (public, so possibly API used by
  others) or `only tests use it`. Entry points, tests, generated code, constructors and methods implementing a
  supertype's are never listed. Code reached by reflection or a framework can still show up, so read it as a hint.
  `srcrelief-index <dir> --unused` prints the same list.
- **Constructors live inside their type.** A Go `func NewX(...) *X` is a row of `X`'s card, not a box of its own.
- **Dependency injection is set aside.** Code recognised as wiring (Google Wire's generated injectors and provider
  sets, Dagger components/modules) is hidden with its arrows, so it never changes the layering. Tick **View ▸ Wiring**
  to show it muted, with faint dotted arrows. Highlighting an entry point still follows wiring.
- **Architecture first.** Only architectural types are drawn by default: handlers, services, repositories,
  interfaces, classes other classes extend, entry-point owners, what the DI framework assembles (Wire/Dagger, Spring `@Component`/`@Service`,
  `@Inject` constructors) and types used from two or more other packages. Data types (structs without methods,
  records, enums, getter/setter classes, `*Request`/`*Response`/models), nested types and helpers in
  `util`/`common` packages are *details*: an open box shows `+ N details`; click it to show that box's details and
  `hide N details` to put them away again, or tick **View ▸ Details** to show them everywhere.
  The `significance` pack holds these rules.
- **Implementations live inside their interface.** At first glance you see the interface; open it to see the
  structs or classes that implement it.
- **Error and exception types are set aside** (Java `*Exception` and subclasses of Exception/Throwable/Error, Go
  types with an `Error()` method or named `*Error`); tick **View ▸ Errors** to show them in their own row.
- **Test code is out:** test sources, test helpers (files importing `testing`/testify or JUnit/Mockito, folders such
  as `testutil`/`testdata`, globs under `[test] paths`) and test mains are not drawn or listed. Tick **View ▸ Tests**
  to draw them and **View ▸ Test dependencies** to see test-only dependencies.

### Room for the map

`Ctrl+B` hides or shows the left panel, `Ctrl+I` the right one and `F11` both. With the mouse: the small arrow at
the bottom inner corner of a panel hides it, and the slim strip left at the window edge brings it back (View menu
too).

### Index from the command line

```sh
cargo run --release -p srcrelief-index -- ~/path/to/project [--config FILE] [--out FILE] [--force]
```

Prints modules, their links, entry points grouped as in the viewer, the call-resolution ratio and the index size.

### Screenshots without a person at the window

```sh
srcrelief <dir> --screenshot out.png [--focus NAME] [--entry LABEL] [--zoom FACTOR] [--settings] [--review REV] [--review-target REV] [--review-by-commit]
         [--open METHOD] [--sequence METHOD] [--map-only]
```

Renders one frame after zooming to the first node matching `NAME`, selecting an entry point, opening the settings
window and/or reviewing (base `REV`, `main` for the merge-base; another target; commit by commit), writes a PNG
and exits.

## Project settings (optional)

Settings come from four layers; a later one overrides an earlier one:

1. built-in defaults (tests, test deps, details, errors and wiring hidden; build output, `vendor`, dot folders left out),
2. your defaults for every project: `~/.config/srcrelief/settings.toml`,
3. the project's `srcrelief.toml` (or `--config FILE`), shared with the team,
4. your settings for this project: `~/.config/srcrelief/projects/<hash>.toml`, written by the viewer.

All four files take the same keys:

```toml
name = "Sample"
exclude = ["**/generated/**"]
index = "/somewhere/else.relief"

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
your defaults and *save to srcrelief.toml* moves it to the shared file, keeping that file's comments. The viewer
writes nothing into the project unless you press that last button.

## Rule packs

Framework knowledge (which methods a framework calls back, what registers an HTTP route, how things are coloured)
lives in **rule packs**, not in SrcRelief's code. The built-in packs in `rules/` use exactly the format you would
write yourself: `java-core`, `libgdx`, `spring-boot`, `dagger`, `go-core`, `fiber`, `rabbitmq-amqp091`,
`google-wire`, `ts-core`, `angular`, `express`, `rust-core`, `layers`, `significance`, `default-visuals`.

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

## Layout of the repository

```
crates/srcrelief-format   the .relief file: graph model, builder, memory-mapped reader
crates/srcrelief-build    module discovery (Gradle, go.mod, go.work, package.json, Cargo)
crates/srcrelief-java     Java extractor (tree-sitter)
crates/srcrelief-go       Go extractor (tree-sitter)
crates/srcrelief-ts       TypeScript / JavaScript extractor (tree-sitter)
crates/srcrelief-rust     Rust extractor (tree-sitter)
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
cargo test --workspace -- --include-ignored  # also the acceptance checks on the reference projects and on SrcRelief itself
```

Scale numbers are in `docs/benchmarks.md`.

## Known limits of the POC

- Calls are resolved from what the source states (imports, declared types, return types, element types of
  collections for loop and closure variables); there is no full type inference, so pattern bindings, tuples and
  generic results stay unresolved, and nothing inside Rust macros is read. Calls on library values count as resolved
  library calls. The status bar shows the ratio (about 68–85% resolved depending on the language).
- Overloaded methods share one row.
- Maven, Kotlin, Python and other languages are not indexed.
