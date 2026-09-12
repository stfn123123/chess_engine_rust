#![allow(dead_code)]
// entry point of the UCI binary: the engine with no GUI at all, which is what lichess-bot runs.
//
// Only the modules a search needs are declared here. gui, stockfish and the settings the
// panel reads are left out, so nothing eframe touches is linked into this binary and a
// fault on the GUI side cannot reach a game being played.

mod board;
mod clock;
mod evaluate;
mod opening;
mod search;
mod transposition;
mod uci;

fn main() {
    uci::run();
}
