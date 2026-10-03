//! Basic chess value types. Squares are indexed 0 = a1 .. 63 = h8 (rank * 8 + file).
use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Color {
    White,
    Black,
}

impl Color {
    pub fn opposite(self) -> Color {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Color::White => "White",
            Color::Black => "Black",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum PieceKind {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

impl PieceKind {
    pub fn from_lower_char(c: char) -> Option<PieceKind> {
        Some(match c {
            'p' => PieceKind::Pawn,
            'n' => PieceKind::Knight,
            'b' => PieceKind::Bishop,
            'r' => PieceKind::Rook,
            'q' => PieceKind::Queen,
            'k' => PieceKind::King,
            _ => return None,
        })
    }

    pub fn lower_char(self) -> char {
        match self {
            PieceKind::Pawn => 'p',
            PieceKind::Knight => 'n',
            PieceKind::Bishop => 'b',
            PieceKind::Rook => 'r',
            PieceKind::Queen => 'q',
            PieceKind::King => 'k',
        }
    }

    /// Letter used in SAN (empty for pawns).
    pub fn san_letter(self) -> &'static str {
        match self {
            PieceKind::Pawn => "",
            PieceKind::Knight => "N",
            PieceKind::Bishop => "B",
            PieceKind::Rook => "R",
            PieceKind::Queen => "Q",
            PieceKind::King => "K",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Piece {
    pub color: Color,
    pub kind: PieceKind,
}

impl Piece {
    pub fn new(color: Color, kind: PieceKind) -> Piece {
        Piece { color, kind }
    }

    pub fn from_fen_char(c: char) -> Option<Piece> {
        let kind = PieceKind::from_lower_char(c.to_ascii_lowercase())?;
        let color = if c.is_ascii_uppercase() { Color::White } else { Color::Black };
        Some(Piece { color, kind })
    }

    pub fn fen_char(self) -> char {
        let c = self.kind.lower_char();
        if self.color == Color::White {
            c.to_ascii_uppercase()
        } else {
            c
        }
    }

    /// Unicode chess glyph (the solid glyphs are used for both colours by the
    /// board renderer, which distinguishes colour by fill).
    pub fn unicode(self) -> char {
        match (self.color, self.kind) {
            (Color::White, PieceKind::King) => '♔',
            (Color::White, PieceKind::Queen) => '♕',
            (Color::White, PieceKind::Rook) => '♖',
            (Color::White, PieceKind::Bishop) => '♗',
            (Color::White, PieceKind::Knight) => '♘',
            (Color::White, PieceKind::Pawn) => '♙',
            (Color::Black, PieceKind::King) => '♚',
            (Color::Black, PieceKind::Queen) => '♛',
            (Color::Black, PieceKind::Rook) => '♜',
            (Color::Black, PieceKind::Bishop) => '♝',
            (Color::Black, PieceKind::Knight) => '♞',
            (Color::Black, PieceKind::Pawn) => '♟',
        }
    }
}

pub type Square = u8;

pub fn square(file: u8, rank: u8) -> Square {
    rank * 8 + file
}
pub fn file_of(s: Square) -> u8 {
    s % 8
}
pub fn rank_of(s: Square) -> u8 {
    s / 8
}

pub fn square_name(s: Square) -> String {
    format!("{}{}", (b'a' + file_of(s)) as char, rank_of(s) + 1)
}

pub fn parse_square(s: &str) -> Option<Square> {
    let b = s.as_bytes();
    if b.len() != 2 || !(b'a'..=b'h').contains(&b[0]) || !(b'1'..=b'8').contains(&b[1]) {
        return None;
    }
    Some(square(b[0] - b'a', b[1] - b'1'))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Move {
    pub from: Square,
    pub to: Square,
    pub promotion: Option<PieceKind>,
}

impl Move {
    pub fn new(from: Square, to: Square) -> Move {
        Move { from, to, promotion: None }
    }
    pub fn with_promotion(from: Square, to: Square, kind: PieceKind) -> Move {
        Move { from, to, promotion: Some(kind) }
    }

    /// Coordinate notation as used by UCI, e.g. `e2e4`, `e7e8q`.
    pub fn to_uci(&self) -> String {
        let mut s = format!("{}{}", square_name(self.from), square_name(self.to));
        if let Some(p) = self.promotion {
            s.push(p.lower_char());
        }
        s
    }

    /// Syntax-only parse of coordinate notation (does not check legality).
    pub fn from_uci_str(s: &str) -> Option<Move> {
        let s = s.trim();
        if !s.is_ascii() || !(s.len() == 4 || s.len() == 5) {
            return None;
        }
        let from = parse_square(&s[0..2])?;
        let to = parse_square(&s[2..4])?;
        let promotion = if s.len() == 5 {
            let k = PieceKind::from_lower_char(s.as_bytes()[4].to_ascii_lowercase() as char)?;
            if matches!(k, PieceKind::Pawn | PieceKind::King) {
                return None;
            }
            Some(k)
        } else {
            None
        };
        Some(Move { from, to, promotion })
    }
}

impl fmt::Display for Move {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_uci())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct CastlingRights {
    pub white_king: bool,
    pub white_queen: bool,
    pub black_king: bool,
    pub black_queen: bool,
}

impl CastlingRights {
    pub fn all() -> Self {
        CastlingRights { white_king: true, white_queen: true, black_king: true, black_queen: true }
    }

    pub fn to_fen_field(&self) -> String {
        let mut s = String::new();
        if self.white_king {
            s.push('K');
        }
        if self.white_queen {
            s.push('Q');
        }
        if self.black_king {
            s.push('k');
        }
        if self.black_queen {
            s.push('q');
        }
        if s.is_empty() {
            s.push('-');
        }
        s
    }

    pub fn from_fen_field(s: &str) -> Option<Self> {
        let mut r = CastlingRights::default();
        if s == "-" {
            return Some(r);
        }
        if s.is_empty() {
            return None;
        }
        for c in s.chars() {
            match c {
                'K' => r.white_king = true,
                'Q' => r.white_queen = true,
                'k' => r.black_king = true,
                'q' => r.black_queen = true,
                _ => return None,
            }
        }
        Some(r)
    }
}
