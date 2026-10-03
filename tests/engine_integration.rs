use cea::chess::Position;
use cea::engine::analysis::AnalysisState;
use cea::engine::log::Logger;
use cea::engine::uci::{probe, UciEngine};
use cea::engine::*;
use std::time::{Duration, Instant};

const MOCK: &str = env!("CARGO_BIN_EXE_mock-uci-engine");
const T: Duration = Duration::from_secs(10);

fn spec(args: &[&str]) -> EngineSpec {
    EngineSpec { path: MOCK.into(), args: args.iter().map(|s| s.to_string()).collect() }
}

fn launch(args: &[&str]) -> UciEngine {
    UciEngine::launch(spec(args), Logger::new(false), Duration::from_secs(5)).expect("mock engine starts")
}

struct Outcome {
    infos: Vec<Info>,
    best: Option<Option<String>>,
    failed: Option<EngineError>,
}

/// Pump events until the engine has actually started searching (past the isready/readyok step).
fn wait_searching(engine: &mut UciEngine) {
    let start = Instant::now();
    while engine.state() != EngineState::Searching {
        assert!(start.elapsed() < T, "engine never reached Searching");
        let _ = engine.next_event(Duration::from_millis(10));
    }
}

fn run(engine: &mut UciEngine, pos: &Position, limit: SearchLimit) -> Outcome {
    engine.start_search(SearchRequest { position: pos.clone(), limit }).unwrap();
    collect(engine, None)
}

fn collect(engine: &mut UciEngine, stop_after: Option<Duration>) -> Outcome {
    let mut out = Outcome { infos: vec![], best: None, failed: None };
    let start = Instant::now();
    let mut stopped = false;
    while start.elapsed() < T {
        if let Some(d) = stop_after {
            if !stopped && start.elapsed() > d {
                engine.stop_search().unwrap();
                stopped = true;
            }
        }
        match engine.next_event(Duration::from_millis(20)) {
            Some(EngineEvent::Info(i)) => out.infos.push(i),
            Some(EngineEvent::BestMove { best, .. }) => {
                out.best = Some(best);
                break;
            }
            Some(EngineEvent::Failed(e)) => {
                out.failed = Some(e);
                break;
            }
            Some(EngineEvent::SearchCancelled) => break,
            _ => {}
        }
    }
    out
}

#[test]
fn handshake_discovers_identity_and_every_declared_option() {
    let e = launch(&[]);
    assert_eq!(e.identity().name, "MockFish");
    assert_eq!(e.identity().author.as_deref(), Some("The Test Suite"));
    let names: Vec<&str> = e.options().iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, ["Threads", "Hash", "MultiPV", "Style", "Use NNUE", "Eval File", "Clear Hash", "Totally Custom Knob"]);
    assert!(e.options().iter().any(|o| o.is_button()));
    let caps = Capabilities::from_options(e.options());
    assert!(caps.threads.is_some() && caps.hash_mb.is_some() && caps.multipv.is_some());
}

#[test]
fn engines_with_different_options_get_different_capabilities() {
    let e = launch(&["--no-multipv"]);
    assert!(Capabilities::from_options(e.options()).multipv.is_none());
    let e = launch(&["--minimal", "--name", "Tiny"]);
    assert_eq!(e.identity().name, "Tiny");
    assert!(e.options().is_empty());
    assert_eq!(Capabilities::from_options(e.options()), Capabilities::default());
}

#[test]
fn probe_reports_identity_options_and_cleans_up() {
    let r = probe(spec(&[]), Logger::new(false), Duration::from_secs(5)).unwrap();
    assert_eq!(r.identity.name, "MockFish");
    assert_eq!(r.options.len(), 8);
}

#[test]
fn depth_search_returns_legal_bestmove_with_san_and_analysis() {
    let mut e = launch(&[]);
    let pos = Position::starting();
    let out = run(&mut e, &pos, SearchLimit::Depth(4));
    let best = out.best.expect("bestmove").expect("a move");
    assert!(pos.parse_uci_move(&best).is_ok());
    assert!(out.infos.iter().any(|i| i.depth == Some(4)));
    assert_eq!(e.state(), EngineState::Idle);

    let mut a = AnalysisState::new(pos.clone());
    for i in &out.infos {
        a.apply_info(i);
    }
    a.apply_bestmove(Some(&best), None);
    let line = a.main_line().unwrap();
    assert_eq!(line.depth, Some(4));
    assert!(!line.pv_san.is_empty());
    let b = a.best.as_ref().unwrap();
    assert_eq!(b.uci.as_deref(), Some(best.as_str()));
    assert_eq!(b.san.as_deref(), Some(pos.to_san(&pos.parse_uci_move(&best).unwrap()).as_str()));
    assert!(!b.illegal);
}

#[test]
fn black_to_move_scores_are_flipped_to_white_perspective() {
    let mut e = launch(&[]);
    let pos = Position::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1").unwrap();
    let out = run(&mut e, &pos, SearchLimit::Depth(2));
    let mut a = AnalysisState::new(pos);
    out.infos.iter().for_each(|i| a.apply_info(i));
    // The mock reports a positive score for the side to move; for Black that is bad for White.
    match a.main_line().unwrap().score.unwrap() {
        Score::Cp { value, .. } => assert!(value < 0, "got {value}"),
        s => panic!("{s:?}"),
    }
}

#[test]
fn time_and_node_limits_are_honoured() {
    let mut e = launch(&[]);
    let t = Instant::now();
    let out = run(&mut e, &Position::starting(), SearchLimit::Time { ms: 300 });
    assert!(out.best.is_some());
    assert!(t.elapsed() >= Duration::from_millis(250) && t.elapsed() < Duration::from_secs(5));
    let out = run(&mut e, &Position::starting(), SearchLimit::Nodes(20_000));
    assert!(out.best.is_some());
    assert!(out.infos.last().unwrap().nodes.unwrap() >= 20_000);
}

#[test]
fn infinite_search_can_be_stopped() {
    let mut e = launch(&[]);
    e.start_search(SearchRequest { position: Position::starting(), limit: SearchLimit::Infinite }).unwrap();
    let out = collect(&mut e, Some(Duration::from_millis(250)));
    assert!(out.best.is_some(), "stop must produce a bestmove");
    assert_eq!(e.state(), EngineState::Idle);
    assert!(out.infos.len() > 3);
}

#[test]
fn cannot_start_a_second_search_while_busy() {
    let mut e = launch(&[]);
    e.start_search(SearchRequest { position: Position::starting(), limit: SearchLimit::Infinite }).unwrap();
    let err = e.start_search(SearchRequest { position: Position::starting(), limit: SearchLimit::Depth(1) }).unwrap_err();
    assert!(matches!(err, EngineError::Busy(_)));
    wait_searching(&mut e);
    e.stop_search().unwrap();
    assert!(collect(&mut e, None).best.is_some());
    // And now it works again.
    assert!(run(&mut e, &Position::starting(), SearchLimit::Depth(1)).best.is_some());
}

#[test]
fn stop_before_search_starts_cancels_cleanly() {
    let mut e = launch(&[]);
    e.start_search(SearchRequest { position: Position::starting(), limit: SearchLimit::Infinite }).unwrap();
    assert_eq!(e.state(), EngineState::Preparing);
    e.stop_search().unwrap();
    assert_eq!(e.state(), EngineState::Idle);
    assert!(matches!(e.poll().as_slice(), [EngineEvent::SearchCancelled]));
    // The stale readyok must not start a search or confuse the next one.
    let out = run(&mut e, &Position::starting(), SearchLimit::Depth(2));
    assert!(out.best.is_some());
}

#[test]
fn option_changes_during_search_are_queued_until_idle() {
    let mut e = launch(&[]);
    e.start_search(SearchRequest { position: Position::starting(), limit: SearchLimit::Infinite }).unwrap();
    wait_searching(&mut e);
    e.set_option("MultiPV", Some("3")).unwrap(); // queued, not sent
    e.stop_search().unwrap();
    assert!(collect(&mut e, None).best.is_some());
    let out = run(&mut e, &Position::starting(), SearchLimit::Depth(3));
    let max_pv = out.infos.iter().filter_map(|i| i.multipv).max().unwrap();
    assert_eq!(max_pv, 3, "queued MultiPV must have been applied");
}

#[test]
fn multipv_lines_are_tracked_separately() {
    let mut e = launch(&[]);
    e.set_option("MultiPV", Some("3")).unwrap();
    let pos = Position::starting();
    let out = run(&mut e, &pos, SearchLimit::Depth(3));
    let mut a = AnalysisState::new(pos);
    out.infos.iter().for_each(|i| a.apply_info(i));
    let lines: Vec<_> = a.lines().collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines.iter().map(|l| l.multipv).collect::<Vec<_>>(), [1, 2, 3]);
    assert!(lines.iter().all(|l| l.depth == Some(3)));
    let firsts: std::collections::HashSet<_> = lines.iter().map(|l| l.pv_uci[0].clone()).collect();
    assert_eq!(firsts.len(), 3);
}

#[test]
fn set_option_validation_and_buttons() {
    let mut e = launch(&[]);
    assert!(matches!(e.set_option("Nope", Some("1")), Err(EngineError::InvalidOption(_))));
    assert!(matches!(e.set_option("Hash", Some("0")), Err(EngineError::InvalidOption(_))));
    assert!(matches!(e.set_option("Hash", Some("abc")), Err(EngineError::InvalidOption(_))));
    assert!(matches!(e.set_option("Hash", None), Err(EngineError::InvalidOption(_))));
    assert!(matches!(e.set_option("Style", Some("Sleepy")), Err(EngineError::InvalidOption(_))));
    e.set_option("hash", Some("64")).unwrap(); // case-insensitive lookup
    e.set_option("Style", Some("aggressive")).unwrap();
    e.set_option("Use NNUE", Some("false")).unwrap();
    e.set_option("Totally Custom Knob", Some("-7")).unwrap();
    e.set_option("Clear Hash", None).unwrap();
}

#[test]
fn wire_commands_are_logged_exactly() {
    let logger = Logger::new(true);
    let mut e = UciEngine::launch(spec(&[]), logger.clone(), Duration::from_secs(5)).unwrap();
    e.set_option("Style", Some("Defensive")).unwrap();
    e.set_option("Eval File", Some("")).unwrap();
    run(&mut e, &Position::starting(), SearchLimit::Time { ms: 50 });
    let log = logger.snapshot().join("\n");
    for needle in [
        "[Engine] Starting",
        "[UCI →] uci",
        "[UCI ←] id name MockFish",
        "[UCI ←] uciok",
        "[UCI →] setoption name Style value Defensive",
        "[UCI →] setoption name Eval File value <empty>",
        "[UCI →] isready",
        "[UCI ←] readyok",
        "[UCI →] position fen rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "[UCI →] go movetime 50",
        "[UCI ←] bestmove",
    ] {
        assert!(log.contains(needle), "missing {needle:?} in log:\n{log}");
    }
}

#[test]
fn invalid_position_is_rejected_before_reaching_the_engine() {
    let mut e = launch(&[]);
    let bad = Position::from_fen("8/8/8/8/8/8/8/8 w - - 0 1").unwrap();
    let err = e.start_search(SearchRequest { position: bad, limit: SearchLimit::Depth(1) }).unwrap_err();
    assert!(matches!(err, EngineError::InvalidPosition(_)));
    assert_eq!(err.user_message(), "The supplied FEN is invalid.");
    assert_eq!(e.state(), EngineState::Idle);
}

#[test]
fn engine_with_no_legal_moves_reports_none() {
    let mut e = launch(&[]);
    let mate = Position::from_fen("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3").unwrap();
    let out = run(&mut e, &mate, SearchLimit::Depth(2));
    assert_eq!(out.best, Some(None));
}

#[test]
fn illegal_bestmove_from_engine_is_flagged() {
    let mut e = launch(&["--bad-bestmove"]);
    let pos = Position::starting();
    let out = run(&mut e, &pos, SearchLimit::Depth(1));
    let best = out.best.unwrap();
    let mut a = AnalysisState::new(pos);
    a.apply_bestmove(best.as_deref(), None);
    assert!(a.best.unwrap().illegal);
}

#[test]
fn mate_scores_survive() {
    let mut e = launch(&["--mate"]);
    let out = run(&mut e, &Position::starting(), SearchLimit::Depth(1));
    assert_eq!(out.infos[0].score, Some(Score::Mate { moves: 3, bound: Bound::Exact }));
}

#[test]
fn crash_during_search_is_reported_with_details() {
    let mut e = launch(&["--crash-on-go"]);
    let out = run(&mut e, &Position::starting(), SearchLimit::Depth(5));
    match out.failed.expect("crash must be reported") {
        EngineError::Exited { while_searching, status, stderr_tail } => {
            assert!(while_searching);
            assert!(status.contains('1'), "status was {status}");
            assert!(stderr_tail.iter().any(|l| l.contains("simulated crash")));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(e.state(), EngineState::Exited);
    assert!(matches!(e.start_search(SearchRequest { position: Position::starting(), limit: SearchLimit::Depth(1) }), Err(EngineError::Exited { .. })));
    assert!(e.set_option("Hash", Some("32")).is_err());
}

#[test]
fn killed_engine_is_detected_and_restart_recovers() {
    let mut e = launch(&[]);
    e.set_option("Hash", Some("64")).unwrap();
    e.start_search(SearchRequest { position: Position::starting(), limit: SearchLimit::Infinite }).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    // Simulate an external kill of the engine process.
    let status = std::process::Command::new("kill").args(["-9", &e.pid().to_string()]).status().unwrap();
    assert!(status.success());
    let out = collect(&mut e, None);
    assert!(matches!(out.failed, Some(EngineError::Exited { .. })), "{:?}", out.failed.map(|e| e.technical_details()));
    assert_eq!(e.state(), EngineState::Exited);
    e.restart().unwrap();
    assert_eq!(e.state(), EngineState::Idle);
    assert!(run(&mut e, &Position::starting(), SearchLimit::Depth(2)).best.is_some());
}

#[test]
fn restart_reapplies_options() {
    let logger = Logger::new(true);
    let mut e = UciEngine::launch(spec(&[]), logger.clone(), Duration::from_secs(5)).unwrap();
    e.set_option("Hash", Some("128")).unwrap();
    e.restart().unwrap();
    let log = logger.snapshot();
    assert_eq!(log.iter().filter(|l| l.contains("setoption name Hash value 128")).count(), 2);
}

#[test]
fn shutdown_is_clean_and_idempotent() {
    let mut e = launch(&[]);
    let t = Instant::now();
    e.shutdown();
    e.shutdown();
    assert_eq!(e.state(), EngineState::Exited);
    assert!(t.elapsed() < Duration::from_secs(3));
    assert!(e.poll().is_empty(), "an intentional shutdown must not look like a crash");
}

#[test]
fn shutdown_while_searching() {
    let mut e = launch(&[]);
    e.start_search(SearchRequest { position: Position::starting(), limit: SearchLimit::Infinite }).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    e.shutdown();
    assert!(e.poll().iter().all(|ev| !matches!(ev, EngineEvent::Failed(_))));
}

// ---------------------------------------------------------------- bad executables

#[test]
fn missing_executable_is_a_start_failure() {
    let err = UciEngine::launch(EngineSpec::new("/definitely/not/here/engine"), Logger::new(false), Duration::from_secs(1)).err().unwrap();
    assert!(matches!(err, EngineError::StartFailed { .. }));
    assert_eq!(err.user_message(), "The selected executable could not be started.");
    assert!(err.technical_details().contains("/definitely/not/here/engine"));
}

#[test]
fn non_executable_file_is_a_start_failure() {
    let dir = std::env::temp_dir().join(format!("cea-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("plain.txt");
    std::fs::write(&file, "hello").unwrap();
    let err = UciEngine::launch(EngineSpec::new(&file), Logger::new(false), Duration::from_secs(1)).err().unwrap();
    assert!(matches!(err, EngineError::StartFailed { .. }));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn program_that_is_not_an_engine_fails_the_handshake() {
    // Exits immediately without speaking UCI.
    let err = UciEngine::launch(EngineSpec::new("/bin/true"), Logger::new(false), Duration::from_secs(2)).err().unwrap();
    assert!(matches!(err, EngineError::NotUci { .. }), "{err:?}");
    assert_eq!(err.user_message(), "The program did not respond correctly to the UCI handshake.");
}

#[test]
fn silent_engine_times_out_the_handshake() {
    let t = Instant::now();
    let err = UciEngine::launch(spec(&["--silent"]), Logger::new(false), Duration::from_millis(500)).err().unwrap();
    assert!(matches!(err, EngineError::NotUci { .. }));
    assert!(t.elapsed() < Duration::from_secs(4));
    assert!(err.technical_details().contains("no 'uciok'"));
}

#[test]
fn garbage_output_fails_the_handshake_and_shows_what_was_received() {
    let err = UciEngine::launch(spec(&["--garbage"]), Logger::new(false), Duration::from_millis(500)).err().unwrap();
    match err {
        EngineError::NotUci { detail, .. } => assert!(detail.contains("Totally Not A Chess Engine"), "{detail}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn slow_starting_engine_is_waited_for() {
    let e = UciEngine::launch(spec(&["--startup-delay-ms", "400"]), Logger::new(false), Duration::from_secs(5)).unwrap();
    assert_eq!(e.identity().name, "MockFish");
}

#[test]
fn paths_with_spaces_and_shell_metacharacters_are_not_interpreted() {
    // Copy the mock to an awkward path; direct exec must handle it without any shell involvement.
    let dir = std::env::temp_dir().join(format!("cea weird $(touch pwned); `x` {}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("my engine; echo hacked");
    std::fs::copy(MOCK, &target).unwrap();
    let e = UciEngine::launch(EngineSpec::new(&target), Logger::new(false), Duration::from_secs(5)).unwrap();
    assert_eq!(e.identity().name, "MockFish");
    drop(e);
    assert!(!std::path::Path::new("pwned").exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn formatting_helpers() {
    use cea::engine::analysis::*;
    assert_eq!(format_score(&Score::Cp { value: 82, bound: Bound::Exact }), "+0.82");
    assert_eq!(format_score(&Score::Cp { value: -150, bound: Bound::Exact }), "-1.50");
    assert_eq!(format_score(&Score::Mate { moves: 3, bound: Bound::Exact }), "#3");
    assert_eq!(format_score(&Score::Mate { moves: -2, bound: Bound::Exact }), "-#2");
    assert_eq!(format_score(&Score::Cp { value: 10, bound: Bound::Lower }), "≥ +0.10");
    assert_eq!(format_count(14_200_000), "14.2M");
    assert_eq!(format_count(950), "950");
    assert_eq!(format_time(4800), "4.8s");
    assert_eq!(format_time(125_000), "2m 05s");
}

#[test]
fn numbered_pv_formatting() {
    use cea::engine::analysis::numbered_pv;
    let start = Position::starting();
    let san: Vec<String> = ["Nf3", "d5", "g3", "c5"].iter().map(|s| s.to_string()).collect();
    assert_eq!(numbered_pv(&start, &san), "1. Nf3 d5 2. g3 c5");
    let black = Position::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 7").unwrap();
    let san: Vec<String> = ["c5", "Nf3", "d6"].iter().map(|s| s.to_string()).collect();
    assert_eq!(numbered_pv(&black, &san), "7... c5 8. Nf3 d6");
    assert_eq!(numbered_pv(&start, &[]), "");
}

#[test]
fn bound_only_final_line_does_not_replace_exact_score_and_pv() {
    use cea::engine::parser::parse_info;
    let pos = Position::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1").unwrap();
    let mut a = AnalysisState::new(pos);
    a.apply_info(&parse_info("depth 20 score cp -30 nodes 1000 time 100 pv e7e5 g1f3 b8c6"));
    a.apply_info(&parse_info("depth 21 score cp -21 upperbound nodes 2000 time 200 pv e7e5"));
    let l = a.main_line().unwrap();
    assert_eq!(l.score, Some(Score::Cp { value: 30, bound: Bound::Exact })); // flipped to White's view
    assert_eq!(l.pv_san, vec!["e5", "Nf3", "Nc6"]);
    assert_eq!((l.nodes, l.time_ms, l.depth), (Some(2000), Some(200), Some(20)));
}
