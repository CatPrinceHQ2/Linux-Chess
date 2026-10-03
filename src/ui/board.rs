//! Interactive chessboard widget (cairo drawing inside a GtkDrawingArea).
//!
//! * drag a piece: a legal move is *played*; any other drag is a free edit (hold Shift to force
//!   a free edit); dropping a piece outside the board removes it
//! * click a piece to show its legal moves, click a highlighted square to play the move
//! * right-click removes a piece; palette tools place/erase pieces
use crate::chess::*;
use gtk::prelude::*;
use gtk4 as gtk;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Move,
    Place(Piece),
    Erase,
}

struct Drag {
    from: Square,
    piece: Piece,
    start: (f64, f64),
    pos: (f64, f64),
    moved: bool,
}

pub struct BoardState {
    pub position: Position,
    pub selected: Option<Square>,
    pub legal_targets: Vec<Square>,
    pub last_move: Option<(Square, Square)>,
    pub arrow: Option<(Square, Square)>,
    pub flipped: bool,
    pub show_legal: bool,
    pub tool: Tool,
    drag: Option<Drag>,
}

type ChangeCallback = Rc<RefCell<Option<Box<dyn Fn(bool)>>>>;

#[derive(Clone)]
pub struct BoardView {
    area: gtk::DrawingArea,
    state: Rc<RefCell<BoardState>>,
    on_change: ChangeCallback,
}

const LIGHT: (f64, f64, f64) = (0.941, 0.851, 0.710);
const DARK: (f64, f64, f64) = (0.710, 0.533, 0.388);

impl BoardView {
    pub fn new() -> BoardView {
        let area = gtk::DrawingArea::new();
        area.set_content_width(480);
        area.set_content_height(480);
        area.set_hexpand(true);
        area.set_vexpand(true);
        area.set_focusable(true);
        let state = Rc::new(RefCell::new(BoardState {
            position: Position::starting(),
            selected: None,
            legal_targets: Vec::new(),
            last_move: None,
            arrow: None,
            flipped: false,
            show_legal: true,
            tool: Tool::Move,
            drag: None,
        }));
        let view = BoardView { area, state, on_change: Rc::new(RefCell::new(None)) };
        view.install_drawing();
        view.install_input();
        view
    }

    pub fn widget(&self) -> &gtk::DrawingArea {
        &self.area
    }

    /// `f(played_move)` is called after every user change; `played_move` is true when the change
    /// was a legal move (so analysis results are stale and the side to move changed).
    pub fn connect_changed(&self, f: impl Fn(bool) + 'static) {
        *self.on_change.borrow_mut() = Some(Box::new(f));
    }

    fn notify(&self, played: bool) {
        self.area.queue_draw();
        if let Some(cb) = self.on_change.borrow().as_ref() {
            cb(played);
        }
    }

    pub fn position(&self) -> Position {
        self.state.borrow().position.clone()
    }

    pub fn set_position(&self, p: Position, last_move: Option<(Square, Square)>) {
        {
            let mut s = self.state.borrow_mut();
            s.position = p;
            s.last_move = last_move;
            s.selected = None;
            s.legal_targets.clear();
            s.arrow = None;
        }
        self.area.queue_draw();
    }

    pub fn set_arrow(&self, a: Option<(Square, Square)>) {
        self.state.borrow_mut().arrow = a;
        self.area.queue_draw();
    }

    pub fn set_flipped(&self, f: bool) {
        self.state.borrow_mut().flipped = f;
        self.area.queue_draw();
    }

    pub fn set_show_legal(&self, f: bool) {
        let mut s = self.state.borrow_mut();
        s.show_legal = f;
        if !f {
            s.legal_targets.clear();
        }
        drop(s);
        self.area.queue_draw();
    }

    pub fn set_tool(&self, t: Tool) {
        let mut s = self.state.borrow_mut();
        s.tool = t;
        s.selected = None;
        s.legal_targets.clear();
        drop(s);
        self.area.queue_draw();
    }

    /// Plays a legal coordinate move (used by "Play move"). Returns false if illegal.
    pub fn play_uci(&self, uci: &str) -> bool {
        let ok = {
            let mut s = self.state.borrow_mut();
            match s.position.parse_uci_move(uci) {
                Ok(m) => {
                    apply_played(&mut s, m);
                    true
                }
                Err(_) => false,
            }
        };
        if ok {
            self.notify(true);
        }
        ok
    }

    /// Edit-drop of a piece coming from the palette (drag and drop).
    pub fn drop_piece(&self, piece: Piece, x: f64, y: f64) {
        let changed = {
            let mut s = self.state.borrow_mut();
            match square_at(&self.area, &s, x, y) {
                Some(sq) => {
                    s.position.set_piece(sq, Some(piece));
                    s.last_move = None;
                    s.arrow = None;
                    true
                }
                None => false,
            }
        };
        if changed {
            self.notify(false);
        }
    }

    // ------------------------------------------------------------ drawing

    fn install_drawing(&self) {
        let state = self.state.clone();
        self.area.set_draw_func(move |_, cr, w, h| {
            let s = state.borrow();
            draw_board(cr, w as f64, h as f64, &s);
        });
    }

    // ------------------------------------------------------------ input

    fn install_input(&self) {
        // Left button: click / drag.
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        {
            let view = self.clone();
            drag.connect_drag_begin(move |_, x, y| view.drag_begin(x, y));
        }
        {
            let view = self.clone();
            drag.connect_drag_update(move |_, dx, dy| view.drag_update(dx, dy));
        }
        {
            let view = self.clone();
            drag.connect_drag_end(move |g, dx, dy| {
                let shift = g.current_event_state().contains(gtk::gdk::ModifierType::SHIFT_MASK);
                view.drag_end(dx, dy, shift)
            });
        }
        self.area.add_controller(drag);

        // Right button: erase.
        let right = gtk::GestureClick::new();
        right.set_button(3);
        {
            let view = self.clone();
            right.connect_pressed(move |_, _, x, y| {
                let changed = {
                    let mut s = view.state.borrow_mut();
                    s.selected = None;
                    s.legal_targets.clear();
                    match square_at(&view.area, &s, x, y) {
                        Some(sq) if s.position.piece_at(sq).is_some() => {
                            s.position.set_piece(sq, None);
                            s.last_move = None;
                            s.arrow = None;
                            true
                        }
                        _ => false,
                    }
                };
                if changed {
                    view.notify(false);
                } else {
                    view.area.queue_draw();
                }
            });
        }
        self.area.add_controller(right);

        // Drops from the palette.
        let target = gtk::DropTarget::new(String::static_type(), gtk::gdk::DragAction::COPY);
        {
            let view = self.clone();
            target.connect_drop(move |_, value, x, y| {
                if let Ok(text) = value.get::<String>() {
                    if let Some(p) = text.chars().next().and_then(Piece::from_fen_char) {
                        view.drop_piece(p, x, y);
                        return true;
                    }
                }
                false
            });
        }
        self.area.add_controller(target);
    }

    fn drag_begin(&self, x: f64, y: f64) {
        let mut placed = false;
        {
            let mut s = self.state.borrow_mut();
            let Some(sq) = square_at(&self.area, &s, x, y) else { return };
            match s.tool {
                Tool::Place(p) => {
                    s.position.set_piece(sq, Some(p));
                    s.last_move = None;
                    s.arrow = None;
                    placed = true;
                }
                Tool::Erase => {
                    if s.position.piece_at(sq).is_some() {
                        s.position.set_piece(sq, None);
                        s.last_move = None;
                        s.arrow = None;
                        placed = true;
                    }
                }
                Tool::Move => {
                    if let Some(piece) = s.position.piece_at(sq) {
                        s.drag = Some(Drag { from: sq, piece, start: (x, y), pos: (x, y), moved: false });
                    } else {
                        s.drag = None;
                        // Clicking an empty square may complete a click-to-move.
                        s.drag = Some(Drag { from: sq, piece: Piece::new(Color::White, PieceKind::Pawn), start: (x, y), pos: (x, y), moved: false });
                        let _ = &s;
                    }
                }
            }
        }
        if placed {
            self.notify(false);
        }
    }

    fn drag_update(&self, dx: f64, dy: f64) {
        let mut s = self.state.borrow_mut();
        if let Some(d) = s.drag.as_mut() {
            d.pos = (d.start.0 + dx, d.start.1 + dy);
            if dx.abs() + dy.abs() > 6.0 {
                d.moved = true;
            }
        }
        drop(s);
        self.area.queue_draw();
    }

    fn drag_end(&self, dx: f64, dy: f64, shift: bool) {
        let mut played = false;
        let mut changed = false;
        {
            let mut s = self.state.borrow_mut();
            let Some(d) = s.drag.take() else { return };
            let end = (d.start.0 + dx, d.start.1 + dy);
            let from_has_piece = s.position.piece_at(d.from).is_some();
            let dest = square_at(&self.area, &s, end.0, end.1);
            if !d.moved {
                // A click.
                if s.selected.is_some() && dest.map(|t| s.legal_targets.contains(&t)).unwrap_or(false) {
                    let from = s.selected.unwrap();
                    let to = dest.unwrap();
                    if let Some(m) = pick_move(&s.position, from, to) {
                        apply_played(&mut s, m);
                        played = true;
                    }
                } else if from_has_piece && s.position.piece_at(d.from).map(|p| p.color) == Some(s.position.side_to_move) {
                    s.selected = Some(d.from);
                    s.legal_targets = if s.show_legal {
                        s.position.legal_moves_from(d.from).iter().map(|m| m.to).collect()
                    } else {
                        s.position.legal_moves_from(d.from).iter().map(|m| m.to).collect()
                    };
                } else {
                    s.selected = None;
                    s.legal_targets.clear();
                }
            } else if from_has_piece {
                match dest {
                    None => {
                        s.position.set_piece(d.from, None);
                        s.last_move = None;
                        s.arrow = None;
                        s.selected = None;
                        s.legal_targets.clear();
                        changed = true;
                    }
                    Some(to) if to != d.from => {
                        let legal = if shift { None } else { pick_move(&s.position, d.from, to) };
                        match legal {
                            Some(m) => {
                                apply_played(&mut s, m);
                                played = true;
                            }
                            None => {
                                let p = s.position.piece_at(d.from);
                                s.position.set_piece(to, p);
                                s.position.set_piece(d.from, None);
                                s.last_move = None;
                                s.arrow = None;
                                s.selected = None;
                                s.legal_targets.clear();
                                changed = true;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if played || changed {
            self.notify(played);
        } else {
            self.area.queue_draw();
        }
    }
}

impl Default for BoardView {
    fn default() -> Self {
        BoardView::new()
    }
}

/// Finds the legal move from `from` to `to`; pawn promotions default to a queen.
fn pick_move(pos: &Position, from: Square, to: Square) -> Option<Move> {
    let candidates: Vec<Move> = pos.legal_moves().into_iter().filter(|m| m.from == from && m.to == to).collect();
    candidates
        .iter()
        .find(|m| m.promotion == Some(PieceKind::Queen))
        .or_else(|| candidates.first())
        .copied()
}

fn apply_played(s: &mut BoardState, m: Move) {
    s.position = s.position.make_move(&m);
    s.last_move = Some((m.from, m.to));
    s.selected = None;
    s.legal_targets.clear();
    s.arrow = None;
}

// -------------------------------------------------------------------- geometry

fn geometry(w: f64, h: f64) -> (f64, f64, f64) {
    let side = w.min(h);
    ((w - side) / 2.0, (h - side) / 2.0, side / 8.0)
}

fn square_at(area: &gtk::DrawingArea, s: &BoardState, x: f64, y: f64) -> Option<Square> {
    let (x0, y0, sq) = geometry(area.width() as f64, area.height() as f64);
    let fx = ((x - x0) / sq).floor();
    let fy = ((y - y0) / sq).floor();
    if !(0.0..8.0).contains(&fx) || !(0.0..8.0).contains(&fy) {
        return None;
    }
    let (file, row) = (fx as u8, fy as u8);
    Some(if s.flipped { square(7 - file, row) } else { square(file, 7 - row) })
}

fn square_origin(s: &BoardState, sq: Square, x0: f64, y0: f64, size: f64) -> (f64, f64) {
    let (file, rank) = (file_of(sq), rank_of(sq));
    let (col, row) = if s.flipped { (7 - file, rank) } else { (file, 7 - rank) };
    (x0 + col as f64 * size, y0 + row as f64 * size)
}

// -------------------------------------------------------------------- drawing

fn draw_board(cr: &gtk::cairo::Context, w: f64, h: f64, s: &BoardState) {
    let (x0, y0, size) = geometry(w, h);
    let in_check = s.position.is_check().then(|| s.position.king_square(s.position.side_to_move)).flatten();

    for sq in 0..64u8 {
        let (x, y) = square_origin(s, sq, x0, y0, size);
        let light = (file_of(sq) + rank_of(sq)) % 2 == 1;
        let c = if light { LIGHT } else { DARK };
        cr.set_source_rgb(c.0, c.1, c.2);
        cr.rectangle(x, y, size, size);
        let _ = cr.fill();

        if let Some((a, b)) = s.last_move {
            if sq == a || sq == b {
                cr.set_source_rgba(0.61, 0.78, 0.0, 0.42);
                cr.rectangle(x, y, size, size);
                let _ = cr.fill();
            }
        }
        if s.selected == Some(sq) {
            cr.set_source_rgba(1.0, 0.92, 0.23, 0.55);
            cr.rectangle(x, y, size, size);
            let _ = cr.fill();
        }
        if in_check == Some(sq) {
            let g = gtk::cairo::RadialGradient::new(x + size / 2.0, y + size / 2.0, size * 0.1, x + size / 2.0, y + size / 2.0, size * 0.7);
            g.add_color_stop_rgba(0.0, 1.0, 0.0, 0.0, 0.9);
            g.add_color_stop_rgba(1.0, 0.9, 0.0, 0.0, 0.0);
            let _ = cr.set_source(&g);
            cr.rectangle(x, y, size, size);
            let _ = cr.fill();
        }

        // Coordinates in the corners of the edge squares.
        let (col, row) = if s.flipped { (7 - file_of(sq), rank_of(sq)) } else { (file_of(sq), 7 - rank_of(sq)) };
        let label_c = if light { DARK } else { LIGHT };
        cr.set_source_rgba(label_c.0, label_c.1, label_c.2, 0.95);
        cr.select_font_face("Sans", gtk::cairo::FontSlant::Normal, gtk::cairo::FontWeight::Bold);
        cr.set_font_size((size * 0.18).max(8.0));
        if row == 7 {
            cr.move_to(x + size * 0.86 - size * 0.08, y + size - size * 0.07);
            let _ = cr.show_text(&((b'a' + file_of(sq)) as char).to_string());
        }
        if col == 0 {
            cr.move_to(x + size * 0.06, y + size * 0.2);
            let _ = cr.show_text(&(rank_of(sq) + 1).to_string());
        }
        cr.new_path();
    }

    // Legal-move hints.
    if s.show_legal {
        for &t in &s.legal_targets {
            let (x, y) = square_origin(s, t, x0, y0, size);
            cr.set_source_rgba(0.08, 0.33, 0.12, 0.55);
            if s.position.piece_at(t).is_some() {
                cr.set_line_width(size * 0.08);
                cr.arc(x + size / 2.0, y + size / 2.0, size * 0.42, 0.0, std::f64::consts::TAU);
                let _ = cr.stroke();
            } else {
                cr.arc(x + size / 2.0, y + size / 2.0, size * 0.15, 0.0, std::f64::consts::TAU);
                let _ = cr.fill();
            }
        }
    }

    // Pieces (the dragged one is drawn last, under the pointer).
    let dragging = s.drag.as_ref().filter(|d| d.moved && s.position.piece_at(d.from).is_some());
    for sq in 0..64u8 {
        if let Some(p) = s.position.piece_at(sq) {
            if dragging.map(|d| d.from == sq).unwrap_or(false) {
                continue;
            }
            let (x, y) = square_origin(s, sq, x0, y0, size);
            draw_piece(cr, p, x + size / 2.0, y + size / 2.0, size);
        }
    }

    // Best-move arrow.
    if let Some((a, b)) = s.arrow {
        let (ax, ay) = square_origin(s, a, x0, y0, size);
        let (bx, by) = square_origin(s, b, x0, y0, size);
        draw_arrow(cr, ax + size / 2.0, ay + size / 2.0, bx + size / 2.0, by + size / 2.0, size);
    }

    if let Some(d) = dragging {
        draw_piece(cr, d.piece, d.pos.0, d.pos.1, size * 1.1);
    }
}

fn draw_arrow(cr: &gtk::cairo::Context, x1: f64, y1: f64, x2: f64, y2: f64, size: f64) {
    let (dx, dy) = (x2 - x1, y2 - y1);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return;
    }
    let (ux, uy) = (dx / len, dy / len);
    let head = size * 0.34;
    let width = size * 0.13;
    let (sx, sy) = (x1 + ux * size * 0.2, y1 + uy * size * 0.2);
    let (bx, by) = (x2 - ux * head, y2 - uy * head);
    cr.set_source_rgba(0.95, 0.55, 0.05, 0.82);
    cr.set_line_width(width);
    cr.set_line_cap(gtk::cairo::LineCap::Round);
    cr.move_to(sx, sy);
    cr.line_to(bx, by);
    let _ = cr.stroke();
    cr.move_to(x2, y2);
    cr.line_to(bx - uy * head * 0.55, by + ux * head * 0.55);
    cr.line_to(bx + uy * head * 0.55, by - ux * head * 0.55);
    cr.close_path();
    let _ = cr.fill();
}

/// Draws a piece centred at (cx, cy) using the solid Unicode chess glyphs with an outline, so
/// white and black pieces are both clearly visible on both square colours.
pub fn draw_piece(cr: &gtk::cairo::Context, piece: Piece, cx: f64, cy: f64, size: f64) {
    let glyph = match piece.kind {
        PieceKind::King => '♚',
        PieceKind::Queen => '♛',
        PieceKind::Rook => '♜',
        PieceKind::Bishop => '♝',
        PieceKind::Knight => '♞',
        PieceKind::Pawn => '♟',
    };
    // U+FE0E forces text (not emoji) presentation for the pawn glyph.
    let text = format!("{glyph}\u{FE0E}");
    let layout = pangocairo::functions::create_layout(cr);
    let mut font = gtk::pango::FontDescription::new();
    font.set_family("DejaVu Sans, Noto Sans Symbols 2, Noto Sans Symbols, FreeSerif, Symbola, Sans");
    font.set_absolute_size(size * 0.82 * gtk::pango::SCALE as f64);
    layout.set_font_description(Some(&font));
    layout.set_text(&text);
    let (_, ink_logical) = layout.pixel_extents();
    cr.save().ok();
    // pango draws the layout at the cairo *current point*; clear any leftover one (e.g. from the
    // coordinate labels) so the glyph is placed relative to the translation below only.
    cr.new_path();
    cr.translate(cx - ink_logical.width() as f64 / 2.0, cy - ink_logical.height() as f64 / 2.0);
    pangocairo::functions::layout_path(cr, &layout);
    let (fill, stroke) = match piece.color {
        Color::White => ((1.0, 1.0, 1.0), (0.1, 0.1, 0.1)),
        Color::Black => ((0.1, 0.1, 0.1), (0.95, 0.95, 0.95)),
    };
    cr.set_source_rgb(fill.0, fill.1, fill.2);
    let _ = cr.fill_preserve();
    cr.set_source_rgb(stroke.0, stroke.1, stroke.2);
    cr.set_line_width((size * 0.028).max(1.0));
    cr.set_line_join(gtk::cairo::LineJoin::Round);
    let _ = cr.stroke();
    cr.restore().ok();
}
