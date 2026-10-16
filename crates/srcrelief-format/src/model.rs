use rkyv::{Archive, Deserialize, Serialize};

pub type NodeId = u32;
pub const NONE: u32 = u32::MAX;

pub const SCOPE_MAIN: u8 = 1;
pub const SCOPE_TEST: u8 = 2;

macro_rules! u8_enum {
    ($name:ident { $($variant:ident),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[repr(u8)]
        pub enum $name { $($variant),+ }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            pub fn from_u8(v: u8) -> $name {
                Self::ALL.get(v as usize).copied().unwrap_or(Self::ALL[0])
            }
        }
    };
}

u8_enum!(NodeKind { Project, Module, Package, Type, Member, External });
u8_enum!(SubKind { None, Class, Interface, Enum, Record, Annotation, Struct, NamedType, Method, Function, Field, Constructor });
u8_enum!(Lang { None, Java, Go });
u8_enum!(EdgeKind { DependsOn, Calls, Inherits, Implements, Instantiates, FrameworkDispatch });
u8_enum!(Scope { Main, Test });
u8_enum!(Confidence { Resolved, Guessed });
u8_enum!(Category { Application, DevTool, Test });
u8_enum!(Lifecycle { Once, Repeating, OnEvent, Later, Unknown });

impl EdgeKind {
    pub fn is_call_like(self) -> bool {
        matches!(self, EdgeKind::Calls | EdgeKind::Instantiates | EdgeKind::FrameworkDispatch)
    }
}

impl Scope {
    pub fn mask(self) -> u8 {
        match self {
            Scope::Main => SCOPE_MAIN,
            Scope::Test => SCOPE_TEST,
        }
    }
}

impl Category {
    pub fn parse(s: &str) -> Option<Category> {
        match s {
            "application" => Some(Category::Application),
            "dev-tool" => Some(Category::DevTool),
            "test" => Some(Category::Test),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Category::Application => "application",
            Category::DevTool => "dev-tool",
            Category::Test => "test",
        }
    }
}

impl Lifecycle {
    pub fn parse(s: &str) -> Option<Lifecycle> {
        match s {
            "once" => Some(Lifecycle::Once),
            "repeating" => Some(Lifecycle::Repeating),
            "on-event" => Some(Lifecycle::OnEvent),
            "later" => Some(Lifecycle::Later),
            "unknown" => Some(Lifecycle::Unknown),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Lifecycle::Once => "once",
            Lifecycle::Repeating => "repeating",
            Lifecycle::OnEvent => "on-event",
            Lifecycle::Later => "later",
            Lifecycle::Unknown => "unknown",
        }
    }
}

macro_rules! flag_set {
    ($name:ident { $($flag:ident = $bit:expr),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
        pub struct $name(pub u8);

        impl $name {
            pub const NONE: $name = $name(0);
            $(pub const $flag: $name = $name($bit);)+

            pub fn contains(self, other: $name) -> bool {
                self.0 & other.0 == other.0
            }

            pub fn intersects(self, other: $name) -> bool {
                self.0 & other.0 != 0
            }
        }

        impl std::ops::BitOr for $name {
            type Output = $name;
            fn bitor(self, rhs: $name) -> $name {
                $name(self.0 | rhs.0)
            }
        }

        impl std::ops::BitOrAssign for $name {
            fn bitor_assign(&mut self, rhs: $name) {
                self.0 |= rhs.0;
            }
        }
    };
}

flag_set!(NodeFlags { EXTERNAL = 1, TEST = 2, MOCK = 4, GENERATED = 8, WIRING = 16, NOISE = 32, DETAIL = 64 });
flag_set!(EdgeFlags { UNDECLARED = 1, WIRING = 2, NOISE = 4 });

#[derive(Clone, Debug, PartialEq)]
pub struct EntryPointSpec {
    pub node: NodeId,
    pub kind: String,
    pub category: Category,
    pub label: String,
    pub parent: Option<u32>,
    pub lifecycle: Lifecycle,
    pub order: u32,
    pub rule: String,
    pub handoff: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    pub files: u32,
    pub lines: u64,
    pub source_bytes: u64,
    pub calls_resolved: u64,
    pub calls_guessed: u64,
    pub calls_unresolved: u64,
    pub skipped_roots: Vec<(String, String)>,
}

#[derive(Archive, Serialize, Deserialize, Debug)]
pub struct Node {
    pub name: u32,
    pub parent: u32,
    pub kind: u8,
    pub sub: u8,
    pub lang: u8,
    pub flags: u8,
    pub subtree_lines: u32,
    pub file: u32,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, Copy)]
pub struct Edge {
    pub to: u32,
    pub kind: u8,
    pub scope: u8,
    pub confidence: u8,
    pub flags: u8,
    pub count: u32,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, Copy)]
pub struct Agg {
    pub a: u32,
    pub b: u32,
    pub calls: u16,
    pub deps: u16,
    pub wiring: u16,
    pub implements: u16,
    pub scope_mask: u8,
    pub undeclared: bool,
}

#[derive(Archive, Serialize, Deserialize, Debug)]
pub struct EntryPoint {
    pub node: u32,
    pub kind: u32,
    pub category: u8,
    pub lifecycle: u8,
    pub label: u32,
    pub parent: u32,
    pub order: u32,
    pub rule: u32,
    pub handoff: u32,
}

#[derive(Archive, Serialize, Deserialize, Debug)]
pub struct PackStamp {
    pub name: u32,
    pub version: u32,
    pub hash: u32,
}

#[derive(Archive, Serialize, Deserialize, Debug, Default)]
pub struct GraphStats {
    pub files: u32,
    pub lines: u64,
    pub source_bytes: u64,
    pub calls_resolved: u64,
    pub calls_guessed: u64,
    pub calls_unresolved: u64,
    pub skipped_kinds: Vec<u32>,
    pub skipped_paths: Vec<u32>,
}

#[derive(Archive, Serialize, Deserialize, Debug)]
pub struct Graph {
    pub strings: Vec<u8>,
    pub str_offsets: Vec<u32>,
    pub nodes: Vec<Node>,
    pub child_offsets: Vec<u32>,
    pub edges: Vec<Edge>,
    pub edge_index: Vec<u32>,
    pub aggs: Vec<Agg>,
    pub agg_offsets: Vec<u32>,
    pub tags: Vec<u32>,
    pub tag_offsets: Vec<u32>,
    pub entries: Vec<EntryPoint>,
    pub stats: GraphStats,
    pub provenance: Vec<PackStamp>,
}
