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

// the panel is a fixed width, the board gets whatever is left over
const PANEL_WIDTH: f32 = 264.0;
const GAP: f32 = 18.0;
// below this the board is unusable, so it stops shrinking with the window
const MIN_BOARD_SIZE: f32 = 240.0;

// what the last search found, and what it cost, as shown in the side panel
struct SearchStats {
    depth: u32,
    // the move the search would play here, None once the game is over
    best_move: Option<Move>,
    // what the search thinks the position is worth, from white's point of view
    score: i32,
    positions_searched: u64,
    // how many of those the transposition table answered without searching them
    table_cutoffs: u64,
    // how much of the table has been written, 0.0 to 1.0
    table_fill: f32,
    // whether the move came out of the opening book rather than out of a search
    from_book: bool,
    duration: Duration,
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
    // positions put aside to come back to, oldest first
    saved_positions: Vec<SavedPosition>,
}

impl ChessApp {
    fn new(settings: Settings) -> Self {
        ChessApp::with_state(settings, true, storage::load())
    }

    // a fresh game that keeps what belongs to the session rather than to the game:
    // the analysis toggle and the positions put aside so far
    fn with_state(
        settings: Settings,
        analysis_enabled: bool,
        saved_positions: Vec<SavedPosition>,
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
            saved_positions,
        };
        app.position_changed();
        app
    }

    fn reset(&mut self) {
        let saved = std::mem::take(&mut self.saved_positions);
        *self = ChessApp::with_state(self.settings, self.analysis_enabled, saved);
    }

    // the game has ended, so no more moves are taken
    fn game_over(&self) -> bool {
        self.tone == Tone::Over
    }

    // everything that is worked out from the position and nothing else, in the one
    // order that works: the status first, because the evaluation asks it whether the
    // game is still running
    fn position_changed(&mut self) {
        self.refresh_status();
        // the old count belongs to the position that was on the board before this one
        self.last_perft = None;
        // the phase is a property of the position rather than a verdict on it, so it
        // stays up to date even with the engine turned off
        self.phase = game_phase_of(&self.board);
        self.refresh_analysis();
    }

    // what the engine has to say about the position, or nothing at all when it is
    // turned off - the old numbers are dropped rather than left standing, so nothing
    // on screen ever belongs to a position other than the one on the board
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

    // the search and the evaluation both score for the side to move, the panel shows
    // white's point of view - otherwise the sign would flip with every move and the
    // number would say more about whose turn it is than about the position
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
            depth,
            best_move: result.best_move,
            score: self.white_view(result.score),
            positions_searched: result.positions_searched,
            table_cutoffs: result.table_cutoffs,
            table_fill: result.table_fill,
            from_book: result.from_book,
            duration,
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
        if self.game_over() {
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
}

impl eframe::App for ChessApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
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
        viewport: egui::ViewportBuilder::default().with_inner_size([980.0, 780.0]),
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
