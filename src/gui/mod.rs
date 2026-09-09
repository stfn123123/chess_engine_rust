// The window: what the app knows, and how a click turns into a move.
// The drawing itself is split off into board_view and info_panel.
//
// The layout is the board on the left and one panel on the right. Everything that
// is text - whose move it is, how the game stands, what the last search cost - sits
// in that panel, stacked top to bottom, so nothing has to share a line with
// anything else and nothing falls off the edge of a narrow window.

mod board_view;
mod info_panel;
mod storage;
mod theme;

use eframe::egui;
use std::time::{Duration, Instant};

use self::theme::{APP_BG, TEXT_PRIMARY};
use crate::Settings;
use crate::board::Board;
use crate::board::chess_move::Move;
use crate::board::piece::{Color, PieceType};
use crate::evaluate::{evaluate, game_phase_of};
use crate::search;
use crate::search::DepthPass;

// the panel is a fixed width, wide enough for its two columns, the board gets the rest
const PANEL_WIDTH: f32 = 480.0;
const GAP: f32 = 18.0;
// below this the board is unusable, so it stops shrinking with the window
const MIN_BOARD_SIZE: f32 = 240.0;
// autoplay waits this long after a move lands before playing the next one, so a game
// reads as a sequence of moves rather than a flicker
const AUTOPLAY_DELAY: Duration = Duration::from_millis(800);

// what the last search found, and what it cost, as shown in the side panel
struct SearchStats {
    // the deepest pass of the deepening that finished
    depth: u32,
    // the move the search would play here, None once the game is over
    best_move: Option<Move>,
    // what the search thinks the position is worth, from white's point of view
    score: i32,
    positions_searched: u64,
    // of those, how many were quiescence positions rather than full-depth ones
    positions_searched_quiescience: u64,
    // how many of those the transposition table answered without searching them
    table_cutoffs: u64,
    // how much of the table has been written, 0.0 to 1.0
    table_fill: f32,
    // whether the move came out of the opening book rather than out of a search
    from_book: bool,
    duration: Duration,
    // one entry per pass of the deepening, shallowest first
    passes: Vec<DepthPass>,
    // how many nodes were cut short by a move that beat beta, and how many of those the
    // killers of that ply supplied - the share is what says the killers are working
    beta_cutoffs: u64,
    killer_cutoffs: u64,
}

// what a position count found, and what it cost
struct PerftStats {
    depth: u32,
    positions: u64,
    duration: Duration,
}

// a position put aside from the panel, to come back to later
struct SavedPosition {
    // the whole board, history included, so a recalled position can be played on
    board: Board,
    // what the row in the panel reads, e.g. "#1 White - 32p"
    label: String,
}

// a finished or in-progress game, saved permanently rather than only put aside for
// this session - stepped back through move by move instead of jumped to directly
struct SavedGame {
    // the whole board, history included, so its move list can be replayed from move 1
    board: Board,
    // what the row in the panel reads, e.g. "Game #1 - 34 moves"
    label: String,
}

// a saved game currently being stepped through; its own moves rather than an index
// into saved_games, so forgetting other games while replaying cannot invalidate it
struct Replay {
    // the live game, put aside so exit_replay has it to come back to
    live_board: Board,
    moves: Vec<Move>,
    // how many of them are shown on the board, 0 is the starting position
    ply: usize,
}

// how urgently a status line should read
#[derive(Clone, Copy, PartialEq)]
enum Tone {
    // the game is running as usual
    Calm,
    // someone is in check and has to answer it
    Warning,
    // the game is over, one way or another
    Over,
}

pub struct ChessApp {
    board: Board,
    settings: Settings,
    // the engine, kept across moves: its transposition table is worth more the longer
    // it has been filling up, and the game is one position after another
    engine: search::Search,
    // the square the player picked a piece up from, if any
    selected: Option<u8>,
    // where that piece may legally go, so the board can mark those squares
    legal_targets: Vec<u8>,
    status: String,
    tone: Tone,
    // how the position on the board stands, in centipawns from white's point of
    // view - None once the game is over, when there is nothing left to weigh
    evaluation: Option<i32>,
    // how late the game is: 1.00 on the opening board, 0.00 once the pieces are off
    phase: f32,
    last_search: Option<SearchStats>,
    // how deep the next position count goes, set in the panel
    perft_depth: u32,
    // what the last position count found, dropped as soon as the board changes
    last_perft: Option<PerftStats>,
    // whether the engine weighs and searches after every move - turned off, a position
    // can be set up a move at a time without waiting for a search at every click
    analysis_enabled: bool,
    // when a side's flag is set, the engine plays its own best move the moment it is
    // that side's turn, instead of waiting for a click
    autoplay_white: bool,
    autoplay_black: bool,
    // when the board last changed, so autoplay can pace itself against it rather than
    // playing moves back to back
    last_move_at: Instant,
    // positions put aside to come back to, oldest first
    saved_positions: Vec<SavedPosition>,
    // games saved permanently, oldest first
    saved_games: Vec<SavedGame>,
    // the game currently being stepped through, if any - while this is set the
    // board shows a replay ply rather than the live game
    replay: Option<Replay>,
    // which of the board's bitboards are painted over the position, indexed as they are -
    // a way to watch one through a promotion, an en passant or a castle
    shown_bitboards: [[bool; 6]; 2],
    // whether the attack table's answer for the selected piece is drawn over the board
    show_attacks: bool,
}

impl ChessApp {
    fn new(settings: Settings) -> Self {
        ChessApp::with_state(settings, true, false, false, storage::load(), storage::load_games())
    }

    // a fresh game that keeps what belongs to the session rather than to the game:
    // the analysis toggle, the autoplay toggles, and what was put aside or saved so far
    fn with_state(
        settings: Settings,
        analysis_enabled: bool,
        autoplay_white: bool,
        autoplay_black: bool,
        saved_positions: Vec<SavedPosition>,
        saved_games: Vec<SavedGame>,
    ) -> Self {
        let mut board = Board::new();
        board.set_start_position();

        let engine = if settings.use_opening_book {
            search::Search::new(settings.table_megabytes)
        } else {
            search::Search::without_book(settings.table_megabytes)
        };

        let mut app = ChessApp {
            board,
            settings,
            engine,
            selected: None,
            legal_targets: Vec::new(),
            status: String::new(),
            tone: Tone::Calm,
            evaluation: None,
            phase: 1.0,
            last_search: None,
            perft_depth: 4,
            last_perft: None,
            analysis_enabled,
            autoplay_white,
            autoplay_black,
            last_move_at: Instant::now(),
            saved_positions,
            saved_games,
            replay: None,
            shown_bitboards: [[false; 6]; 2],
            show_attacks: false,
        };
        app.position_changed();
        app
    }

    fn reset(&mut self) {
        let saved_positions = std::mem::take(&mut self.saved_positions);
        let saved_games = std::mem::take(&mut self.saved_games);
        // belongs to the session rather than to the game, like the toggles above it
        let shown_bitboards = self.shown_bitboards;
        let show_attacks = self.show_attacks;
        *self = ChessApp::with_state(
            self.settings,
            self.analysis_enabled,
            self.autoplay_white,
            self.autoplay_black,
            saved_positions,
            saved_games,
        );
        self.shown_bitboards = shown_bitboards;
        self.show_attacks = show_attacks;
    }

    // the game has ended, so no more moves are taken
    fn game_over(&self) -> bool {
        self.tone == Tone::Over
    }

    // status first, since the evaluation asks it whether the game is still running
    fn position_changed(&mut self) {
        self.last_move_at = Instant::now();
        self.refresh_status();
        // the old count belongs to the position that was on the board before this one
        self.last_perft = None;
        // the phase is a property of the position rather than a verdict on it, so it
        // stays up to date even with the engine turned off
        self.phase = game_phase_of(&self.board);
        self.refresh_analysis();
    }

    // old numbers are dropped rather than left standing when analysis is off
    fn refresh_analysis(&mut self) {
        if !self.analysis_enabled {
            self.evaluation = None;
            self.last_search = None;
            return;
        }

        self.analyse_once();
    }

    // one run of the engine on the position as it stands, whatever the toggle says
    fn analyse_once(&mut self) {
        self.refresh_evaluation();
        self.run_search();
    }

    // weighs the position as it now stands
    fn refresh_evaluation(&mut self) {
        self.evaluation = if self.game_over() {
            None
        } else {
            Some(self.white_view(evaluate(&self.board)))
        };
    }

    // turning the engine back on brings it up to date with the board straight away,
    // rather than waiting for the next move to be played
    fn set_analysis(&mut self, enabled: bool) {
        self.analysis_enabled = enabled;
        self.refresh_analysis();
    }

    // puts the position aside, history and all, so it can be come back to and played
    // on from exactly here
    fn store_position(&mut self) {
        let pieces = (0..64)
            .filter(|&square| self.board.piece_at(square).is_some())
            .count();
        let side = match self.board.turn() {
            Color::White => "White",
            Color::Black => "Black",
        };

        self.saved_positions.push(SavedPosition {
            board: self.board.clone(),
            label: format!("#{} {side} - {pieces}p", self.saved_positions.len() + 1),
        });
        storage::save(&self.saved_positions);
    }

    // puts a stored position back on the board; the labels keep the numbers they were
    // stored with, so recalling one does not renumber the rest
    fn recall_position(&mut self, index: usize) {
        let Some(board) = self
            .saved_positions
            .get(index)
            .map(|saved| saved.board.clone())
        else {
            return;
        };

        self.board = board;
        self.clear_selection();
        self.position_changed();
    }

    fn forget_position(&mut self, index: usize) {
        if index < self.saved_positions.len() {
            self.saved_positions.remove(index);
            storage::save(&self.saved_positions);
        }
    }

    // whether a saved game is on the board instead of the live one - moves are not
    // taken and autoplay does not run while one is
    fn is_replaying(&self) -> bool {
        self.replay.is_some()
    }

    // saves the game as it stands, whether finished or not - unlike a stored position
    // this survives being played on, and is meant to be stepped back through later
    fn save_game(&mut self) {
        let moves = self.board.moves_played().len();

        self.saved_games.push(SavedGame {
            board: self.board.clone(),
            label: format!("Game #{} - {moves} moves", self.saved_games.len() + 1),
        });
        storage::save_games(&self.saved_games);
    }

    fn forget_game(&mut self, index: usize) {
        if index < self.saved_games.len() {
            self.saved_games.remove(index);
            storage::save_games(&self.saved_games);
        }
    }

    // starts stepping through a saved game from its first move, putting the live
    // game aside so it can be returned to with exit_replay
    fn start_replay(&mut self, index: usize) {
        let Some(saved) = self.saved_games.get(index) else {
            return;
        };

        self.replay = Some(Replay {
            live_board: self.board.clone(),
            moves: saved.board.moves_played(),
            ply: 0,
        });
        self.apply_replay_ply();
    }

    // leaves replay and puts the live game back on the board exactly as it was
    fn exit_replay(&mut self) {
        let Some(replay) = self.replay.take() else {
            return;
        };

        self.board = replay.live_board;
        self.clear_selection();
        self.position_changed();
    }

    // moves the replay forward or back by `delta` plies, clamped to the game's length
    fn replay_step(&mut self, delta: isize) {
        let Some(replay) = &mut self.replay else {
            return;
        };

        let last_ply = replay.moves.len() as isize;
        replay.ply = (replay.ply as isize + delta).clamp(0, last_ply) as usize;
        self.apply_replay_ply();
    }

    // rebuilds the board from the start position up to the replay's current ply
    fn apply_replay_ply(&mut self) {
        let Some(replay) = &self.replay else {
            return;
        };

        let mut board = Board::new();
        board.set_start_position();
        for chess_move in &replay.moves[..replay.ply] {
            board.make_move(chess_move);
        }

        self.board = board;
        self.clear_selection();
        self.position_changed();
    }

    // scores are for the side to move; the panel always shows white's point of view
    fn white_view(&self, score: i32) -> i32 {
        match self.board.turn() {
            Color::White => score,
            Color::Black => -score,
        }
    }

    // searches the position for the best move and records what that cost; called
    // whenever the position on the board changes
    fn run_search(&mut self) {
        let depth = self.settings.search_depth;
        let start = Instant::now();
        let result = self.engine.find_best_move(&mut self.board, depth);
        let duration = start.elapsed();

        self.last_search = Some(SearchStats {
            // what the search reached, which is not the depth asked for when a mate
            // was proved on the way up and the passes after it were dropped
            depth: result.depth,
            best_move: result.best_move,
            score: self.white_view(result.score),
            positions_searched: result.positions_searched,
            positions_searched_quiescience: result.positions_searched_quiescience,
            table_cutoffs: result.table_cutoffs,
            table_fill: result.table_fill,
            from_book: result.from_book,
            duration,
            passes: result.passes,
            beta_cutoffs: result.beta_cutoffs,
            killer_cutoffs: result.killer_cutoffs,
        });
    }

    // counts every position `perft_depth` plies away from the board as it stands, to
    // check move generation against the published perft numbers
    fn count_positions(&mut self) {
        let depth = self.perft_depth;
        let start = Instant::now();
        let positions = search::count_positions(&mut self.board, depth);
        let duration = start.elapsed();

        self.last_perft = Some(PerftStats {
            depth,
            positions,
            duration,
        });
    }

    // recomputes the status line and how urgently it reads
    fn refresh_status(&mut self) {
        let turn = self.board.turn();

        let (status, tone) = if self.board.is_checkmate() {
            let winner = self.board.winner();
            (format!("Checkmate - {winner:?} wins"), Tone::Over)
        } else if self.board.is_stalemate() {
            ("Stalemate - draw".to_string(), Tone::Over)
        } else if self.board.insufficient_material() {
            ("Draw - insufficient material".to_string(), Tone::Over)
        } else if self.board.is_threefold_repetition() {
            ("Draw - threefold repetition".to_string(), Tone::Over)
        } else if self.board.is_fifty_move_draw() {
            ("Draw - fifty move rule".to_string(), Tone::Over)
        } else if self.board.is_check(turn) {
            (format!("{turn:?} is in check"), Tone::Warning)
        } else {
            ("Game in progress".to_string(), Tone::Calm)
        };

        self.status = status;
        self.tone = tone;
    }

    // handles a click on `square`: either plays the selected piece there, if that is
    // one of its legal destinations, or picks up whatever piece stands on it
    fn handle_click(&mut self, square: u8) {
        if self.game_over() || self.is_replaying() {
            return;
        }

        if let Some(from) = self.selected {
            if self.legal_targets.contains(&square) {
                // the GUI has no promotion dialog yet, so a promoting pawn becomes a queen
                self.board
                    .make_move_from_squares(from, square, Some(PieceType::Queen));
                self.clear_selection();
                self.position_changed();
                return;
            }
        }

        match self.board.piece_at(square) {
            Some(piece) if piece.color() == self.board.turn() => self.select(square),
            _ => self.clear_selection(),
        }
    }

    fn select(&mut self, square: u8) {
        self.selected = Some(square);
        self.legal_targets = self
            .board
            .legal_moves()
            .into_iter()
            .filter(|candidate| candidate.from == square)
            .map(|candidate| candidate.to)
            .collect();
    }

    fn clear_selection(&mut self) {
        self.selected = None;
        self.legal_targets.clear();
    }

    // called once a frame: if the side to move has autoplay on, plays the move the
    // engine found and asks for another frame straight away, so autoplay runs on its
    // own instead of stalling until the next click or keypress
    fn autoplay_step(&mut self, ctx: &egui::Context) {
        if self.game_over() || self.is_replaying() {
            return;
        }

        let enabled = match self.board.turn() {
            Color::White => self.autoplay_white,
            Color::Black => self.autoplay_black,
        };
        if !enabled {
            return;
        }

        // wait out the pause before playing the next move, so the board is readable
        // move to move rather than racing through the game
        let elapsed = self.last_move_at.elapsed();
        if elapsed < AUTOPLAY_DELAY {
            ctx.request_repaint_after(AUTOPLAY_DELAY - elapsed);
            return;
        }

        // analysis may be turned off, in which case nothing has searched this
        // position yet - autoplay needs a move regardless of that toggle
        if self.last_search.is_none() {
            self.analyse_once();
        }

        let Some(best_move) = self.last_search.as_ref().and_then(|stats| stats.best_move) else {
            return;
        };

        self.board.make_move(&best_move);
        self.clear_selection();
        self.position_changed();
        ctx.request_repaint();
    }
}

impl eframe::App for ChessApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.autoplay_step(ui.ctx());

        let available = ui.available_size();
        // the board is square, so it takes the smaller of what is left beside the
        // panel and the height of the window
        let board_size = (available.x - PANEL_WIDTH - GAP)
            .min(available.y)
            .max(MIN_BOARD_SIZE);
        let panel_height = available.y.max(MIN_BOARD_SIZE);

        ui.horizontal_top(|ui| {
            board_view::show(self, ui, board_size);
            ui.add_space(GAP);
            info_panel::show(self, ui, PANEL_WIDTH, panel_height);
        });
    }
}

pub fn run(settings: Settings) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1200.0, 780.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Chess",
        options,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);

            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = APP_BG;
            visuals.window_fill = APP_BG;
            visuals.override_text_color = Some(TEXT_PRIMARY);
            cc.egui_ctx.set_visuals(visuals);

            Ok(Box::new(ChessApp::new(settings)))
        }),
    )
}
