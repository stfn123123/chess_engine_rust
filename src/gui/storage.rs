// Where the stored positions and saved games live between runs.
//
// Both are written down the same way: the moves that lead there, replayed from the
// starting position when read back - that keeps their history, which a diagram of
// where the pieces stand would lose along with the repetition and fifty move counts.
// A position and a game differ only in the file they live in and in what the app
// does with one once it is back on the board.

use std::fs;
use std::path::PathBuf;

use super::{SavedGame, SavedPosition};
use crate::board::Board;
use crate::board::chess_move::parse_coordinates;

const APP_DIRECTORY: &str = "chess_engine";
const POSITIONS_FILE_NAME: &str = "positions.txt";
const GAMES_FILE_NAME: &str = "games.txt";

// one line per position: its label, a tab, then the moves, oldest first
pub fn load() -> Vec<SavedPosition> {
    load_lines(positions_file_path()).map(|(label, board)| SavedPosition { board, label }).collect()
}

// the whole list every time, so forgetting one is written down like storing one
pub fn save(positions: &[SavedPosition]) {
    save_lines(positions_file_path(), positions.iter().map(|saved| (&saved.label, &saved.board)));
}

// a saved game reads and writes exactly like a saved position - what tells the two
// apart is what the app does with one once it is back on the board
pub fn load_games() -> Vec<SavedGame> {
    load_lines(games_file_path()).map(|(label, board)| SavedGame { board, label }).collect()
}

pub fn save_games(games: &[SavedGame]) {
    save_lines(games_file_path(), games.iter().map(|saved| (&saved.label, &saved.board)));
}

fn load_lines(path: Option<PathBuf>) -> impl Iterator<Item = (String, Board)> {
    // nothing stored yet is the usual case, not something to report
    let text = path.and_then(|path| fs::read_to_string(path).ok()).unwrap_or_default();
    text.lines().filter_map(read_line).collect::<Vec<_>>().into_iter()
}

fn save_lines<'a>(path: Option<PathBuf>, items: impl Iterator<Item = (&'a String, &'a Board)>) {
    let Some(path) = path else {
        return;
    };
    if let Some(directory) = path.parent()
        && fs::create_dir_all(directory).is_err()
    {
        return;
    }

    let mut text = String::new();
    for (label, board) in items {
        let moves: Vec<String> = board
            .moves_played()
            .iter()
            .map(|chess_move| chess_move.coordinates())
            .collect();

        text.push_str(label);
        text.push('\t');
        text.push_str(&moves.join(" "));
        text.push('\n');
    }

    // something that cannot be written down is not worth interrupting the game for
    let _ = fs::write(path, text);
}

fn read_line(line: &str) -> Option<(String, Board)> {
    let (label, moves) = line.split_once('\t')?;

    let mut board = Board::new();
    board.set_start_position();

    for text in moves.split_whitespace() {
        let (from, to, promotion) = parse_coordinates(text)?;

        // an edited or outdated file could name a move that isn't legal here
        let legal = board.legal_moves().into_iter().any(|candidate| {
            candidate.from == from && candidate.to == to && candidate.promotion == promotion
        });
        if !legal {
            return None;
        }

        board.make_move_from_squares(from, to, promotion);
    }

    Some((label.to_string(), board))
}

// %APPDATA%\chess_engine on windows, ~/.config/chess_engine elsewhere
fn config_dir() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
    }?;

    Some(base.join(APP_DIRECTORY))
}

fn positions_file_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join(POSITIONS_FILE_NAME))
}

fn games_file_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join(GAMES_FILE_NAME))
}
