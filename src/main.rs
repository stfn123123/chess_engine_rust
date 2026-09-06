// A chess engine.
//
// This file is the entry point and nothing else: it puts together the settings the

mod board;
mod gui;
mod opening;
mod search;
mod evaluate;
mod transposition;

// the knobs the engine runs with
#[derive(Clone, Copy)]
pub struct Settings {
    // how deep the search runs after every move; one ply more multiplies the work by
    // roughly the number of legal moves in a position, less what the pruning saves
    pub search_depth: u32,
    // how much memory the transposition table gets - the bigger it is, the fewer of
    // the positions it kept get thrown out to make room for another one
    pub table_megabytes: usize,
    // whether the opening book answers the first moves of a game; off, the engine
    // searches its way out of the opening like it searches everything else
    pub use_opening_book: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            search_depth: search::DEFAULT_DEPTH,
            table_megabytes: transposition::TranspositionTable::DEFAULT_MEGABYTES,
            use_opening_book: true,
        }
    }
}

fn main() -> eframe::Result {
    gui::run(Settings::default())
}
