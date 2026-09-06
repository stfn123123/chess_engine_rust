// Where the stored positions live between runs.
//
// A position is written down as the moves that lead to it and replayed from the
// starting position when it is read back - that keeps its history, which a diagram of
// where the pieces stand would lose along with the repetition and fifty move counts.

use std::fs;
use std::path::PathBuf;

use super::SavedPosition;
use crate::board::Board;
use crate::board::chess_move::parse_coordinates;

const APP_DIRECTORY: &str = "chess_engine";
const FILE_NAME: &str = "positions.txt";

// one line per position: its label, a tab, then the moves, oldest first
pub fn load() -> Vec<SavedPosition> {
    let Some(path) = file_path() else {
        return Vec::new();
    };
    // nothing stored yet is the usual case, not something to report
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };

    text.lines().filter_map(read_line).collect()
}

// the whole list every time, so forgetting one is written down like storing one
pub fn save(positions: &[SavedPosition]) {
    let Some(path) = file_path() else {
        return;
    };
    if let Some(directory) = path.parent()
        && fs::create_dir_all(directory).is_err()
    {
        return;
    }

    let mut text = String::new();
    for saved in positions {
        let moves: Vec<String> = saved
            .board
            .moves_played()
            .iter()
            .map(|chess_move| chess_move.coordinates())
            .collect();

        text.push_str(&saved.label);
        text.push('\t');
        text.push_str(&moves.join(" "));
        text.push('\n');
    }

    // a position that cannot be written down is not worth interrupting the game for
    let _ = fs::write(path, text);
}

fn read_line(line: &str) -> Option<SavedPosition> {
    let (label, moves) = line.split_once('\t')?;

    let mut board = Board::new();
    board.set_start_position();

    for text in moves.split_whitespace() {
        let (from, to, promotion) = parse_coordinates(text)?;

        // an edited or outdated file could name a move that is not legal here, and
        // playing one of those would bring the program down rather than only lose the
        // line it stands on
        let legal = board.legal_moves().into_iter().any(|candidate| {
            candidate.from == from && candidate.to == to && candidate.promotion == promotion
        });
        if !legal {
            return None;
        }

        board.make_move_from_squares(from, to, promotion);
    }

    Some(SavedPosition {
        board,
        label: label.to_string(),
    })
}

// %APPDATA%\chess_engine\positions.txt on windows, ~/.config/chess_engine/... elsewhere
fn file_path() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
    }?;

    Some(base.join(APP_DIRECTORY).join(FILE_NAME))
}
