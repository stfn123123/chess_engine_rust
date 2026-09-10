// A small library of positions with Stockfish's answer already worked out for them,
// stored as plain text so the GUI can load it instantly rather than spawning
// Stockfish again for a position it has already been asked about.
//
// The file lives at assets/stockfish_library.txt and is written by
// scripts/generate_library.py, one line per position:
//
//   <id> <TAB> <fen> <TAB> <phase> <TAB> <move 1> <TAB> ... <TAB> <move 5> <TAB> <eval>
//
// `id` is a sequential number assigned when the position was first added. `phase` is
// the game phase (0.0 bare kings, 1.0 a full board) the same way evaluate::game_phase_of
// computes it, just worked out from the FEN in Python instead. `eval` is Stockfish's
// score for the position - the same number its best move scored - written as
// `cp<centipawns>` or `mate<moves>` so it parses back exactly, unlike the "+0.30" the
// panel prints for a person to read.

use std::fs;
use std::path::PathBuf;

use crate::stockfish::StockfishScore;

const FILE_NAME: &str = "stockfish_library.txt";

#[derive(Clone)]
pub struct LibraryPosition {
    pub id: u32,
    pub fen: String,
    pub phase: f32,
    pub moves: Vec<String>,
    pub eval: StockfishScore,
}

// whatever the library file holds - empty when it has not been generated yet
pub fn load() -> Vec<LibraryPosition> {
    let Some(path) = file_path() else {
        return Vec::new();
    };
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };

    text.lines().filter_map(parse_line).collect()
}

fn parse_line(line: &str) -> Option<LibraryPosition> {
    let fields: Vec<&str> = line.split('\t').collect();
    // id, fen, phase, at least one move, and the eval
    if fields.len() < 5 {
        return None;
    }

    let id = fields[0].parse().ok()?;
    let fen = fields[1].to_string();
    let phase = fields[2].parse().ok()?;
    let eval = parse_eval(fields[fields.len() - 1])?;
    let moves = fields[3..fields.len() - 1].iter().map(|&text| text.to_string()).collect();

    Some(LibraryPosition { id, fen, phase, moves, eval })
}

fn parse_eval(text: &str) -> Option<StockfishScore> {
    if let Some(value) = text.strip_prefix("cp") {
        return value.parse().ok().map(StockfishScore::Centipawns);
    }
    if let Some(value) = text.strip_prefix("mate") {
        return value.parse().ok().map(StockfishScore::MateIn);
    }
    None
}

// <project>/assets, alongside the opening books and the stockfish binary
fn file_path() -> Option<PathBuf> {
    Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets").join(FILE_NAME))
}
