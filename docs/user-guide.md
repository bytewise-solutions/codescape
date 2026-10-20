# Codescape user guide

How Codescape finds a project, what the map shows, and how to move around it. For settings and rule packs
see [configuration.md](configuration.md).

## How a project is read

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

The status bar's `calls X% resolved · Y% guessed` covers calls into your own code. Calls on library values (the JDK,
Go's standard library, npm packages, Rust's `std`, and chains that start from one) are counted apart; point at the
figure for the four counts.

Nothing is written into the project. The index lives in the user cache folder
(`~/.cache/codescape/<hash>.codescape` on Linux) and is rebuilt when sources, detection rules or index settings
change.

The menu bar sits at the top left: **File** (Open Folder… `Ctrl+O`, Open Recent, Re-index, Project Settings…
`Ctrl+,`, Close Project) and **View** (what to show, one item per filter, the legend, Jump to… `Ctrl+K`, Find in file… `Ctrl+F`, File structure… `Ctrl+Shift+O`). What you
tick in View is remembered for each project.

## Navigating

| Action | How |
|---|---|
| Pan | drag, or two-finger swipe on a trackpad |
| Scroll | mouse wheel (vertical), `Shift` + wheel (horizontal) |
| Zoom | `Ctrl` + wheel or pinch, around the pointer |
| Zoom into a box | double-click it |
| Inspect a box | click it (right panel) |
| Focus an entry point or a box | click it: what it **uses** turns pink, what **uses it** turns blue, the rest dims |
| Walk the code | click a row under *Uses* or *Used by* in the inspector (tests hidden on the map are left out, with a `+N in tests` note); `Alt+Left` or **Back** retraces |
| See everything involved | inspector **Show**: *neighbours*, *everything it reaches*, *everywhere it's used* |
| Jump to a name | `Ctrl+K`, type, `Enter` |
| Find text in the open file | `Ctrl+F`, type; `Enter` / `Shift+Enter` for the next / previous match |
| Go to a method of the open file | `Ctrl+Shift+O`, type part of its name, `Enter` |
| Go to a definition from the code | hold `Ctrl` over a name in the open file (it underlines), click |
| Go back up | click a breadcrumb segment, or the minimap |

Keyboard (press `?` or `F1`, or **Help ▸ Shortcuts**, for the full cheatsheet with mouse gestures). The shortcuts
with `Ctrl`, `F3` and `F11` can be rebound in **Help ▸ Change shortcuts…**: click one, press the new keys. They are
saved for every project in `~/.config/codescape/keys.toml` (only the ones you changed):

| Keys | Action |
|---|---|
| `←` `→` `↑` `↓` | select the nearest box in that direction |
| `Enter` / `Backspace` | zoom into the selected box / go up to its parent |
| `Esc` | close the window on top; with none open, close the sequence diagram, else leave the open file, else clear the selection |
| the same shortcut again | close what it opened: `F1`/`?`, `S`, jump, find, file structure, settings |
| `1` `2` `3` | show neighbours / everything it reaches / everywhere it's used |
| `+` `−` / `F` | zoom in / out / fit the whole project |
| `Page Up` `Page Down` `Home` `End` | scroll the file under the pointer or the selected one |
| `Alt+←` | back to the previous selection |
| `Ctrl+K` | jump to a name (`↑` `↓` to pick a hit, `Enter` to go) |
| `Ctrl+F` | find in the open file; matches are highlighted, `Enter` / `Shift+Enter` step through them |
| `F3` / `Shift+F3` | next / previous match while the find bar is open |
| `Ctrl+Shift+O` | the open file's methods, fields and nested types in source order; type to filter, `Enter` to go |
| `S` | sequence diagram of the selected method (click a line in an open file to select its method), or of a class from its most used method |
| `N` / `P` | next / previous change (Review) |
| `R` | mark the current change reviewed (Review) |
| `[` / `]` | previous / next commit, commit by commit (Review) |
| `Ctrl+B` / `Ctrl+I` / `F11` | hide or show the left panel / the right panel / both |
| `Ctrl+O` / `Ctrl+,` | open a folder / project settings |

A container opens to show its children once it is big enough; arrows inside appear once the children are large
enough to read.

## What you see

- **Entry points on top.** Inside every container, the children that hold an application entry point sit on the
  top row, alphabetically, so a project looks the same from one day to the next. Everything else layers below by
  dependency; red arrows point upward (a layering violation), amber ones join two boxes of the same layer.
- **Box colours by kind:** interface (teal), class/struct (blue), enum/record (violet), function/method (grey),
  constructor (dim blue). A legend sits in the bottom-left corner and lists only what the current view draws, so it changes as you zoom,
  select or review; fold it with its arrow or **View ▸ Legend**.
- **The file when you zoom in close.** The class, struct or top-level function under the pointer (the middle of the
  view from the keyboard) gives way to its whole file once it is wide enough on screen, and the code then fills the
  whole map area; the minimap and the legend step aside. Only that one opens. While it is open, zooming (`Ctrl` +
  wheel, pinch, `+` `−`) changes only the text size; zoom out past the smallest size, or press `Esc`, to go back to
  the map. The wheel and dragging scroll the file, sideways too (horizontal wheel, `Shift` + wheel or a sideways
  drag), and so do `Page Up`/`Page Down`/`Home`/`End`. It opens at the type's first line. The type's own lines and
  its methods sit on a light band. Hold `Ctrl` and point at a name: the name the index resolved is underlined and
  `Ctrl`+click opens its definition (a field leads to its class). Names it could not resolve stay plain, and so do
  names defined in code the map hides (tests, errors, loggers, wiring or boxes you hid) until you show that
  code. Types that hold other boxes (an interface with its implementations) stay boxes. Lines are read from disk.
- **Quiet arrows.** Every dependency is drawn, but faint, slightly stronger for the heaviest ones in a container. Hover
  or select a box and its own arrows light up at full strength, as thick as the number of classes they link. Turn
  **View ▸ Quiet arrows** off to draw every arrow by its weight instead.
- **Click an arrow** to open a window listing the calls behind it (`caller → callee  :line`). Click a row to open the
  file at that call; **Go to** buttons fly to either end. Hovering an arrow brightens it and shows a pointing hand.
- **Implements / inherits** arrows are dotted with a hollow triangle at the interface, like UML.
- **Types are UML cards.** A header with the stereotype («interface», «trait», «struct», «enumeration», «record»)
  and the name, then the fields (`- cache: Map<String, Integer>`), then the methods as left-aligned rows with their visibility (`+` public, `#` protected, `~` package,
  `-` private), constructors first (◇); a dot
  marks methods used from outside the type or that are entry points. Arrows join cards, not methods. Hover a row to
  see it, click it to select the method (its uses light up), double-click to open the file at that method. Functions
  that live directly in a package (Go, TypeScript, Rust) stay boxes.
- **Complexity hints.** Every function's cyclomatic complexity is counted while indexing (1 + each `if`, loop,
  non-default `case` or extra `match` arm, `catch`, ternary, `&&`, `||`, `??`). From 10 a method row or function box
  carries an amber number, red from 20; a card's header shows its highest. The right panel gives the exact value:
  under the method's header (hover for the breakdown) below the hint, in its FLAGS once it is flagged.
  **View ▸ Complexity hints** turns them off. Hovering a number explains it: the decisions counted, the nesting and
  the level it passed, as in the right panel. **View ▸ Colour by complexity** (off by default) also tints every box by
  the most complex function inside it, on a ramp from amber (at the hint level) to red (at the high level); below the
  hint a box stays neutral, as do boxes without functions, and open boxes are tinted at half strength. Code hidden
  from the map is not counted. The legend shows the ramp.
- **Review mode.** The **Overview | Review** switch at the top says which mode you are in (Review is greyed out
  outside a git repository). In Review an orange bar picks what to compare: **Base** (by default where the target
  left `main` or `master`) and **Target** (by default the working copy, uncommitted changes included), each from the
  branches or any commit or tag. When the target is another branch, the map and the files show that branch. Choose
  **Whole range** or **Commit by commit**: the latter steps through the commits from base to target (`<` `>` or
  `[` `]`), each compared with its parent, then the uncommitted changes. Every version is exported with
  `git archive` into the cache folder and indexed once; neither your checkout nor the repository is touched (no
  worktree, nothing in `.git`). Versions unused for 14 days are deleted when a review starts, and **Review ▸ Clear
  cached versions** deletes them all. Blocks are matched by their
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
  read it as a UML sequence diagram: one lifeline per class (or file for free functions), the caller one level up
  (**Whole chain** in the header climbs to the nearest entry point, **One level** goes back), then its calls in the
  order they are written, two levels deep. `+ open` shows one level more; a method already on the stack is marked
  `recursive`. A call through an interface follows its only implementation, or offers the implementations to pick
  from; a caller in the chain with alternatives is a menu to pick another. Click an arrow to open the code at that
  call, double-click it to tell the called method instead, double-click a lifeline to see that class on the map;
  **Back to map** or `Esc` returns. In Review mode new and changed calls are coloured as on the map.
- **Reviewed marks.** Tick a change (or press `R`) once you have looked at it; `34 / 91 reviewed` tracks progress and
  **Hide reviewed** leaves the rest. A mark remembers the block's text: when the agent changes it again, the change
  comes back as `changed since review`. Marks are kept per project in the cache folder.
- **Unused code.** **View ▸ Unused code** marks blocks nothing in main code calls, creates or names outside their
  own lines, and lists them under Insights ▸ Unused code: `unused`, `no caller in the project` (public, so possibly API used by
  others) or `only tests use it`. Entry points, tests, generated code, constructors, methods implementing a
  supertype's or marked as overrides (Java `@Override`, TS `override`), and the methods the runtime calls itself (Java `equals(Object)`, `hashCode()`, `toString()`, `clone()`,
  `finalize()`; Go `String()` and `Error()`; TS/JS `toString()`, `valueOf()`, `toJSON(key)`) are never listed. Code reached by reflection or a framework can still show up, so read it as a hint.
  `codescape-index <dir> --unused` prints the same list.
- **Insights.** At the top of the left panel, collapsible sections list areas of interest, each row a name and a short reason (hover for
  the full explanation, click to fly there; a method opens its code): most complex, hotspots (complex code changed
  in at least 3 commits within the hotspot period; `needs git` outside a repository, `not in review` in Review mode),
  most depended on, upward dependencies, unused code, untested public code and duplicates. **Whole project / Selection**
  limits them to the selected block (Selection is available once something is selected). Unused code (every
  block, unused ones first) and the unused flags follow **View ▸ Unused code**; with it off, the section says so. Thresholds are in the map panel's HINTS section.
- **Flags in the inspector.** A selected block with flags gets a FLAGS section at the top of the right panel: each
  flag's short name coloured by kind, its full explanation below (complexity breakdown, the commits behind a
  hotspot, unused wording, duplicate places, an upward arrow's target), and the other duplicate places or the upward
  target as rows that fly there.
- **Flags in the code.** In an open file, every flagged method or type gets small letter icons in the gutter on its
  first line, one per kind and coloured like the inspector (`C` complex, `H` hotspot, `U` unused, `N` no caller, `T`
  only tests, `?` untested, `D` duplicate, `^` points up). Hover them for each flag's reason; this works in the
  reading view and in the files open on the map. Clicking a line, icons included, selects that method, so the
  inspector's FLAGS section shows the same reasons with clickable rows for the other duplicate copies; a duplicate
  names the file when the other copy has the same name.
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
- **Loggers are set aside:** types named `Logger` or `*Logger` (Java also `*Logging`, `LoggerFactory`, `LogManager`;
  Go also `SugaredLogger`) and types extending them, plus fields of those types on cards and in the inspector, and
  their entries in USES / USED BY lists. Tick **View ▸ Logging** to show them.
- **Test code is out:** test sources, test helpers (files importing `testing`/testify or JUnit/Mockito, folders such
  as `testutil`/`testdata`, globs under `[test] paths`) and test mains are not drawn or listed. Tick **View ▸ Tests**
  to draw them and **View ▸ Test dependencies** to see test-only dependencies.

## Choosing what's on the map

**View ▸ What's on the map…** gathers every choice in one panel, each with how many blocks it concerns:

- **Code:** tests, test dependencies, generated code, errors and exceptions, loggers, dependency-injection wiring, details.
- **Cards:** private members, fields, getters and setters, constructors.
- **Arrows:** quiet arrows, guessed calls (arrows made only of calls guessed by name, drawn dotted), implements /
  extends, imports without calls. Hiding an arrow kind never moves a box.
- **Hints:** complexity hints and their two levels, unused code, how many callers make a change risky, how long
  compared versions are kept, and how many tokens a copy needs to count as duplicated code (50, at least 10;
  identifiers and literals are ignored, so a copy with renamed variables still counts; test and generated code are
  left out; changing it re-indexes the project about a second after the last change), and how many days of git history count when ranking hotspots (365).
- **Hidden by you:** right-click any box ▸ **Hide this**, **Hide package …** or **Hide every name ending in …**
  (`…Exception`, `…Dto`), or type a pattern in the panel. Each entry says which settings file holds it and has
  **show again**.
- **By what a type extends:** right-click a type ▸ **Hide every type extending …** (the type itself, each of its
  supertypes, or the family such as `…Exception`), or type a pattern and press **Hide types extending**. The match
  runs along the extends / implements chain the index knows: project types, library types they name directly, and
  the JDK's exception tree (`IllegalStateException` → `RuntimeException` → `Exception` → `Throwable`). Java and
  TypeScript record extends and implements; Rust records trait implementations; Go records the project interfaces a
  type satisfies.

The right panel says why a block is set aside (for example `detail: only getters and setters` or
`error: name ends with Exception`) and offers **Always show** for one that is hidden, **Hide** for one that is
not. Everything here is a display setting: it applies at once, is kept in your file for the project, and can be
moved to `codescape.toml` for the team from **File ▸ Project Settings…**.

## Room for the map

`Ctrl+B` hides or shows the left panel, `Ctrl+I` the right one and `F11` both. With the mouse: the small arrow at
the bottom inner corner of a panel hides it, and the slim strip left at the window edge brings it back (View menu
too).

The `+` and `-` buttons at the top of the left panel (next to ENTRY POINTS) and in the right panel (next to Hide)
expand or collapse every section of that panel at once.

## Screenshots without a person at the window

```sh
codescape <dir> --screenshot out.png [--focus NAME] [--entry LABEL] [--zoom FACTOR] [--settings] [--review REV] [--review-target REV] [--review-by-commit]
         [--open METHOD] [--sequence METHOD] [--map-only] [--fly NAME] [--links]
```

Renders one frame after zooming to the first node matching `NAME`, selecting an entry point, opening the settings
window and/or reviewing (base `REV`, `main` for the merge-base; another target; commit by commit), writes a PNG
and exits. `--links` underlines every name of the open file that `Ctrl`+click follows.
