// The UCI side of the engine: what lichess-bot, or any UCI GUI, talks to.
//
// One loop over stdin, with a Board and a Search kept for the whole game so the
// transposition table survives between moves. Nothing here searches or evaluates - it
// parses commands, turns a reported clock into a budget, and prints what the search found.

use std::io::BufRead;
use std::time::{Duration, Instant};

use crate::board::Board;
use crate::board::chess_move::parse_coordinates;
use crate::board::piece::Color;
use crate::clock::{GameClock, TimeControl};
use crate::evaluate::{MATE, MATE_BOUND};
use crate::search::{self, Search, SearchLimits, SearchResult};
use crate::transposition::TranspositionTable;

const NAME: &str = env!("CARGO_PKG_NAME");
const VERSION: &str = env!("CARGO_PKG_VERSION");
const AUTHOR: &str = "stfn123123";

// held back from a reported clock before any budgeting: the move still has to travel to
// the server, which the reserve inside clock.rs is too small to cover
const DEFAULT_MOVE_OVERHEAD: Duration = Duration::from_millis(300);

// reads commands until stdin ends or `quit` arrives
pub fn run() {
    let mut engine = Engine::new();
    let stdin = std::io::stdin();

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };

        if !engine.handle(line.trim()) {
            break;
        }
    }
}

struct Engine {
    board: Board,
    // kept across moves, so the table and the book outlive one search
    search: Search,
    table_megabytes: usize,
    use_book: bool,
    move_overhead: Duration,
}

impl Engine {
    fn new() -> Engine {
        let table_megabytes = TranspositionTable::DEFAULT_MEGABYTES;
        let mut board = Board::new();
        board.set_start_position();

        Engine {
            board,
            search: Search::new(table_megabytes),
            table_megabytes,
            use_book: true,
            move_overhead: DEFAULT_MOVE_OVERHEAD,
        }
    }

    // false once the loop should end; an unknown command is ignored, as the protocol asks
    fn handle(&mut self, line: &str) -> bool {
        let words: Vec<&str> = line.split_whitespace().collect();
        let Some(&command) = words.first() else {
            return true;
        };
        let rest = &words[1..];

        match command {
            "uci" => self.identify(),
            "isready" => println!("readyok"),
            "ucinewgame" => self.new_game(),
            "setoption" => self.set_option(rest),
            "position" => self.set_position(rest),
            "go" => self.go(rest),
            // a search here runs to the end before the next line is read, so the move was
            // printed long before a stop could arrive - there is never one to interrupt
            "stop" | "ponderhit" => {}
            "quit" => return false,
            _ => {}
        }

        true
    }

    fn identify(&self) {
        println!("id name {NAME} {VERSION}");
        println!("id author {AUTHOR}");
        println!(
            "option name Hash type spin default {} min 1 max 4096",
            TranspositionTable::DEFAULT_MEGABYTES
        );
        println!("option name OwnBook type check default true");
        println!(
            "option name Move Overhead type spin default {} min 0 max 5000",
            DEFAULT_MOVE_OVERHEAD.as_millis()
        );
        println!("uciok");
    }

    // a new game gets a new table: the old one is about positions this game never reaches
    fn new_game(&mut self) {
        let mut board = Board::new();
        board.set_start_position();

        self.board = board;
        self.search = self.fresh_search();
    }

    fn fresh_search(&self) -> Search {
        match self.use_book {
            true => Search::new(self.table_megabytes),
            false => Search::without_book(self.table_megabytes),
        }
    }

    // `setoption name <name> value <value>`, where the name can have spaces in it and so
    // runs up to the "value" keyword rather than to the next word
    fn set_option(&mut self, words: &[&str]) {
        let Some(name_at) = words.iter().position(|&word| word == "name") else {
            return;
        };
        let value_at = words
            .iter()
            .position(|&word| word == "value")
            .filter(|&at| at > name_at);

        let name = words[name_at + 1..value_at.unwrap_or(words.len())].join(" ");
        let value = match value_at {
            Some(at) => words[at + 1..].join(" "),
            None => String::new(),
        };

        match name.as_str() {
            // the table is built with the game, so a size set mid-game waits for the next
            "Hash" => {
                if let Ok(megabytes) = value.parse() {
                    self.table_megabytes = megabytes;
                }
            }
            "OwnBook" => {
                self.use_book = value == "true";
                self.search = self.fresh_search();
            }
            "Move Overhead" => {
                if let Ok(milliseconds) = value.parse() {
                    self.move_overhead = Duration::from_millis(milliseconds);
                }
            }
            _ => {}
        }
    }

    // `position startpos|fen <fields> [moves e2e4 e7e5 ...]`
    fn set_position(&mut self, words: &[&str]) {
        let moves_at = words.iter().position(|&word| word == "moves");
        let position = &words[..moves_at.unwrap_or(words.len())];

        let board = match position.first() {
            Some(&"startpos") => {
                let mut board = Board::new();
                board.set_start_position();
                board
            }
            Some(&"fen") => match Board::from_fen(&position[1..].join(" ")) {
                Ok(board) => board,
                // keeping the last good board would answer a later `go` about the wrong
                // position, so say so and leave this command having changed nothing
                Err(error) => {
                    println!("info string bad fen: {error}");
                    return;
                }
            },
            _ => return,
        };

        self.board = board;

        let Some(moves_at) = moves_at else {
            return;
        };

        for text in &words[moves_at + 1..] {
            let Some((from, to, promotion)) = parse_coordinates(text) else {
                println!("info string bad move: {text}");
                return;
            };
            self.board.make_move_from_squares(from, to, promotion);
        }
    }

    fn go(&mut self, words: &[&str]) {
        let limits = self.limits_from(words);
        let result = self.search.find_best_move_limited(&mut self.board, &limits);

        report(&result);
    }

    // what the `go` line allows, as the search understands it
    fn limits_from(&self, words: &[&str]) -> SearchLimits {
        // nothing here can be interrupted mid-search, so "search until told to stop" is
        // answered with the default depth rather than with silence
        if words.contains(&"infinite") {
            return SearchLimits::depth(search::DEFAULT_DEPTH);
        }

        let mut depth = None;
        let mut movetime = None;
        let mut remaining = [None, None];
        let mut increment = [Duration::ZERO; 2];

        for pair in words.windows(2) {
            let value = pair[1].parse::<u64>().ok();

            match pair[0] {
                "depth" => depth = value,
                "movetime" => movetime = value.map(Duration::from_millis),
                "wtime" => remaining[Color::White.index()] = value.map(Duration::from_millis),
                "btime" => remaining[Color::Black.index()] = value.map(Duration::from_millis),
                "winc" => {
                    increment[Color::White.index()] =
                        value.map_or(Duration::ZERO, Duration::from_millis)
                }
                "binc" => {
                    increment[Color::Black.index()] =
                        value.map_or(Duration::ZERO, Duration::from_millis)
                }
                _ => {}
            }
        }

        let side = self.board.turn();
        let ceiling = depth
            .map(|plies| (plies as u32).min(search::MAX_SEARCH_DEPTH))
            .unwrap_or(search::MAX_SEARCH_DEPTH);

        // an explicit movetime beats the clock, and both are spent net of the overhead
        let budget = match movetime {
            Some(movetime) => Some(movetime.saturating_sub(self.move_overhead)),
            None => remaining[side.index()].map(|_| self.budget_from(remaining, increment, side)),
        };

        match budget {
            // under a deadline the depth is only a ceiling: the search deepens until the
            // budget is gone, the same way the benchmark runs it
            Some(budget) => SearchLimits {
                max_depth: ceiling,
                deadline: Some(Instant::now() + budget),
            },
            // no clock and no movetime, so a depth is all that bounds it
            None => SearchLimits::depth(depth.map_or(search::DEFAULT_DEPTH, |_| ceiling)),
        }
    }

    // clock.rs already knows how much of a clock one move may have, so the tuned
    // constants stay in one place rather than being copied for lichess
    fn budget_from(
        &self,
        remaining: [Option<Duration>; 2],
        increment: [Duration; 2],
        side: Color,
    ) -> Duration {
        let left = |color: Color| {
            remaining[color.index()]
                .unwrap_or(Duration::ZERO)
                .saturating_sub(self.move_overhead)
        };

        let clock = GameClock::with_remaining(left(Color::White), left(Color::Black));

        clock.budget_for(
            side,
            // only the increment is read out of the control; the clocks hold the rest
            TimeControl::Clock {
                base: Duration::ZERO,
                increment: increment[side.index()],
            },
        )
    }
}

// what the search found, as UCI: one info line per pass of the deepening, then the move.
// The lines go out after the search rather than during it, which is all a GUI needs -
// they are read off the pipe before the bestmove either way
fn report(result: &SearchResult) {
    if result.from_book {
        println!("info string book move");
    }

    for pass in &result.passes {
        let Some(best) = pass.best_move else { continue };

        println!(
            "info depth {} score {} nodes {} time {} pv {}",
            pass.depth,
            score_text(pass.score),
            pass.positions_searched,
            pass.elapsed.as_millis(),
            best.coordinates()
        );
    }

    match result.best_move {
        Some(best) => println!("bestmove {}", best.coordinates()),
        // checkmate or stalemate on the board: the protocol still wants an answer
        None => println!("bestmove 0000"),
    }
}

// the search scores a mate as MATE less the ply it lands on; UCI counts moves, and wants
// a mate reported as a distance rather than as a centipawn score no eval could reach
fn score_text(score: i32) -> String {
    if score.abs() < MATE_BOUND {
        return format!("cp {score}");
    }

    let plies = MATE - score.abs();
    let moves = (plies + 1) / 2;

    format!("mate {}", if score > 0 { moves } else { -moves })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_score_is_centipawns() {
        assert_eq!(score_text(35), "cp 35");
        assert_eq!(score_text(-120), "cp -120");
    }

    // a mate on the ply after ours is mate in one, and the losing side reports it negative
    #[test]
    fn a_mate_score_is_a_distance_in_moves() {
        assert_eq!(score_text(MATE - 1), "mate 1");
        assert_eq!(score_text(MATE - 3), "mate 2");
        assert_eq!(score_text(-(MATE - 3)), "mate -2");
    }

    #[test]
    fn moves_are_replayed_onto_the_position() {
        let mut engine = Engine::new();
        engine.handle("position startpos moves e2e4 e7e5");

        assert_eq!(engine.board.turn(), Color::White);
        assert!(engine.board.piece_at(28).is_some(), "e4 is empty");
    }

    // the board must be left alone by a position it could not parse, or a later `go`
    // answers about a position the GUI never asked for
    #[test]
    fn a_bad_fen_changes_nothing() {
        let mut engine = Engine::new();
        engine.handle("position startpos moves d2d4");
        let before = engine.board.to_fen();

        engine.handle("position fen not-a-fen at all 0 1");

        assert_eq!(engine.board.to_fen(), before);
    }

    #[test]
    fn an_option_name_may_have_spaces_in_it() {
        let mut engine = Engine::new();
        engine.handle("setoption name Move Overhead value 900");

        assert_eq!(engine.move_overhead, Duration::from_millis(900));
    }

    #[test]
    fn a_depth_only_go_is_bounded_by_that_depth() {
        let engine = Engine::new();
        let limits = engine.limits_from(&["depth", "4"]);

        assert_eq!(limits.max_depth, 4);
        assert!(limits.deadline.is_none());
    }

    // the clock lichess reports has to come back as a budget, with the depth left open
    #[test]
    fn a_clock_becomes_a_deadline() {
        let engine = Engine::new();
        let limits = engine.limits_from(&["wtime", "60000", "btime", "60000", "winc", "0"]);

        assert_eq!(limits.max_depth, search::MAX_SEARCH_DEPTH);

        let deadline = limits.deadline.expect("a clock gave no deadline");
        let budget = deadline - Instant::now();

        // a twentieth of the minute that is left, near enough either way
        assert!(
            budget > Duration::from_millis(2_500) && budget < Duration::from_millis(3_500),
            "{budget:?} out of a 60s clock"
        );
    }

    // a clock this short is all overhead, and a search still has to come back with a move
    #[test]
    fn a_nearly_dead_clock_still_gets_a_deadline() {
        let engine = Engine::new();
        let limits = engine.limits_from(&["wtime", "50", "btime", "60000"]);

        assert!(limits.deadline.is_some());
    }
}
