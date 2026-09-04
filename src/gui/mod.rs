// The window: what the app knows, and how a click turns into a move.
// The drawing itself is split off into board_view and info_panel.
//
// The layout is the board on the left and one panel on the right. Everything that
// is text - whose move it is, how the game stands, what the last search cost - sits
// in that panel, stacked top to bottom, so nothing has to share a line with
// anything else and nothing falls off the edge of a narrow window.

mod board_view;
mod info_panel;
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
    duration: Duration,
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
}

impl ChessApp {
    fn new(settings: Settings) -> Self {
        let mut board = Board::new();
        board.set_start_position();

        let mut app = ChessApp {
            board,
            settings,
            selected: None,
            legal_targets: Vec::new(),
            status: String::new(),
            tone: Tone::Calm,
            evaluation: None,
            phase: 1.0,
            last_search: None,
        };
        app.position_changed();
        app
    }

    fn reset(&mut self) {
        *self = ChessApp::new(self.settings);
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
        self.refresh_evaluation();
        self.run_search();
    }

    // weighs the position as it now stands
    fn refresh_evaluation(&mut self) {
        self.phase = game_phase_of(&self.board);

        self.evaluation = if self.game_over() {
            None
        } else {
            Some(self.white_view(evaluate(&self.board)))
        };
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
        let result = search::find_best_move(&mut self.board, depth);
        let duration = start.elapsed();

        self.last_search = Some(SearchStats {
            depth,
            best_move: result.best_move,
            score: self.white_view(result.score),
            positions_searched: result.positions_searched,
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
