use cea::chess::*;

fn pos(fen: &str) -> Position {
    Position::from_fen(fen).unwrap()
}

#[test]
fn fen_roundtrip_preserves_all_fields() {
    for fen in [
        Position::START_FEN,
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq e6 0 2",
        "8/8/8/8/8/8/8/K6k b - - 37 91",
    ] {
        assert_eq!(pos(fen).to_fen(), fen);
    }
}

#[test]
fn fen_missing_counters_default() {
    let p = pos("8/8/8/8/8/8/8/K6k w - -");
    assert_eq!((p.halfmove_clock, p.fullmove_number), (0, 1));
}

#[test]
fn fen_syntax_errors() {
    assert!(matches!(Position::from_fen("nonsense"), Err(FenError::FieldCount(1))));
    assert!(matches!(Position::from_fen("8/8/8/8/8/8/8 w - - 0 1"), Err(FenError::RankCount(7))));
    assert!(matches!(Position::from_fen("9/8/8/8/8/8/8/8 w - - 0 1"), Err(FenError::InvalidPiece('9'))));
    assert!(matches!(Position::from_fen("7/8/8/8/8/8/8/8 w - - 0 1"), Err(FenError::RankLength(8))));
    assert!(matches!(Position::from_fen("8/8/8/8/8/8/8/8 x - - 0 1"), Err(FenError::InvalidSide(_))));
    assert!(matches!(Position::from_fen("8/8/8/8/8/8/8/8 w KX - 0 1"), Err(FenError::InvalidCastling(_))));
    assert!(matches!(Position::from_fen("8/8/8/8/8/8/8/8 w - z9 0 1"), Err(FenError::InvalidEnPassant(_))));
    assert!(matches!(Position::from_fen("8/8/8/8/8/8/8/8 w - - x 1"), Err(FenError::InvalidNumber(_))));
    assert!(matches!(Position::from_fen("ppppppppp/8/8/8/8/8/8/8 w - - 0 1"), Err(FenError::RankLength(8))));
}

#[test]
fn validation_catches_semantic_problems() {
    assert!(Position::starting().validate().is_empty());
    let no_kings = pos("8/8/8/8/8/8/8/8 w - - 0 1");
    assert_eq!(no_kings.validate().len(), 2);
    let pawn_back = pos("P3k3/8/8/8/8/8/8/4K3 w - - 0 1");
    assert!(pawn_back.validate().contains(&PositionIssue::PawnOnBackRank));
    // Black king in check while White is to move is fine; White king in check with White to move... also fine.
    // But the side NOT to move being in check is impossible:
    let impossible = pos("4k3/8/8/8/8/8/4R3/4K3 w - - 0 1");
    assert!(impossible.validate().contains(&PositionIssue::OpponentKingInCheck));
    let bad_castle = pos("4k3/8/8/8/8/8/8/4K3 w KQ - 0 1");
    assert_eq!(bad_castle.validate().len(), 2);
    let bad_ep = pos("4k3/8/8/8/8/8/8/4K3 w - e6 0 1");
    assert!(matches!(bad_ep.validate()[0], PositionIssue::BadEnPassant(_)));
    let good_ep = pos("rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq e6 0 2");
    assert!(good_ep.validate().is_empty());
}

#[test]
fn perft_start_position() {
    let p = Position::starting();
    assert_eq!(p.perft(1), 20);
    assert_eq!(p.perft(2), 400);
    assert_eq!(p.perft(3), 8902);
    assert_eq!(p.perft(4), 197_281);
}

#[test]
fn perft_kiwipete_covers_castling_ep_promotion_pins() {
    let p = pos("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1");
    assert_eq!(p.perft(1), 48);
    assert_eq!(p.perft(2), 2039);
    assert_eq!(p.perft(3), 97_862);
}

#[test]
fn perft_endgame_en_passant_pins() {
    let p = pos("8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1");
    assert_eq!(p.perft(1), 14);
    assert_eq!(p.perft(2), 191);
    assert_eq!(p.perft(3), 2812);
    assert_eq!(p.perft(4), 43_238);
}

#[test]
fn perft_promotion_heavy_positions() {
    let p4 = pos("r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1");
    assert_eq!(p4.perft(1), 6);
    assert_eq!(p4.perft(2), 264);
    assert_eq!(p4.perft(3), 9467);
    let p5 = pos("rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8");
    assert_eq!(p5.perft(1), 44);
    assert_eq!(p5.perft(2), 1486);
    assert_eq!(p5.perft(3), 62_379);
}

#[test]
fn uci_move_parsing_and_legality() {
    let p = Position::starting();
    assert_eq!(p.parse_uci_move("g1f3").unwrap().to_uci(), "g1f3");
    assert_eq!(p.parse_uci_move("e2e5"), Err(MoveError::Illegal));
    assert_eq!(p.parse_uci_move("zz"), Err(MoveError::BadSyntax));
    assert_eq!(p.parse_uci_move("e7e8k"), Err(MoveError::BadSyntax));
}

#[test]
fn san_basic_and_disambiguation() {
    let p = Position::starting();
    let san = |p: &Position, u: &str| p.to_san(&p.parse_uci_move(u).unwrap());
    assert_eq!(san(&p, "g1f3"), "Nf3");
    assert_eq!(san(&p, "e2e4"), "e4");
    // Two knights can reach d2: file disambiguation.
    let p = pos("4k3/8/8/8/8/5N2/8/1N2K3 w - - 0 1");
    assert_eq!(san(&p, "b1d2"), "Nbd2");
    assert_eq!(san(&p, "f3d2"), "Nfd2");
    // Two rooks on the same file: rank disambiguation.
    let p = pos("4k3/8/R7/8/8/8/8/R3K3 w - - 0 1");
    assert_eq!(san(&p, "a1a3"), "R1a3");
    assert_eq!(san(&p, "a6a3"), "R6a3");
}

#[test]
fn san_captures_castling_promotion_ep_and_check() {
    let san = |p: &Position, u: &str| p.to_san(&p.parse_uci_move(u).unwrap());
    let p = pos("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
    assert_eq!(san(&p, "e1g1"), "O-O");
    assert_eq!(san(&p, "e1c1"), "O-O-O");
    let p = pos("4k3/P7/8/8/8/8/8/4K3 w - - 0 1");
    assert_eq!(san(&p, "a7a8q"), "a8=Q+");
    assert_eq!(san(&p, "a7a8n"), "a8=N");
    let p = pos("rnbqkbnr/pppp1ppp/8/4pP2/8/8/PPPPP1PP/RNBQKBNR w KQkq e6 0 3");
    assert_eq!(san(&p, "f5e6"), "fxe6");
    let p = pos("rnbqkbnr/ppp2ppp/8/3pp3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 3");
    assert_eq!(san(&p, "e4d5"), "exd5");
    // Fool's mate
    let p = pos("rnbqkbnr/pppp1ppp/8/4p3/6P1/5P2/PPPPP2P/RNBQKBNR b KQkq - 0 2");
    assert_eq!(san(&p, "d8h4"), "Qh4#");
}

#[test]
fn en_passant_capture_removes_pawn_and_updates_fen() {
    let p = pos("rnbqkbnr/pppp1ppp/8/4pP2/8/8/PPPPP1PP/RNBQKBNR w KQkq e6 0 3");
    let after = p.make_move(&p.parse_uci_move("f5e6").unwrap());
    assert_eq!(after.to_fen(), "rnbqkbnr/pppp1ppp/4P3/8/8/8/PPPPP1PP/RNBQKBNR b KQkq - 0 3");
}

#[test]
fn checkmate_and_stalemate_detection() {
    let mate = pos("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3");
    assert!(mate.is_checkmate());
    let stale = pos("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1");
    assert!(stale.is_stalemate());
}

#[test]
fn line_to_san_stops_on_illegal_move() {
    let p = Position::starting();
    let line: Vec<String> = ["e2e4", "e7e5", "g1f3", "a1a8"].iter().map(|s| s.to_string()).collect();
    assert_eq!(p.line_to_san(&line), vec!["e4", "e5", "Nf3"]);
}
