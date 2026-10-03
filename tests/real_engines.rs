//! Runs the generic integration layer against real UCI engines. Engines are taken from the
//! CEA_TEST_ENGINES environment variable (a ':'-separated list of executables) plus any
//! Stockfish found in the usual system locations. With none available the tests pass trivially
//! and say so, so the suite never depends on a particular engine being installed.
use cea::chess::Position;
use cea::engine::analysis::AnalysisState;
use cea::engine::log::Logger;
use cea::engine::uci::UciEngine;
use cea::engine::*;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn engines() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::env::var("CEA_TEST_ENGINES")
        .unwrap_or_default()
        .split(':')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect();
    for p in ["/usr/games/stockfish", "/usr/bin/stockfish", "/usr/local/bin/stockfish"] {
        if std::path::Path::new(p).exists() && !v.iter().any(|x| x == &PathBuf::from(p)) {
            v.push(p.into());
        }
    }
    if v.is_empty() {
        eprintln!("note: no real UCI engines found; set CEA_TEST_ENGINES to enable these tests");
    }
    v
}

fn start(path: &PathBuf) -> UciEngine {
    UciEngine::launch(EngineSpec::new(path), Logger::new(false), Duration::from_secs(20))
        .unwrap_or_else(|e| panic!("{}: {}", path.display(), e.technical_details()))
}

fn search(e: &mut UciEngine, pos: &Position, limit: SearchLimit) -> (AnalysisState, Option<String>) {
    e.start_search(SearchRequest { position: pos.clone(), limit }).unwrap();
    let mut a = AnalysisState::new(pos.clone());
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(60) {
        match e.next_event(Duration::from_millis(50)) {
            Some(EngineEvent::Info(i)) => a.apply_info(&i),
            Some(EngineEvent::BestMove { best, ponder }) => {
                a.apply_bestmove(best.as_deref(), ponder.as_deref());
                return (a, best);
            }
            Some(EngineEvent::Failed(err)) => panic!("{}", err.technical_details()),
            _ => {}
        }
    }
    panic!("search timed out");
}

#[test]
fn real_engine_handshake_and_options_are_discovered() {
    for path in engines() {
        let e = start(&path);
        assert!(!e.identity().name.is_empty(), "{}", path.display());
        assert!(!e.options().is_empty(), "{} declared no options", e.identity().name);
        eprintln!("{}: {} options, caps {:?}", e.identity().name, e.options().len(), Capabilities::from_options(e.options()));
    }
}

#[test]
fn real_engine_finds_mate_in_one_with_correct_san() {
    for path in engines() {
        let mut e = start(&path);
        let pos = Position::from_fen("6k1/5ppp/8/8/8/8/8/R3K3 w - - 0 1").unwrap();
        let (a, best) = search(&mut e, &pos, SearchLimit::Depth(8));
        assert_eq!(best.as_deref(), Some("a1a8"), "{}", e.identity().name);
        let b = a.best.clone().unwrap();
        assert_eq!(b.san.as_deref(), Some("Ra8#"));
        assert!(matches!(a.main_line().unwrap().score, Some(Score::Mate { moves, .. }) if moves > 0));
    }
}

#[test]
fn real_engine_start_position_time_search_and_pv_san() {
    for path in engines() {
        let mut e = start(&path);
        let pos = Position::starting();
        let (a, best) = search(&mut e, &pos, SearchLimit::Time { ms: 400 });
        assert!(pos.parse_uci_move(&best.unwrap()).is_ok());
        let line = a.main_line().unwrap();
        assert!(line.depth.unwrap_or(0) >= 1 && !line.pv_san.is_empty());
        assert!(line.nodes.is_some());
    }
}

#[test]
fn real_engine_multipv_when_advertised() {
    for path in engines() {
        let mut e = start(&path);
        let Some((name, _, max)) = Capabilities::from_options(e.options()).multipv else { continue };
        let n = 3.min(max);
        e.set_option(&name, Some(&n.to_string())).unwrap();
        let (a, _) = search(&mut e, &Position::starting(), SearchLimit::Depth(8));
        assert_eq!(a.lines().count() as i64, n, "{}", e.identity().name);
    }
}

#[test]
fn real_engine_infinite_search_stops() {
    for path in engines() {
        let mut e = start(&path);
        e.start_search(SearchRequest { position: Position::starting(), limit: SearchLimit::Infinite }).unwrap();
        let start = Instant::now();
        let mut stopped = false;
        let mut got_best = false;
        while start.elapsed() < Duration::from_secs(30) {
            if !stopped && e.state() == EngineState::Searching && start.elapsed() > Duration::from_millis(500) {
                e.stop_search().unwrap();
                stopped = true;
            }
            if let Some(EngineEvent::BestMove { best, .. }) = e.next_event(Duration::from_millis(50)) {
                assert!(best.is_some());
                got_best = true;
                break;
            }
        }
        assert!(got_best, "{}", e.identity().name);
        assert_eq!(e.state(), EngineState::Idle);
    }
}

#[test]
fn real_engine_every_declared_option_accepts_its_own_default() {
    for path in engines() {
        let mut e = start(&path);
        let options: Vec<EngineOption> = e.options().to_vec();
        for o in options {
            if let Some(v) = o.default_value() {
                // Skip file-ish strings: engines may legitimately reject paths that do not exist.
                if matches!(o.kind, OptionKind::Str { .. }) {
                    continue;
                }
                e.set_option(&o.name, Some(&v)).unwrap_or_else(|err| panic!("{}: {}", o.name, err.technical_details()));
            }
        }
        // The engine must still be healthy afterwards.
        let (_, best) = search(&mut e, &Position::starting(), SearchLimit::Depth(4));
        assert!(best.is_some());
    }
}
