//! Parsing of engine -> GUI UCI lines. The parser is deliberately tolerant: unknown tokens are
//! skipped, missing fields stay `None`, and anything unrecognised becomes `UciMessage::Unknown`.
use super::options::{EngineOption, OptionKind};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bound {
    Exact,
    Lower,
    Upper,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Score {
    /// Centipawns from the point of view of the side to move.
    Cp { value: i32, bound: Bound },
    /// Moves until mate; negative when the side to move is being mated.
    Mate { moves: i32, bound: Bound },
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Info {
    pub depth: Option<u32>,
    pub seldepth: Option<u32>,
    pub multipv: Option<u32>,
    pub score: Option<Score>,
    pub nodes: Option<u64>,
    pub nps: Option<u64>,
    pub time_ms: Option<u64>,
    pub hashfull: Option<u32>,
    pub tbhits: Option<u64>,
    pub currmove: Option<String>,
    pub currmovenumber: Option<u32>,
    pub pv: Vec<String>,
    pub string: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum UciMessage {
    IdName(String),
    IdAuthor(String),
    UciOk,
    ReadyOk,
    Option(EngineOption),
    Info(Info),
    BestMove { best: Option<String>, ponder: Option<String> },
    Unknown(String),
}

pub fn parse_line(line: &str) -> UciMessage {
    let line = line.trim();
    let mut it = line.splitn(2, char::is_whitespace);
    let first = it.next().unwrap_or("");
    let rest = it.next().unwrap_or("").trim();
    match first {
        "uciok" => UciMessage::UciOk,
        "readyok" => UciMessage::ReadyOk,
        "id" => parse_id(rest).unwrap_or_else(|| UciMessage::Unknown(line.to_string())),
        "option" => parse_option(rest).map(UciMessage::Option).unwrap_or_else(|| UciMessage::Unknown(line.to_string())),
        "info" => UciMessage::Info(parse_info(rest)),
        "bestmove" => parse_bestmove(rest).unwrap_or_else(|| UciMessage::Unknown(line.to_string())),
        _ => UciMessage::Unknown(line.to_string()),
    }
}

fn parse_id(rest: &str) -> Option<UciMessage> {
    let mut it = rest.splitn(2, char::is_whitespace);
    let key = it.next()?;
    let value = it.next().unwrap_or("").trim().to_string();
    match key {
        "name" => Some(UciMessage::IdName(value)),
        "author" => Some(UciMessage::IdAuthor(value)),
        _ => None,
    }
}

fn parse_bestmove(rest: &str) -> Option<UciMessage> {
    let toks: Vec<&str> = rest.split_whitespace().collect();
    let first = *toks.first()?;
    let best = if first == "(none)" || first == "0000" || first == "NULL" { None } else { Some(first.to_string()) };
    let ponder = toks
        .iter()
        .position(|t| *t == "ponder")
        .and_then(|p| toks.get(p + 1))
        .map(|s| s.to_string());
    Some(UciMessage::BestMove { best, ponder })
}

const OPTION_KEYWORDS: [&str; 6] = ["name", "type", "default", "min", "max", "var"];

/// Parses the part of an `option` line after the word `option`.
pub fn parse_option(rest: &str) -> Option<EngineOption> {
    let toks: Vec<&str> = rest.split_whitespace().collect();
    if toks.first() != Some(&"name") {
        return None;
    }
    // Split into (keyword, words...) groups. A keyword is only recognised where the UCI grammar
    // allows it, so option names/values may contain other words freely. The name extends to the
    // first `type` token.
    let type_pos = toks.iter().position(|t| *t == "type")?;
    let name = toks[1..type_pos].join(" ");
    if name.is_empty() {
        return None;
    }
    let mut groups: Vec<(&str, Vec<&str>)> = Vec::new();
    for t in &toks[type_pos..] {
        if OPTION_KEYWORDS.contains(t) {
            groups.push((t, Vec::new()));
        } else if let Some(g) = groups.last_mut() {
            g.1.push(t);
        } else {
            groups.push((t, Vec::new()));
        }
    }
    let get = |key: &str| groups.iter().find(|g| g.0 == key).map(|g| g.1.join(" "));
    let kind_str = get("type")?;
    let kind = match kind_str.as_str() {
        "check" => OptionKind::Check { default: get("default").map(|d| d.eq_ignore_ascii_case("true")).unwrap_or(false) },
        "spin" => {
            let default = get("default").and_then(|v| v.parse().ok()).unwrap_or(0);
            let min = get("min").and_then(|v| v.parse().ok()).unwrap_or(i64::MIN / 2);
            let max = get("max").and_then(|v| v.parse().ok()).unwrap_or(i64::MAX / 2);
            OptionKind::Spin { default, min, max }
        }
        "combo" => {
            let choices: Vec<String> = groups.iter().filter(|g| g.0 == "var").map(|g| g.1.join(" ")).collect();
            OptionKind::Combo { default: get("default").unwrap_or_default(), choices }
        }
        "string" => OptionKind::Str { default: get("default").unwrap_or_default() },
        "button" => OptionKind::Button,
        _ => return None,
    };
    Some(EngineOption { name, kind })
}

pub fn parse_info(rest: &str) -> Info {
    let toks: Vec<&str> = rest.split_whitespace().collect();
    let mut info = Info::default();
    let mut i = 0;
    let num = |i: usize| toks.get(i).and_then(|t| t.parse::<i64>().ok());
    while i < toks.len() {
        match toks[i] {
            "depth" => {
                info.depth = num(i + 1).map(|v| v.max(0) as u32);
                i += 2;
            }
            "seldepth" => {
                info.seldepth = num(i + 1).map(|v| v.max(0) as u32);
                i += 2;
            }
            "multipv" => {
                info.multipv = num(i + 1).map(|v| v.max(1) as u32);
                i += 2;
            }
            "nodes" => {
                info.nodes = num(i + 1).map(|v| v.max(0) as u64);
                i += 2;
            }
            "nps" => {
                info.nps = num(i + 1).map(|v| v.max(0) as u64);
                i += 2;
            }
            "time" => {
                info.time_ms = num(i + 1).map(|v| v.max(0) as u64);
                i += 2;
            }
            "hashfull" => {
                info.hashfull = num(i + 1).map(|v| v.max(0) as u32);
                i += 2;
            }
            "tbhits" => {
                info.tbhits = num(i + 1).map(|v| v.max(0) as u64);
                i += 2;
            }
            "currmovenumber" => {
                info.currmovenumber = num(i + 1).map(|v| v.max(0) as u32);
                i += 2;
            }
            "currmove" => {
                info.currmove = toks.get(i + 1).map(|s| s.to_string());
                i += 2;
            }
            "score" => {
                i += 1;
                let kind = toks.get(i).copied();
                let value = num(i + 1);
                i += 2;
                let mut bound = Bound::Exact;
                while let Some(t) = toks.get(i) {
                    match *t {
                        "lowerbound" => bound = Bound::Lower,
                        "upperbound" => bound = Bound::Upper,
                        _ => break,
                    }
                    i += 1;
                }
                info.score = match (kind, value) {
                    (Some("cp"), Some(v)) => Some(Score::Cp { value: v.clamp(i32::MIN as i64, i32::MAX as i64) as i32, bound }),
                    (Some("mate"), Some(v)) => Some(Score::Mate { moves: v.clamp(i32::MIN as i64, i32::MAX as i64) as i32, bound }),
                    _ => None,
                };
            }
            "string" => {
                info.string = Some(toks[i + 1..].join(" "));
                break;
            }
            "pv" => {
                // Everything after `pv` is the line, up to the next known keyword (some engines
                // append more fields after it).
                let mut j = i + 1;
                while j < toks.len() && is_move_like(toks[j]) {
                    info.pv.push(toks[j].to_string());
                    j += 1;
                }
                i = j;
            }
            "refutation" | "currline" => break,
            _ => i += 1, // unknown field or value: skip a single token and carry on
        }
    }
    info
}

fn is_move_like(t: &str) -> bool {
    let b = t.as_bytes();
    (b.len() == 4 || b.len() == 5)
        && (b'a'..=b'h').contains(&b[0])
        && (b'1'..=b'8').contains(&b[1])
        && (b'a'..=b'h').contains(&b[2])
        && (b'1'..=b'8').contains(&b[3])
        && (b.len() == 4 || b"qrbnQRBN".contains(&b[4]))
}
