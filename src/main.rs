// entry point: wires up the default settings and starts the GUI

mod board;
mod clock;
mod gui;
mod opening;
mod search;
mod evaluate;
mod stockfish;
mod stockfish_library;
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
    // what bounds a move the engine plays: a depth, a time per move, or a game clock
    pub time_control: clock::TimeControl,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            search_depth: search::DEFAULT_DEPTH,
            table_megabytes: transposition::TranspositionTable::DEFAULT_MEGABYTES,
            use_opening_book: true,
            // a depth by default, which leaves the app as it was before there were clocks
            time_control: clock::TimeControl::Depth,
        }
    }
}

fn main() -> eframe::Result {
    let mut args = std::env::args().skip(1);
    let command = args.next();

    // `chess_engine eval-fen "<fen>" <depth> [table_megabytes] [time_ms]` searches one
    // position and prints the result as a single "key=value ..." line - used by the
    // benchmark script (scripts/benchmark.py) to compare this engine against Stockfish
    if command.as_deref() == Some("eval-fen") {
        let fen = args.next();
        let depth = args.next();
        let table_megabytes = args.next();
        let time_ms = args.next();
        eval_fen(fen, depth, table_megabytes, time_ms);
        return Ok(());
    }

    gui::run(Settings::default())
}

fn eval_fen(
    fen: Option<String>,
    depth: Option<String>,
    table_megabytes: Option<String>,
    time_ms: Option<String>,
) {
    let usage = "usage: chess_engine eval-fen <fen> <depth> [table_megabytes] [time_ms]";

    let Some(fen) = fen else {
        println!("error={usage}");
        std::process::exit(1);
    };
    let Some(depth) = depth.as_deref().and_then(|text| text.parse::<u32>().ok()) else {
        println!("error={usage}");
        std::process::exit(1);
    };
    let table_megabytes = table_megabytes
        .as_deref()
        .and_then(|text| text.parse::<usize>().ok())
        .unwrap_or(transposition::TranspositionTable::DEFAULT_MEGABYTES);

    // with a time given, the depth is only a ceiling: the search deepens until the clock
    // runs out. 0, or nothing at all, searches to the depth as before
    let budget = time_ms
        .as_deref()
        .and_then(|text| text.parse::<u64>().ok())
        .filter(|&milliseconds| milliseconds > 0)
        .map(std::time::Duration::from_millis);

    let mut board = match board::Board::from_fen(&fen) {
        Ok(board) => board,
        Err(error) => {
            println!("error={error}");
            std::process::exit(1);
        }
    };

    let mut engine = search::Search::without_book(table_megabytes);

    let limits = match budget {
        Some(budget) => search::SearchLimits {
            max_depth: depth.min(search::MAX_SEARCH_DEPTH),
            ..search::SearchLimits::timed(budget)
        },
        None => search::SearchLimits::depth(depth),
    };

    let started = std::time::Instant::now();
    let result = engine.find_best_move_limited(&mut board, &limits);
    let elapsed = started.elapsed();

    let best_move = result
        .best_move
        .map(|chess_move| chess_move.coordinates())
        .unwrap_or_else(|| "none".to_string());
    let nodes = result.positions_searched + result.positions_searched_quiescience;

    println!(
        "bestmove={best_move} score={} depth={} nodes={nodes} time_ms={}",
        result.score,
        result.depth,
        elapsed.as_millis()
    );
}
