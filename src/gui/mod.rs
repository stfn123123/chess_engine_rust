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
use crate::board::piece::PieceType;
use crate::search;

// the panel is a fixed width, the board gets whatever is left over
const PANEL_WIDTH: f32 = 264.0;
const GAP: f32 = 18.0;
// below this the board is unusable, so it stops shrinking with the window
const MIN_BOARD_SIZE: f32 = 240.0;

// what the last search run cost, as shown in the side panel
struct SearchStats {
    depth: u32,
    positions_found: u64,
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
            last_search: None,
        };
        app.refresh_status();
        app.run_search();
        app
    }

    fn reset(&mut self) {
        *self = ChessApp::new(self.settings);
    }

    // the game has ended, so no more moves are taken
    fn game_over(&self) -> bool {
        self.tone == Tone::Over
    }

    // re-runs the position search and records how long it took; called whenever
    // the position on the board changes
    fn run_search(&mut self) {
        let depth = self.settings.search_depth;
        let start = Instant::now();
        let result = search::count_positions(&mut self.board, depth);

        self.last_search = Some(SearchStats {
            depth,
            positions_found: result.positions_found,
            positions_searched: result.positions_searched,
            duration: start.elapsed(),
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
                self.refresh_status();
                self.run_search();
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
