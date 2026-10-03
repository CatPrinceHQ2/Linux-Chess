//! Full chess position (all six FEN fields), FEN I/O, validation and legal move generation.
use super::types::*;
use std::fmt;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Position {
    pub board: [Option<Piece>; 64],
    pub side_to_move: Color,
    pub castling: CastlingRights,
    pub en_passant: Option<Square>,
    pub halfmove_clock: u32,
    pub fullmove_number: u32,
}

/// Syntax-level FEN problems.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum FenError {
    FieldCount(usize),
    RankCount(usize),
    RankLength(u8),
    InvalidPiece(char),
    InvalidSide(String),
    InvalidCastling(String),
    InvalidEnPassant(String),
    InvalidNumber(String),
}

impl fmt::Display for FenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FenError::FieldCount(n) => write!(f, "FEN must have 4 to 6 space-separated fields (found {n})"),
            FenError::RankCount(n) => write!(f, "FEN piece placement must have 8 ranks (found {n})"),
            FenError::RankLength(r) => write!(f, "Rank {r} does not describe exactly 8 squares"),
            FenError::InvalidPiece(c) => write!(f, "Unknown piece character '{c}'"),
            FenError::InvalidSide(s) => write!(f, "Side to move must be 'w' or 'b' (found '{s}')"),
            FenError::InvalidCastling(s) => write!(f, "Invalid castling field '{s}'"),
            FenError::InvalidEnPassant(s) => write!(f, "Invalid en-passant field '{s}'"),
            FenError::InvalidNumber(s) => write!(f, "Invalid move counter '{s}'"),
        }
    }
}
impl std::error::Error for FenError {}

/// Semantic problems with an otherwise well-formed position.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PositionIssue {
    KingCount { color: Color, count: usize },
    PawnOnBackRank,
    TooManyPawns(Color),
    TooManyPieces(Color),
    OpponentKingInCheck,
    CastlingRightWithoutPieces(&'static str),
    BadEnPassant(String),
}

impl fmt::Display for PositionIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PositionIssue::KingCount { color, count } => {
                write!(f, "{} must have exactly one king (found {count})", color.name())
            }
            PositionIssue::PawnOnBackRank => write!(f, "Pawns cannot stand on the first or eighth rank"),
            PositionIssue::TooManyPawns(c) => write!(f, "{} has more than 8 pawns", c.name()),
            PositionIssue::TooManyPieces(c) => write!(f, "{} has more than 16 pieces", c.name()),
            PositionIssue::OpponentKingInCheck => {
                write!(f, "The side that is not to move is in check, which is impossible")
            }
            PositionIssue::CastlingRightWithoutPieces(w) => {
                write!(f, "Castling right {w} is set but the king or rook is not on its home square")
            }
            PositionIssue::BadEnPassant(s) => write!(f, "En-passant square is inconsistent: {s}"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MoveError {
    BadSyntax,
    Illegal,
}

impl fmt::Display for MoveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MoveError::BadSyntax => write!(f, "Move is not valid coordinate notation"),
            MoveError::Illegal => write!(f, "Move is not legal in this position"),
        }
    }
}
impl std::error::Error for MoveError {}

const KNIGHT_OFFSETS: [(i8, i8); 8] = [(1, 2), (2, 1), (2, -1), (1, -2), (-1, -2), (-2, -1), (-2, 1), (-1, 2)];
const KING_OFFSETS: [(i8, i8); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];
const ROOK_DIRS: [(i8, i8); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
const BISHOP_DIRS: [(i8, i8); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];

impl Position {
    pub const START_FEN: &'static str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    pub fn empty() -> Position {
        Position {
            board: [None; 64],
            side_to_move: Color::White,
            castling: CastlingRights::default(),
            en_passant: None,
            halfmove_clock: 0,
            fullmove_number: 1,
        }
    }

    pub fn starting() -> Position {
        Position::from_fen(Self::START_FEN).expect("start FEN is valid")
    }

    pub fn piece_at(&self, s: Square) -> Option<Piece> {
        self.board[s as usize]
    }

    pub fn set_piece(&mut self, s: Square, p: Option<Piece>) {
        self.board[s as usize] = p;
    }

    // ---------------------------------------------------------------- FEN

    pub fn from_fen(fen: &str) -> Result<Position, FenError> {
        let fields: Vec<&str> = fen.split_whitespace().collect();
        if !(4..=6).contains(&fields.len()) {
            return Err(FenError::FieldCount(fields.len()));
        }
        let ranks: Vec<&str> = fields[0].split('/').collect();
        if ranks.len() != 8 {
            return Err(FenError::RankCount(ranks.len()));
        }
        let mut pos = Position::empty();
        for (i, rank_str) in ranks.iter().enumerate() {
            let rank = 7 - i as u8;
            let mut file: u32 = 0;
            for c in rank_str.chars() {
                if let Some(d) = c.to_digit(10) {
                    if d == 0 || d > 8 {
                        return Err(FenError::InvalidPiece(c));
                    }
                    file += d;
                } else if let Some(p) = Piece::from_fen_char(c) {
                    if file >= 8 {
                        return Err(FenError::RankLength(rank + 1));
                    }
                    pos.board[square(file as u8, rank) as usize] = Some(p);
                    file += 1;
                } else {
                    return Err(FenError::InvalidPiece(c));
                }
            }
            if file != 8 {
                return Err(FenError::RankLength(rank + 1));
            }
        }
        pos.side_to_move = match fields[1] {
            "w" => Color::White,
            "b" => Color::Black,
            other => return Err(FenError::InvalidSide(other.to_string())),
        };
        pos.castling = CastlingRights::from_fen_field(fields[2])
            .ok_or_else(|| FenError::InvalidCastling(fields[2].to_string()))?;
        pos.en_passant = if fields[3] == "-" {
            None
        } else {
            let sq = parse_square(fields[3]).ok_or_else(|| FenError::InvalidEnPassant(fields[3].to_string()))?;
            Some(sq)
        };
        pos.halfmove_clock = match fields.get(4) {
            Some(s) => s.parse().map_err(|_| FenError::InvalidNumber(s.to_string()))?,
            None => 0,
        };
        pos.fullmove_number = match fields.get(5) {
            Some(s) => s.parse::<u32>().map_err(|_| FenError::InvalidNumber(s.to_string()))?.max(1),
            None => 1,
        };
        Ok(pos)
    }

    pub fn to_fen(&self) -> String {
        let mut s = String::new();
        for rank in (0..8u8).rev() {
            let mut empty = 0;
            for file in 0..8u8 {
                match self.board[square(file, rank) as usize] {
                    Some(p) => {
                        if empty > 0 {
                            s.push_str(&empty.to_string());
                            empty = 0;
                        }
                        s.push(p.fen_char());
                    }
                    None => empty += 1,
                }
            }
            if empty > 0 {
                s.push_str(&empty.to_string());
            }
            if rank > 0 {
                s.push('/');
            }
        }
        let side = if self.side_to_move == Color::White { 'w' } else { 'b' };
        let ep = self.en_passant.map(square_name).unwrap_or_else(|| "-".to_string());
        format!(
            "{s} {side} {} {ep} {} {}",
            self.castling.to_fen_field(),
            self.halfmove_clock,
            self.fullmove_number
        )
    }

    // ------------------------------------------------------- validation

    /// Returns every semantic problem found (empty = position is legal to analyse).
    pub fn validate(&self) -> Vec<PositionIssue> {
        let mut issues = Vec::new();
        for color in [Color::White, Color::Black] {
            let kings = self.count(Piece::new(color, PieceKind::King));
            if kings != 1 {
                issues.push(PositionIssue::KingCount { color, count: kings });
            }
            if self.count(Piece::new(color, PieceKind::Pawn)) > 8 {
                issues.push(PositionIssue::TooManyPawns(color));
            }
            let total = self.board.iter().flatten().filter(|p| p.color == color).count();
            if total > 16 {
                issues.push(PositionIssue::TooManyPieces(color));
            }
        }
        for s in 0..64u8 {
            if let Some(p) = self.board[s as usize] {
                if p.kind == PieceKind::Pawn && (rank_of(s) == 0 || rank_of(s) == 7) {
                    issues.push(PositionIssue::PawnOnBackRank);
                    break;
                }
            }
        }
        let wk = Piece::new(Color::White, PieceKind::King);
        let bk = Piece::new(Color::Black, PieceKind::King);
        let wr = Piece::new(Color::White, PieceKind::Rook);
        let br = Piece::new(Color::Black, PieceKind::Rook);
        let c = self.castling;
        let at = |s: &str| self.board[parse_square(s).unwrap() as usize];
        if c.white_king && !(at("e1") == Some(wk) && at("h1") == Some(wr)) {
            issues.push(PositionIssue::CastlingRightWithoutPieces("K"));
        }
        if c.white_queen && !(at("e1") == Some(wk) && at("a1") == Some(wr)) {
            issues.push(PositionIssue::CastlingRightWithoutPieces("Q"));
        }
        if c.black_king && !(at("e8") == Some(bk) && at("h8") == Some(br)) {
            issues.push(PositionIssue::CastlingRightWithoutPieces("k"));
        }
        if c.black_queen && !(at("e8") == Some(bk) && at("a8") == Some(br)) {
            issues.push(PositionIssue::CastlingRightWithoutPieces("q"));
        }
        if let Some(ep) = self.en_passant {
            let (want_rank, pawn_rank, pawn_color) = match self.side_to_move {
                Color::White => (5u8, 4u8, Color::Black),
                Color::Black => (2u8, 3u8, Color::White),
            };
            let behind = square(file_of(ep), pawn_rank);
            let origin_rank = if self.side_to_move == Color::White { 6 } else { 1 };
            let origin = square(file_of(ep), origin_rank);
            if rank_of(ep) != want_rank {
                issues.push(PositionIssue::BadEnPassant(format!(
                    "{} is on the wrong rank for {} to move",
                    square_name(ep),
                    self.side_to_move.name()
                )));
            } else if self.board[behind as usize] != Some(Piece::new(pawn_color, PieceKind::Pawn)) {
                issues.push(PositionIssue::BadEnPassant("no pawn that could have just double-stepped".into()));
            } else if self.board[ep as usize].is_some() || self.board[origin as usize].is_some() {
                issues.push(PositionIssue::BadEnPassant("the squares the pawn passed over are not empty".into()));
            }
        }
        // Only meaningful if both kings exist.
        if self.king_square(Color::White).is_some() && self.king_square(Color::Black).is_some() {
            let waiting = self.side_to_move.opposite();
            if self.in_check(waiting) {
                issues.push(PositionIssue::OpponentKingInCheck);
            }
        }
        issues
    }

    fn count(&self, p: Piece) -> usize {
        self.board.iter().filter(|x| **x == Some(p)).count()
    }

    // ------------------------------------------------------ attack logic

    pub fn king_square(&self, color: Color) -> Option<Square> {
        (0..64u8).find(|&s| self.board[s as usize] == Some(Piece::new(color, PieceKind::King)))
    }

    fn at(&self, f: i8, r: i8) -> Option<Piece> {
        if (0..8).contains(&f) && (0..8).contains(&r) {
            self.board[square(f as u8, r as u8) as usize]
        } else {
            None
        }
    }

    /// Is `target` attacked by any piece of colour `by`?
    pub fn is_attacked(&self, target: Square, by: Color) -> bool {
        let tf = file_of(target) as i8;
        let tr = rank_of(target) as i8;
        let pawn_dir: i8 = if by == Color::White { 1 } else { -1 };
        for df in [-1i8, 1] {
            if self.at(tf + df, tr - pawn_dir) == Some(Piece::new(by, PieceKind::Pawn)) {
                return true;
            }
        }
        for (df, dr) in KNIGHT_OFFSETS {
            if self.at(tf + df, tr + dr) == Some(Piece::new(by, PieceKind::Knight)) {
                return true;
            }
        }
        for (df, dr) in KING_OFFSETS {
            if self.at(tf + df, tr + dr) == Some(Piece::new(by, PieceKind::King)) {
                return true;
            }
        }
        let slide = |dirs: &[(i8, i8)], a: PieceKind, b: PieceKind| -> bool {
            for &(df, dr) in dirs {
                let (mut f, mut r) = (tf + df, tr + dr);
                while (0..8).contains(&f) && (0..8).contains(&r) {
                    if let Some(p) = self.board[square(f as u8, r as u8) as usize] {
                        if p.color == by && (p.kind == a || p.kind == b) {
                            return true;
                        }
                        break;
                    }
                    f += df;
                    r += dr;
                }
            }
            false
        };
        slide(&ROOK_DIRS, PieceKind::Rook, PieceKind::Queen) || slide(&BISHOP_DIRS, PieceKind::Bishop, PieceKind::Queen)
    }

    pub fn in_check(&self, color: Color) -> bool {
        match self.king_square(color) {
            Some(k) => self.is_attacked(k, color.opposite()),
            None => false,
        }
    }

    /// Is the side to move currently in check?
    pub fn is_check(&self) -> bool {
        self.in_check(self.side_to_move)
    }

    // ----------------------------------------------------- move generation

    fn pseudo_legal_moves(&self) -> Vec<Move> {
        let mut moves = Vec::with_capacity(64);
        let us = self.side_to_move;
        for s in 0..64u8 {
            let Some(p) = self.board[s as usize] else { continue };
            if p.color != us {
                continue;
            }
            let f = file_of(s) as i8;
            let r = rank_of(s) as i8;
            match p.kind {
                PieceKind::Pawn => {
                    let dir: i8 = if us == Color::White { 1 } else { -1 };
                    let start = if us == Color::White { 1 } else { 6 };
                    let promo_rank = if us == Color::White { 7 } else { 0 };
                    let nr = r + dir;
                    if !(0..8).contains(&nr) {
                        continue;
                    }
                    let fwd = square(f as u8, nr as u8);
                    if self.board[fwd as usize].is_none() {
                        push_pawn(&mut moves, s, fwd, nr == promo_rank);
                        if r == start {
                            let two = square(f as u8, (r + 2 * dir) as u8);
                            if self.board[two as usize].is_none() {
                                moves.push(Move::new(s, two));
                            }
                        }
                    }
                    for df in [-1i8, 1] {
                        let nf = f + df;
                        if !(0..8).contains(&nf) {
                            continue;
                        }
                        let t = square(nf as u8, nr as u8);
                        match self.board[t as usize] {
                            Some(o) if o.color != us => push_pawn(&mut moves, s, t, nr == promo_rank),
                            None if Some(t) == self.en_passant => moves.push(Move::new(s, t)),
                            _ => {}
                        }
                    }
                }
                PieceKind::Knight | PieceKind::King => {
                    let offs: &[(i8, i8)] =
                        if p.kind == PieceKind::Knight { &KNIGHT_OFFSETS } else { &KING_OFFSETS };
                    for &(df, dr) in offs {
                        let (nf, nr) = (f + df, r + dr);
                        if !(0..8).contains(&nf) || !(0..8).contains(&nr) {
                            continue;
                        }
                        let t = square(nf as u8, nr as u8);
                        match self.board[t as usize] {
                            Some(o) if o.color == us => {}
                            _ => moves.push(Move::new(s, t)),
                        }
                    }
                }
                PieceKind::Bishop | PieceKind::Rook | PieceKind::Queen => {
                    let mut dirs: Vec<(i8, i8)> = Vec::new();
                    if p.kind != PieceKind::Bishop {
                        dirs.extend_from_slice(&ROOK_DIRS);
                    }
                    if p.kind != PieceKind::Rook {
                        dirs.extend_from_slice(&BISHOP_DIRS);
                    }
                    for (df, dr) in dirs {
                        let (mut nf, mut nr) = (f + df, r + dr);
                        while (0..8).contains(&nf) && (0..8).contains(&nr) {
                            let t = square(nf as u8, nr as u8);
                            match self.board[t as usize] {
                                None => moves.push(Move::new(s, t)),
                                Some(o) => {
                                    if o.color != us {
                                        moves.push(Move::new(s, t));
                                    }
                                    break;
                                }
                            }
                            nf += df;
                            nr += dr;
                        }
                    }
                }
            }
        }
        self.castling_moves(&mut moves);
        moves
    }

    fn castling_moves(&self, moves: &mut Vec<Move>) {
        let us = self.side_to_move;
        let them = us.opposite();
        let (rank, ks, qs) = match us {
            Color::White => (0u8, self.castling.white_king, self.castling.white_queen),
            Color::Black => (7u8, self.castling.black_king, self.castling.black_queen),
        };
        let e = square(4, rank);
        if !(ks || qs) || self.board[e as usize] != Some(Piece::new(us, PieceKind::King)) {
            return;
        }
        if self.is_attacked(e, them) {
            return;
        }
        let rook = Some(Piece::new(us, PieceKind::Rook));
        let empty = |f: u8| self.board[square(f, rank) as usize].is_none();
        if ks && empty(5) && empty(6) && self.board[square(7, rank) as usize] == rook && !self.is_attacked(square(5, rank), them) {
            moves.push(Move::new(e, square(6, rank)));
        }
        if qs
            && empty(1)
            && empty(2)
            && empty(3)
            && self.board[square(0, rank) as usize] == rook
            && !self.is_attacked(square(3, rank), them)
        {
            moves.push(Move::new(e, square(2, rank)));
        }
    }

    pub fn legal_moves(&self) -> Vec<Move> {
        let us = self.side_to_move;
        self.pseudo_legal_moves()
            .into_iter()
            .filter(|m| !self.make_move(m).in_check(us))
            .collect()
    }

    /// Legal moves that start on `from` (used for legal-move highlighting).
    pub fn legal_moves_from(&self, from: Square) -> Vec<Move> {
        self.legal_moves().into_iter().filter(|m| m.from == from).collect()
    }

    pub fn is_checkmate(&self) -> bool {
        self.is_check() && self.legal_moves().is_empty()
    }

    pub fn is_stalemate(&self) -> bool {
        !self.is_check() && self.legal_moves().is_empty()
    }

    /// Applies a move that is assumed to be (pseudo-)legal and returns the new position.
    pub fn make_move(&self, m: &Move) -> Position {
        let mut p = self.clone();
        let Some(piece) = p.board[m.from as usize] else { return p };
        let captured = p.board[m.to as usize];
        let (ff, tf) = (file_of(m.from), file_of(m.to));

        // En passant capture.
        if piece.kind == PieceKind::Pawn && ff != tf && captured.is_none() && Some(m.to) == self.en_passant {
            p.board[square(tf, rank_of(m.from)) as usize] = None;
        }
        // Castling: move the rook too.
        if piece.kind == PieceKind::King && (ff as i8 - tf as i8).abs() == 2 {
            let rank = rank_of(m.from);
            let (rook_from, rook_to) = if tf > ff { (7, 5) } else { (0, 3) };
            p.board[square(rook_to, rank) as usize] = p.board[square(rook_from, rank) as usize].take();
        }
        p.board[m.to as usize] = Some(match m.promotion {
            Some(k) if piece.kind == PieceKind::Pawn => Piece::new(piece.color, k),
            _ => piece,
        });
        p.board[m.from as usize] = None;

        for s in [m.from, m.to] {
            match s {
                4 => {
                    p.castling.white_king = false;
                    p.castling.white_queen = false;
                }
                60 => {
                    p.castling.black_king = false;
                    p.castling.black_queen = false;
                }
                0 => p.castling.white_queen = false,
                7 => p.castling.white_king = false,
                56 => p.castling.black_queen = false,
                63 => p.castling.black_king = false,
                _ => {}
            }
        }

        p.en_passant = if piece.kind == PieceKind::Pawn && (rank_of(m.from) as i8 - rank_of(m.to) as i8).abs() == 2 {
            Some(square(ff, (rank_of(m.from) + rank_of(m.to)) / 2))
        } else {
            None
        };
        p.halfmove_clock =
            if piece.kind == PieceKind::Pawn || captured.is_some() { 0 } else { self.halfmove_clock + 1 };
        if self.side_to_move == Color::Black {
            p.fullmove_number += 1;
        }
        p.side_to_move = self.side_to_move.opposite();
        p
    }

    /// Parse a coordinate-notation move and make sure it is legal here.
    pub fn parse_uci_move(&self, s: &str) -> Result<Move, MoveError> {
        let m = Move::from_uci_str(s).ok_or(MoveError::BadSyntax)?;
        self.legal_moves().into_iter().find(|l| *l == m).ok_or(MoveError::Illegal)
    }

    /// Number of leaf nodes at `depth` (used by tests to verify the generator).
    pub fn perft(&self, depth: u32) -> u64 {
        if depth == 0 {
            return 1;
        }
        let moves = self.legal_moves();
        if depth == 1 {
            return moves.len() as u64;
        }
        moves.iter().map(|m| self.make_move(m).perft(depth - 1)).sum()
    }
}

fn push_pawn(moves: &mut Vec<Move>, from: Square, to: Square, promote: bool) {
    if promote {
        for k in [PieceKind::Queen, PieceKind::Rook, PieceKind::Bishop, PieceKind::Knight] {
            moves.push(Move::with_promotion(from, to, k));
        }
    } else {
        moves.push(Move::new(from, to));
    }
}
