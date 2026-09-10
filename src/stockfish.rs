// Talks to the Stockfish binary bundled in assets/ over UCI, so its answers can be
// compared against this engine's own. Used by the GUI's "Ask Stockfish" button, and
// by the `generate-library` command line mode that pre-computes a batch of positions
// (see stockfish_library.rs).

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

use crate::board::Board;

// how many of Stockfish's top lines to ask for and report
const MULTIPV: u32 = 5;
// bounds how long a click on the button can take - Stockfish always answers within
// this, whatever depth it reaches
const SEARCH_MOVETIME_MS: u32 = 10000;

// one of Stockfish's top answers, in the order it ranks them
pub struct StockfishLine {
    pub rank: u32,
    pub best_move: String,
    pub score: StockfishScore,
}

#[derive(Clone, Copy)]
pub enum StockfishScore {
    // centipawns, from the side to move's point of view
    Centipawns(i32),
    // mate in this many moves, from the side to move's point of view - negative
    // means it is the side to move who gets mated
    MateIn(i32),
}

// asks the bundled Stockfish for its top `MULTIPV` moves in the given position. A
// missing binary, an unexpected reply or the process going away all come back as an
// error string rather than a panic - none of this is trusted the way our own engine is
pub fn best_moves(board: &Board) -> Result<Vec<StockfishLine>, String> {
    let path = binary_path().ok_or("stockfish-linux not found in assets/")?;
    let mut child = spawn(&path)?;

    let mut stdin = child.stdin.take().ok_or("could not open stockfish's stdin")?;
    let stdout = child.stdout.take().ok_or("could not open stockfish's stdout")?;
    let mut lines = BufReader::new(stdout).lines();

    let result = converse(board, &mut stdin, &mut lines);

    // best effort: a shutdown going wrong does not change the answer already in hand
    let _ = send(&mut stdin, "quit");
    let _ = child.wait();

    result
}

fn converse(
    board: &Board,
    stdin: &mut ChildStdin,
    lines: &mut impl Iterator<Item = std::io::Result<String>>,
) -> Result<Vec<StockfishLine>, String> {
    send(stdin, "uci")?;
    wait_for(lines, "uciok")?;

    send(stdin, &format!("setoption name MultiPV value {MULTIPV}"))?;
    send(stdin, "isready")?;
    wait_for(lines, "readyok")?;

    send(stdin, &format!("position fen {}", board.to_fen()))?;
    send(stdin, &format!("go movetime {SEARCH_MOVETIME_MS}"))?;

    read_top_lines(lines)
}

fn binary_path() -> Option<PathBuf> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("stockfish-linux");
    path.exists().then_some(path)
}

fn spawn(path: &Path) -> Result<Child, String> {
    Command::new(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("failed to start stockfish: {error}"))
}

fn send(stdin: &mut ChildStdin, command: &str) -> Result<(), String> {
    writeln!(stdin, "{command}").map_err(|error| format!("failed to write to stockfish: {error}"))
}

// reads until a line matching `marker` exactly (once trimmed) goes by
fn wait_for(lines: &mut impl Iterator<Item = std::io::Result<String>>, marker: &str) -> Result<(), String> {
    for line in lines {
        let line = line.map_err(|error| format!("failed to read from stockfish: {error}"))?;
        if line.trim() == marker {
            return Ok(());
        }
    }
    Err(format!("stockfish closed before sending \"{marker}\""))
}

// reads "info ... multipv N ..." lines until "bestmove", keeping only the latest line
// for each rank - Stockfish repeats and refines them as it searches deeper
fn read_top_lines(
    lines: &mut impl Iterator<Item = std::io::Result<String>>,
) -> Result<Vec<StockfishLine>, String> {
    let mut slots: Vec<Option<StockfishLine>> = (0..MULTIPV).map(|_| None).collect();

    for line in lines {
        let line = line.map_err(|error| format!("failed to read from stockfish: {error}"))?;

        if line.starts_with("bestmove") {
            break;
        }
        if let Some(info) = parse_info_line(&line)
            && let Some(slot) = (info.rank as usize).checked_sub(1).and_then(|index| slots.get_mut(index))
        {
            *slot = Some(info);
        }
    }

    let found: Vec<StockfishLine> = slots.into_iter().flatten().collect();
    if found.is_empty() {
        return Err("stockfish gave no usable lines".to_string());
    }

    Ok(found)
}

// picks "multipv", "score cp"/"score mate" and the first move of "pv" out of one
// "info depth ... multipv N score cp X ... pv e2e4 ..." line
fn parse_info_line(line: &str) -> Option<StockfishLine> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.first() != Some(&"info") {
        return None;
    }

    let rank = token_after(&tokens, "multipv")?.parse().ok()?;

    let score = if let Some(text) = token_after(&tokens, "cp") {
        StockfishScore::Centipawns(text.parse().ok()?)
    } else if let Some(text) = token_after(&tokens, "mate") {
        StockfishScore::MateIn(text.parse().ok()?)
    } else {
        return None;
    };

    let best_move = token_after(&tokens, "pv")?.to_string();

    Some(StockfishLine { rank, best_move, score })
}

fn token_after<'a>(tokens: &[&'a str], key: &str) -> Option<&'a str> {
    let position = tokens.iter().position(|&token| token == key)?;
    tokens.get(position + 1).copied()
}
