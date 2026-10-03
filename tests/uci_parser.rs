use cea::engine::options::{Capabilities, EngineOption, OptionKind};
use cea::engine::parser::*;

fn opt(line: &str) -> EngineOption {
    match parse_line(line) {
        UciMessage::Option(o) => o,
        other => panic!("not an option: {other:?}"),
    }
}

fn info(line: &str) -> Info {
    match parse_line(line) {
        UciMessage::Info(i) => i,
        other => panic!("not info: {other:?}"),
    }
}

#[test]
fn parses_all_five_option_types() {
    assert_eq!(
        opt("option name Threads type spin default 1 min 1 max 512"),
        EngineOption { name: "Threads".into(), kind: OptionKind::Spin { default: 1, min: 1, max: 512 } }
    );
    assert_eq!(
        opt("option name Use NNUE type check default true"),
        EngineOption { name: "Use NNUE".into(), kind: OptionKind::Check { default: true } }
    );
    assert_eq!(
        opt("option name Style type combo default Normal var Normal var Aggressive var Defensive"),
        EngineOption {
            name: "Style".into(),
            kind: OptionKind::Combo { default: "Normal".into(), choices: vec!["Normal".into(), "Aggressive".into(), "Defensive".into()] }
        }
    );
    assert_eq!(
        opt("option name EvalFile type string default nn-1c0000000000.nnue"),
        EngineOption { name: "EvalFile".into(), kind: OptionKind::Str { default: "nn-1c0000000000.nnue".into() } }
    );
    assert_eq!(opt("option name Clear Hash type button"), EngineOption { name: "Clear Hash".into(), kind: OptionKind::Button });
}

#[test]
fn unusual_option_shapes() {
    // negative range
    let o = opt("option name Contempt type spin default -5 min -100 max 100");
    assert_eq!(o.kind, OptionKind::Spin { default: -5, min: -100, max: 100 });
    // multi-word names and values, including words that look like keywords inside a combo choice
    let o = opt("option name Playing Style type combo default Very Sharp var Very Sharp var Calm Down");
    assert_eq!(o.name, "Playing Style");
    assert_eq!(o.kind, OptionKind::Combo { default: "Very Sharp".into(), choices: vec!["Very Sharp".into(), "Calm Down".into()] });
    // empty string default
    let o = opt("option name SyzygyPath type string default <empty>");
    assert_eq!(o.kind, OptionKind::Str { default: "<empty>".into() });
    let o = opt("option name Book File type string default");
    assert_eq!(o.kind, OptionKind::Str { default: String::new() });
    // extra whitespace and CRLF
    let o = opt("option   name   Hash   type   spin   default  16  min  1  max  33554432\r");
    assert_eq!(o.kind, OptionKind::Spin { default: 16, min: 1, max: 33554432 });
    // unknown type is not an option we can show
    assert!(matches!(parse_line("option name X type quantum default 1"), UciMessage::Unknown(_)));
    assert!(matches!(parse_line("option nonsense"), UciMessage::Unknown(_)));
}

#[test]
fn parses_id_and_handshake_lines() {
    assert_eq!(parse_line("id name Stockfish 16"), UciMessage::IdName("Stockfish 16".into()));
    assert_eq!(parse_line("id author the Stockfish developers (see AUTHORS file)"), UciMessage::IdAuthor("the Stockfish developers (see AUTHORS file)".into()));
    assert_eq!(parse_line("uciok"), UciMessage::UciOk);
    assert_eq!(parse_line("readyok"), UciMessage::ReadyOk);
    assert!(matches!(parse_line("Stockfish 16 by the Stockfish developers"), UciMessage::Unknown(_)));
    assert!(matches!(parse_line(""), UciMessage::Unknown(_)));
}

#[test]
fn parses_full_info_line() {
    let i = info("info depth 24 seldepth 31 multipv 2 score cp 82 nodes 14200000 nps 3100000 hashfull 331 tbhits 0 time 4800 pv g1f3 d7d5 g2g3 c7c5 f1g2");
    assert_eq!(i.depth, Some(24));
    assert_eq!(i.seldepth, Some(31));
    assert_eq!(i.multipv, Some(2));
    assert_eq!(i.score, Some(Score::Cp { value: 82, bound: Bound::Exact }));
    assert_eq!(i.nodes, Some(14_200_000));
    assert_eq!(i.nps, Some(3_100_000));
    assert_eq!(i.hashfull, Some(331));
    assert_eq!(i.time_ms, Some(4800));
    assert_eq!(i.pv, vec!["g1f3", "d7d5", "g2g3", "c7c5", "f1g2"]);
}

#[test]
fn info_tolerates_missing_unknown_and_reordered_fields() {
    let i = info("info score cp -15 depth 3");
    assert_eq!((i.depth, i.score), (Some(3), Some(Score::Cp { value: -15, bound: Bound::Exact })));
    assert!(i.nodes.is_none() && i.pv.is_empty());
    let i = info("info depth 7 wdl 400 500 100 shiny new thing 12 nodes 99 pv e2e4");
    assert_eq!((i.depth, i.nodes), (Some(7), Some(99)));
    assert_eq!(i.pv, vec!["e2e4"]);
    let i = info("info depth abc nodes");
    assert_eq!((i.depth, i.nodes), (None, None));
    let i = info("info");
    assert_eq!(i, Info::default());
}

#[test]
fn info_scores_bounds_and_mates() {
    assert_eq!(info("info score cp 30 lowerbound").score, Some(Score::Cp { value: 30, bound: Bound::Lower }));
    assert_eq!(info("info score cp 30 upperbound nodes 5").score, Some(Score::Cp { value: 30, bound: Bound::Upper }));
    assert_eq!(info("info score mate 3").score, Some(Score::Mate { moves: 3, bound: Bound::Exact }));
    assert_eq!(info("info score mate -2 nodes 9").score, Some(Score::Mate { moves: -2, bound: Bound::Exact }));
    assert_eq!(info("info score banana 3").score, None);
}

#[test]
fn info_string_currmove_and_promotion_pv() {
    let i = info("info string NNUE evaluation using nn-abc.nnue enabled");
    assert_eq!(i.string.as_deref(), Some("NNUE evaluation using nn-abc.nnue enabled"));
    let i = info("info depth 5 currmove e2e4 currmovenumber 1");
    assert_eq!((i.currmove.as_deref(), i.currmovenumber), (Some("e2e4"), Some(1)));
    let i = info("info depth 9 pv a7a8q e8d7 a8h8 string trailing");
    assert_eq!(i.pv, vec!["a7a8q", "e8d7", "a8h8"]);
}

#[test]
fn parses_bestmove_variants() {
    assert_eq!(parse_line("bestmove g1f3"), UciMessage::BestMove { best: Some("g1f3".into()), ponder: None });
    assert_eq!(parse_line("bestmove g1f3 ponder d7d5"), UciMessage::BestMove { best: Some("g1f3".into()), ponder: Some("d7d5".into()) });
    assert_eq!(parse_line("bestmove (none)"), UciMessage::BestMove { best: None, ponder: None });
    assert_eq!(parse_line("bestmove 0000"), UciMessage::BestMove { best: None, ponder: None });
    assert!(matches!(parse_line("bestmove"), UciMessage::Unknown(_)));
}

#[test]
fn capabilities_only_offered_when_declared() {
    let none: Vec<EngineOption> = vec![opt("option name Style type combo default A var A var B")];
    assert_eq!(Capabilities::from_options(&none), Capabilities::default());
    let some = vec![
        opt("option name hash type spin default 16 min 1 max 1024"),
        opt("option name MultiPV type spin default 1 min 1 max 5"),
        opt("option name Threads type check default true"), // wrong type -> not offered
    ];
    let caps = Capabilities::from_options(&some);
    assert_eq!(caps.hash_mb, Some(("hash".into(), 1, 1024)));
    assert_eq!(caps.multipv, Some(("MultiPV".into(), 1, 5)));
    assert_eq!(caps.threads, None);
}

#[test]
fn option_value_validation() {
    let spin = opt("option name Hash type spin default 16 min 1 max 100");
    assert_eq!(spin.normalize_value(" 64 ").unwrap(), "64");
    assert!(spin.normalize_value("0").is_err());
    assert!(spin.normalize_value("101").is_err());
    assert!(spin.normalize_value("lots").is_err());
    let check = opt("option name Ponder type check default false");
    assert_eq!(check.normalize_value("ON").unwrap(), "true");
    assert_eq!(check.normalize_value("0").unwrap(), "false");
    assert!(check.normalize_value("maybe").is_err());
    let combo = opt("option name Style type combo default Normal var Normal var Aggressive");
    assert_eq!(combo.normalize_value("aggressive").unwrap(), "Aggressive");
    assert!(combo.normalize_value("sleepy").is_err());
    let s = opt("option name Path type string default x");
    assert!(s.normalize_value("a\nquit").is_err());
}
