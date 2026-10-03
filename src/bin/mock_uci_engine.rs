//! A small, deterministic, fake UCI engine used by the test-suite (and handy for trying the GUI
//! without a real engine installed). It plays the first legal move it finds.
//!
//! Behaviour flags (command-line arguments):
//!   --minimal          advertise no options at all
//!   --no-multipv       do not advertise MultiPV
//!   --silent           never answer `uci` (simulates "not a UCI engine")
//!   --garbage          answer `uci` with nonsense and never send `uciok`
//!   --crash-on-go      exit(1) as soon as a `go` command arrives
//!   --bad-bestmove     answer searches with an illegal `bestmove`
//!   --mate             report `score mate 3` instead of centipawns
//!   --startup-delay-ms N   sleep before answering anything
//!   --name NAME        engine name to report
use cea::chess::{Move, Position};
use std::io::{BufRead, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Default)]
struct Flags {
    minimal: bool,
    no_multipv: bool,
    silent: bool,
    garbage: bool,
    crash_on_go: bool,
    bad_bestmove: bool,
    mate: bool,
    name: String,
}

fn out(line: &str) {
    let stdout = std::io::stdout();
    let mut h = stdout.lock();
    let _ = writeln!(h, "{line}");
    let _ = h.flush();
}

fn main() {
    let mut flags = Flags { name: "MockFish".to_string(), ..Default::default() };
    let mut delay = 0u64;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--minimal" => flags.minimal = true,
            "--no-multipv" => flags.no_multipv = true,
            "--silent" => flags.silent = true,
            "--garbage" => flags.garbage = true,
            "--crash-on-go" => flags.crash_on_go = true,
            "--bad-bestmove" => flags.bad_bestmove = true,
            "--mate" => flags.mate = true,
            "--startup-delay-ms" => {
                i += 1;
                delay = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(0);
            }
            "--name" => {
                i += 1;
                if let Some(n) = args.get(i) {
                    flags.name = n.clone();
                }
            }
            _ => {}
        }
        i += 1;
    }
    if delay > 0 {
        std::thread::sleep(Duration::from_millis(delay));
    }

    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(l) => {
                    if tx.send(l).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut position = Position::starting();
    let mut multipv: usize = 1;

    while let Ok(line) = rx.recv() {
        let mut parts = line.split_whitespace();
        let Some(cmd) = parts.next() else { continue };
        match cmd {
            "uci" => {
                if flags.silent {
                    continue;
                }
                if flags.garbage {
                    out("Welcome to Totally Not A Chess Engine v0.0");
                    out("please enjoy your stay");
                    continue;
                }
                out(&format!("id name {}", flags.name));
                out("id author The Test Suite");
                if !flags.minimal {
                    out("option name Threads type spin default 1 min 1 max 64");
                    out("option name Hash type spin default 16 min 1 max 4096");
                    if !flags.no_multipv {
                        out("option name MultiPV type spin default 1 min 1 max 10");
                    }
                    out("option name Style type combo default Normal var Normal var Aggressive var Defensive");
                    out("option name Use NNUE type check default true");
                    out("option name Eval File type string default <empty>");
                    out("option name Clear Hash type button");
                    out("option name Totally Custom Knob type spin default 5 min -10 max 10");
                }
                out("uciok");
            }
            "isready" => out("readyok"),
            "ucinewgame" | "debug" | "register" | "ponderhit" => {}
            "setoption" => {
                // setoption name <id> [value <x>]
                let rest: Vec<&str> = parts.collect();
                if let Some(vpos) = rest.iter().position(|w| *w == "value") {
                    let name = rest[1..vpos].join(" ");
                    let value = rest[vpos + 1..].join(" ");
                    if name == "MultiPV" {
                        multipv = value.parse().unwrap_or(1).max(1);
                    }
                    out(&format!("info string option {name} set to {value}"));
                }
            }
            "position" => {
                let rest: Vec<&str> = parts.collect();
                let moves_at = rest.iter().position(|w| *w == "moves");
                let (head, moves) = match moves_at {
                    Some(p) => (&rest[..p], &rest[p + 1..]),
                    None => (&rest[..], &rest[0..0]),
                };
                let base = if head.first() == Some(&"startpos") {
                    Some(Position::starting())
                } else if head.first() == Some(&"fen") {
                    Position::from_fen(&head[1..].join(" ")).ok()
                } else {
                    None
                };
                if let Some(mut p) = base {
                    for m in moves {
                        match p.parse_uci_move(m) {
                            Ok(mv) => p = p.make_move(&mv),
                            Err(_) => break,
                        }
                    }
                    position = p;
                }
            }
            "go" => {
                if flags.crash_on_go {
                    eprintln!("mock engine: simulated crash");
                    std::process::exit(1);
                }
                let rest: Vec<&str> = parts.collect();
                let get = |key: &str| -> Option<u64> {
                    rest.iter().position(|w| *w == key).and_then(|p| rest.get(p + 1)).and_then(|v| v.parse().ok())
                };
                let infinite = rest.contains(&"infinite");
                let movetime = get("movetime");
                let depth_limit = get("depth");
                let node_limit = get("nodes");
                let quit = search(&position, &flags, multipv, infinite, movetime, depth_limit, node_limit, &rx);
                if quit {
                    return;
                }
            }
            "stop" => {} // nothing running
            "quit" => return,
            _ => out(&format!("info string unknown command: {cmd}")),
        }
    }
}

/// Runs a fake iterative-deepening search. Returns true if the engine should quit.
#[allow(clippy::too_many_arguments)]
fn search(
    pos: &Position,
    flags: &Flags,
    multipv: usize,
    infinite: bool,
    movetime: Option<u64>,
    depth_limit: Option<u64>,
    node_limit: Option<u64>,
    rx: &mpsc::Receiver<String>,
) -> bool {
    let legal = pos.legal_moves();
    let start = Instant::now();
    let max_depth = depth_limit.unwrap_or(if infinite || movetime.is_some() { 60 } else { 6 });
    let lines = multipv.min(legal.len()).max(1);
    let mut nodes = 0u64;
    let mut quit = false;

    'iter: for depth in 1..=max_depth {
        for k in 0..lines {
            if legal.is_empty() {
                break;
            }
            nodes += 1000 * depth;
            let elapsed = start.elapsed().as_millis() as u64;
            let nps = if elapsed > 0 { nodes * 1000 / elapsed } else { nodes * 1000 };
            let score = if flags.mate { "mate 3".to_string() } else { format!("cp {}", 10 * depth as i64 + k as i64 * 25) };
            let mut pv: Vec<String> = Vec::new();
            let mut p = pos.clone();
            let mut next = Some(legal[k]);
            for _ in 0..4 {
                let Some(m) = next else { break };
                pv.push(m.to_uci());
                p = p.make_move(&m);
                next = p.legal_moves().first().copied();
            }
            // Includes deliberately odd extras: an unknown field, hashfull, tbhits and a string.
            out(&format!(
                "info depth {depth} seldepth {} multipv {} score {score} nodes {nodes} nps {nps} hashfull 12 tbhits 0 time {elapsed} unknownfield 42 pv {}",
                depth + 2,
                k + 1,
                pv.join(" ")
            ));
        }
        if depth == 1 {
            out("info string mock engine is thinking");
        }
        if let Some(n) = node_limit {
            if nodes >= n {
                break 'iter;
            }
        }
        if let Some(t) = movetime {
            if start.elapsed().as_millis() as u64 >= t {
                break 'iter;
            }
        }
        // Pace the search and stay responsive to stop/quit/isready.
        let deadline = Instant::now() + Duration::from_millis(15);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(remaining) {
                Ok(l) => match l.split_whitespace().next() {
                    Some("stop") => break 'iter,
                    Some("quit") => {
                        quit = true;
                        break 'iter;
                    }
                    Some("isready") => out("readyok"),
                    _ => {}
                },
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    quit = true;
                    break 'iter;
                }
            }
        }
    }
    if quit {
        return true;
    }
    if flags.bad_bestmove {
        out("bestmove zzzz");
        return false;
    }
    match legal.first() {
        Some(best) => {
            let ponder: Option<Move> = pos.make_move(best).legal_moves().first().copied();
            match ponder {
                Some(p) => out(&format!("bestmove {} ponder {}", best.to_uci(), p.to_uci())),
                None => out(&format!("bestmove {}", best.to_uci())),
            }
        }
        None => out("bestmove (none)"),
    }
    false
}
