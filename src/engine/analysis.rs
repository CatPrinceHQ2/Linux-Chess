//! Analysis state: turns raw engine events into display-ready data (white-perspective scores,
//! SAN lines, formatted counters). Keeps the GUI free of protocol details.
use super::parser::{Bound, Info, Score};
use crate::chess::{Color, Position};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnalysisLine {
    pub multipv: u32,
    pub depth: Option<u32>,
    pub seldepth: Option<u32>,
    /// Always from White's point of view.
    pub score: Option<Score>,
    pub nodes: Option<u64>,
    pub nps: Option<u64>,
    pub time_ms: Option<u64>,
    pub hashfull: Option<u32>,
    pub pv_uci: Vec<String>,
    pub pv_san: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BestMoveResult {
    pub uci: Option<String>,
    pub san: Option<String>,
    pub ponder_uci: Option<String>,
    /// Set when the engine named a move that is not legal in the analysed position.
    pub illegal: bool,
}

#[derive(Clone, Debug)]
pub struct AnalysisState {
    root: Position,
    lines: BTreeMap<u32, AnalysisLine>,
    pub best: Option<BestMoveResult>,
    pub messages: Vec<String>,
    pub running: bool,
}

impl AnalysisState {
    pub fn new(root: Position) -> Self {
        AnalysisState { root, lines: BTreeMap::new(), best: None, messages: Vec::new(), running: true }
    }

    pub fn root(&self) -> &Position {
        &self.root
    }

    pub fn lines(&self) -> impl Iterator<Item = &AnalysisLine> {
        self.lines.values()
    }

    pub fn main_line(&self) -> Option<&AnalysisLine> {
        self.lines.get(&1).or_else(|| self.lines.values().next())
    }

    pub fn apply_info(&mut self, info: &Info) {
        if let Some(s) = &info.string {
            if self.messages.len() < 50 {
                self.messages.push(s.clone());
            }
        }
        // Informational-only updates (`currmove`, `info string`) carry no analysis.
        if info.score.is_none() && info.pv.is_empty() && info.depth.is_none() && info.nodes.is_none() {
            return;
        }
        let key = info.multipv.unwrap_or(1);
        let prev = self.lines.get(&key).cloned();
        let side = self.root.side_to_move;
        // Engines often finish with a bound-only line (`lowerbound`/`upperbound`, short PV) after
        // an exact one. Keep the exact score and PV for display; only refresh the counters.
        let is_bound = matches!(info.score, Some(Score::Cp { bound, .. }) | Some(Score::Mate { bound, .. }) if bound != Bound::Exact);
        let prev_exact = prev.as_ref().map(|p| matches!(p.score, Some(Score::Cp { bound: Bound::Exact, .. }) | Some(Score::Mate { bound: Bound::Exact, .. }))).unwrap_or(false);
        if is_bound && prev_exact {
            let mut kept = prev.clone().unwrap();
            kept.nodes = info.nodes.or(kept.nodes);
            kept.nps = info.nps.or(kept.nps);
            kept.time_ms = info.time_ms.or(kept.time_ms);
            kept.hashfull = info.hashfull.or(kept.hashfull);
            self.lines.insert(key, kept);
            return;
        }
        let score = info.score.map(|s| to_white(s, side)).or_else(|| prev.as_ref().and_then(|p| p.score));
        let (pv_uci, pv_san) = if info.pv.is_empty() {
            prev.as_ref().map(|p| (p.pv_uci.clone(), p.pv_san.clone())).unwrap_or_default()
        } else {
            (info.pv.clone(), self.root.line_to_san(&info.pv))
        };
        let pick = |new: Option<u64>, old: Option<u64>| new.or(old);
        self.lines.insert(
            key,
            AnalysisLine {
                multipv: key,
                depth: info.depth.or(prev.as_ref().and_then(|p| p.depth)),
                seldepth: info.seldepth.or(prev.as_ref().and_then(|p| p.seldepth)),
                score,
                nodes: pick(info.nodes, prev.as_ref().and_then(|p| p.nodes)),
                nps: pick(info.nps, prev.as_ref().and_then(|p| p.nps)),
                time_ms: pick(info.time_ms, prev.as_ref().and_then(|p| p.time_ms)),
                hashfull: info.hashfull.or(prev.as_ref().and_then(|p| p.hashfull)),
                pv_uci,
                pv_san,
            },
        );
    }

    pub fn apply_bestmove(&mut self, best: Option<&str>, ponder: Option<&str>) {
        self.running = false;
        let (san, illegal) = match best {
            Some(u) => match self.root.parse_uci_move(u) {
                Ok(m) => (Some(self.root.to_san(&m)), false),
                Err(_) => (None, true),
            },
            None => (None, false),
        };
        self.best = Some(BestMoveResult {
            uci: best.map(str::to_string),
            san,
            ponder_uci: ponder.map(str::to_string),
            illegal,
        });
    }

    pub fn cancelled(&mut self) {
        self.running = false;
    }
}

fn to_white(score: Score, side: Color) -> Score {
    let flip = side == Color::Black;
    match score {
        Score::Cp { value, bound } => Score::Cp { value: if flip { -value } else { value }, bound: if flip { flip_bound(bound) } else { bound } },
        Score::Mate { moves, bound } => Score::Mate { moves: if flip { -moves } else { moves }, bound: if flip { flip_bound(bound) } else { bound } },
    }
}

fn flip_bound(b: Bound) -> Bound {
    match b {
        Bound::Exact => Bound::Exact,
        Bound::Lower => Bound::Upper,
        Bound::Upper => Bound::Lower,
    }
}

// ------------------------------------------------------------ formatting helpers

pub fn format_score(score: &Score) -> String {
    let (text, bound) = match score {
        Score::Cp { value, bound } => (format!("{:+.2}", *value as f64 / 100.0), bound),
        Score::Mate { moves, bound } => {
            if *moves == 0 {
                ("#".to_string(), bound)
            } else if *moves > 0 {
                (format!("#{moves}"), bound)
            } else {
                (format!("-#{}", moves.abs()), bound)
            }
        }
    };
    match bound {
        Bound::Exact => text,
        Bound::Lower => format!("≥ {text}"),
        Bound::Upper => format!("≤ {text}"),
    }
}

/// 14_200_000 -> "14.2M"
pub fn format_count(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{:.1}k", n as f64 / 1e3),
        1_000_000..=999_999_999 => format!("{:.1}M", n as f64 / 1e6),
        _ => format!("{:.2}G", n as f64 / 1e9),
    }
}

pub fn format_time(ms: u64) -> String {
    if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{}m {:02}s", ms / 60_000, (ms % 60_000) / 1000)
    }
}

/// "1. Nf3 d5 2. g3" / "12... Qxd4 13. Nc3" from a SAN line starting at `root`.
pub fn numbered_pv(root: &Position, san: &[String]) -> String {
    let mut out = String::new();
    let mut number = root.fullmove_number;
    let mut white = root.side_to_move == Color::White;
    for (i, m) in san.iter().enumerate() {
        if !out.is_empty() {
            out.push(' ');
        }
        if white {
            out.push_str(&format!("{number}. {m}"));
        } else {
            if i == 0 {
                out.push_str(&format!("{number}... "));
            }
            out.push_str(m);
            number += 1;
        }
        white = !white;
    }
    out
}
