use srcrelief::matrix::*;
use srcrelief_layout::{ChildBox, ContainerLayout, Direction, LaidEdge, Rect};

fn child(node: u32, layer: u32, x: f32) -> ChildBox {
    ChildBox { node, rect: Rect::new(x, layer as f32 * 100.0, 50.0, 40.0), layer, pinned: false }
}

fn edge(a: u32, b: u32, links: u32, dir: Direction) -> LaidEdge {
    LaidEdge { a, b, calls: links, deps: 0, wiring: 0, implements: 0, links, dir, undeclared: false, test_only: false, points: vec![] }
}

#[test]
fn rows_follow_the_layers_and_cells_carry_links() {
    let layout = ContainerLayout {
        children: vec![child(7, 1, 200.0), child(5, 0, 300.0), child(6, 0, 100.0), child(8, 1, 0.0)],
        edges: vec![edge(6, 7, 3, Direction::Down), edge(8, 6, 1, Direction::Up), edge(6, 5, 2, Direction::Sideways)],
        ..Default::default()
    };
    let m = Matrix::from_layout(&layout);
    assert_eq!(m.rows, [6, 5, 8, 7], "layer by layer, left to right");
    assert_eq!(m.max_links, 3);
    let cell = |r: usize, c: usize| m.cells.iter().find(|x| x.row == r && x.col == c).map(|x| (x.links, x.dir));
    assert_eq!(cell(0, 3), Some((3, Direction::Down)));
    assert_eq!(cell(2, 0), Some((1, Direction::Up)), "upward dependencies land below the diagonal");
    assert_eq!(cell(0, 1), Some((2, Direction::Sideways)));
    assert_eq!(m.cells.len(), 3);
}

#[test]
fn grid_finds_rows_and_cells_under_the_pointer() {
    let g = Grid::new(Rect::new(0.0, 0.0, 600.0, 500.0), 10);
    let c = g.cell_rect(2, 3);
    assert!(c.w >= 2.0 && (c.w - c.h).abs() < 0.01, "square cells");
    assert_eq!(g.hit([c.x + 1.0, c.y + 1.0]), Some(Hit::Cell(2, 3)));
    let r = g.row_rect(4);
    assert_eq!(g.hit([r.x + 2.0, r.y + 1.0]), Some(Hit::Row(4)));
    assert_eq!(g.hit([599.0, 499.0]), None);
    let last = g.cell_rect(9, 9);
    assert!(last.right() <= 600.0 && last.bottom() <= 500.0, "fits inside the container");
}

#[test]
fn row_names_drop_the_prefix_they_all_share() {
    let java = short_names(&["com.mq.server.game.engine", "com.mq.server.game.agents.people", "com.mq.server.game.ui"]);
    assert_eq!(java, ["engine", "agents.people", "ui"]);
    let go = short_names(&["internal/jobs", "internal/jobs/consumer", "internal/smtp"]);
    assert_eq!(go, ["jobs", "jobs/consumer", "smtp"]);
    assert_eq!(short_names(&["a.b", "a.b.c"]), ["b", "b.c"], "never strips a whole name");
    assert_eq!(short_names(&["Foo", "Bar"]), ["Foo", "Bar"]);
    assert_eq!(short_names(&["only.one"]), ["only.one"]);
}
