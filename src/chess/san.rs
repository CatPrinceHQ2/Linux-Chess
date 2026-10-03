//! Standard Algebraic Notation generation, computed from the actual position.
use super::position::Position;
use super::types::*;

impl Position {
    /// SAN for a legal move in this position, including `+` / `#` suffixes.
    pub fn to_san(&self, m: &Move) -> String {
        let Some(piece) = self.piece_at(m.from) else { return m.to_uci() };
        let (ff, tf) = (file_of(m.from), file_of(m.to));
        let mut san = String::new();

        if piece.kind == PieceKind::King && (ff as i8 - tf as i8).abs() == 2 {
            san.push_str(if tf > ff { "O-O" } else { "O-O-O" });
        } else {
            let is_capture = self.piece_at(m.to).is_some()
                || (piece.kind == PieceKind::Pawn && ff != tf && Some(m.to) == self.en_passant);
            if piece.kind == PieceKind::Pawn {
                if is_capture {
                    san.push((b'a' + ff) as char);
                    san.push('x');
                }
            } else {
                san.push_str(piece.kind.san_letter());
                let others: Vec<Move> = self
                    .legal_moves()
                    .into_iter()
                    .filter(|o| {
                        o.to == m.to
                            && o.from != m.from
                            && self.piece_at(o.from).map(|p| p.kind) == Some(piece.kind)
                    })
                    .collect();
                if !others.is_empty() {
                    let same_file = others.iter().any(|o| file_of(o.from) == ff);
                    let same_rank = others.iter().any(|o| rank_of(o.from) == rank_of(m.from));
                    if !same_file {
                        san.push((b'a' + ff) as char);
                    } else if !same_rank {
                        san.push((b'1' + rank_of(m.from)) as char);
                    } else {
                        san.push_str(&square_name(m.from));
                    }
                }
                if is_capture {
                    san.push('x');
                }
            }
            san.push_str(&square_name(m.to));
            if let Some(k) = m.promotion {
                san.push('=');
                san.push_str(&k.lower_char().to_ascii_uppercase().to_string());
            }
        }

        let after = self.make_move(m);
        if after.is_check() {
            san.push(if after.legal_moves().is_empty() { '#' } else { '+' });
        }
        san
    }

    /// Convert a whole line of UCI moves to SAN, stopping at the first move that is
    /// not legal (engines can occasionally report truncated or odd lines).
    pub fn line_to_san(&self, uci_moves: &[String]) -> Vec<String> {
        let mut pos = self.clone();
        let mut out = Vec::new();
        for u in uci_moves {
            match pos.parse_uci_move(u) {
                Ok(m) => {
                    out.push(pos.to_san(&m));
                    pos = pos.make_move(&m);
                }
                Err(_) => break,
            }
        }
        out
    }
}
