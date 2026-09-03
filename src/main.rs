// A chess engine.
//
// This file is the entry point and nothing else: it puts together the settings the

mod board;
mod gui;
mod search;
mod evaluate;

// the knobs the engine runs with
#[derive(Clone, Copy)]
pub struct Settings {
    // how deep the position count runs after every move; one ply more multiplies the
    // work by roughly the number of legal moves in a position
    pub search_depth: u32,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            search_depth: search::DEFAULT_DEPTH,
        }
    }
}

fn main() -> eframe::Result {
    gui::run(Settings::default())
}
