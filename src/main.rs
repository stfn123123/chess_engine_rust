// entry point: wires up the default settings and starts the GUI

mod board;
mod gui;
mod opening;
mod search;
mod evaluate;
mod transposition;

// the knobs the engine runs with
#[derive(Clone, Copy)]
pub struct Settings {
    // how deep the search runs after every move
    pub search_depth: u32,
    // how much memory the transposition table gets
    pub table_megabytes: usize,
    // whether the opening book answers the first moves of a game
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
