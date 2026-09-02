// Visual, interactive chessboard window (replaces the terminal printout).
// Click a piece to see its legal destinations highlighted, then click a
// destination square to play the move. Pawns reaching the last rank always
// promote to a queen.

use crate::board::{Board, Color as PieceColor, Piece, PieceType};
use eframe::egui;

const LIGHT_SQUARE: egui::Color32 = egui::Color32::from_rgb(240, 217, 181);
const DARK_SQUARE: egui::Color32 = egui::Color32::from_rgb(181, 136, 99);
const SELECTED_OUTLINE: egui::Color32 = egui::Color32::from_rgb(246, 246, 105);
const LEGAL_TARGET_DOT: egui::Color32 = egui::Color32::from_rgba_premultiplied(20, 20, 20, 110);

pub struct ChessApp {
    board: Board,
    selected: Option<u8>,
    legal_targets: Vec<u8>,
    status: String,
    game_over: bool,
}

impl ChessApp {
    fn new() -> Self {
        let mut board = Board::new();
        board.create_basic_layout();

        let mut app = ChessApp {
            board,
            selected: None,
            legal_targets: Vec::new(),
            status: String::new(),
            game_over: false,
        };
        app.refresh_status();
        app
    }

    fn reset(&mut self) {
        *self = ChessApp::new();
    }

    // recomputes the status line and whether the game has ended
    fn refresh_status(&mut self) {
        let turn = self.board.turn();

        if self.board.is_checkmate() {
            self.game_over = true;
            let winner = self.board.get_winner();
            self.status = format!("Checkmate - {winner:?} wins");
        } else if self.board.is_stalemate() {
            self.game_over = true;
            self.status = "Stalemate - draw".to_string();
        } else if self.board.insufficient_material() {
            self.game_over = true;
            self.status = "Draw - insufficient material".to_string();
        } else if self.board.is_threefold_repetition() {
            self.game_over = true;
            self.status = "Draw - threefold repetition".to_string();
        } else {
            self.game_over = false;
            let check_suffix = if self.board.is_check(turn) { " - check!" } else { "" };
            self.status = format!("{turn:?} to move{check_suffix}");
        }
    }

    // handles a click on `square`: either selects a piece or, if a piece is
    // already selected and `square` is one of its legal destinations, plays the move
    fn handle_click(&mut self, square: u8) {
        if self.game_over {
            return;
        }

        if self.selected.is_some() && self.legal_targets.contains(&square) {
            let from = self.selected.unwrap();
            self.board.create_move(from, square, Some(PieceType::Queen));
            self.selected = None;
            self.legal_targets.clear();
            self.refresh_status();
            return;
        }

        match self.board.get_piece(square) {
            Some(piece) if piece.color() == self.board.turn() => {
                self.selected = Some(square);
                self.legal_targets = self
                    .board
                    .get_legal_moves(self.board.turn())
                    .into_iter()
                    .filter(|candidate| candidate.from == square)
                    .map(|candidate| candidate.to)
                    .collect();
            }
            _ => {
                self.selected = None;
                self.legal_targets.clear();
            }
        }
    }
}

impl eframe::App for ChessApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.horizontal(|ui| {
            ui.heading("Chess");
            ui.separator();
            ui.label(&self.status);
            if ui.button("New Game").clicked() {
                self.reset();
            }
        });
        ui.separator();

        let available = ui.available_size();
        let board_size = available.x.min(available.y);
        let cell = board_size / 8.0;

        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(board_size, board_size), egui::Sense::hover());
        let origin = rect.min;

        for row in 0..8u8 {
            for col in 0..8u8 {
                let rank = 7 - row;
                let file = col;
                let square = rank * 8 + file;

                let cell_rect = egui::Rect::from_min_size(
                    origin + egui::vec2(col as f32 * cell, row as f32 * cell),
                    egui::vec2(cell, cell),
                );

                let is_light = (rank + file) % 2 == 1;
                let occupant = self.board.get_piece(square);

                ui.painter().rect_filled(
                    cell_rect,
                    0.0,
                    if is_light { LIGHT_SQUARE } else { DARK_SQUARE },
                );

                if let Some(piece) = occupant {
                    piece_image(piece).paint_at(ui, cell_rect.shrink(cell * 0.08));
                }

                // legal-move markers are drawn on top of the piece: a capture gets a ring
                // around the square (a centered dot would be hidden behind the piece),
                // an empty destination gets a small centered dot
                if self.legal_targets.contains(&square) {
                    if occupant.is_some() {
                        ui.painter().circle_stroke(
                            cell_rect.center(),
                            cell * 0.47,
                            egui::Stroke::new(cell * 0.07, LEGAL_TARGET_DOT),
                        );
                    } else {
                        ui.painter()
                            .circle_filled(cell_rect.center(), cell * 0.14, LEGAL_TARGET_DOT);
                    }
                }

                if self.selected == Some(square) {
                    ui.painter().rect_stroke(
                        cell_rect,
                        0.0,
                        egui::Stroke::new(3.0, SELECTED_OUTLINE),
                        egui::StrokeKind::Inside,
                    );
                }

                let response =
                    ui.interact(cell_rect, egui::Id::new(("square", square)), egui::Sense::click());
                if response.clicked() {
                    self.handle_click(square);
                }
            }
        }
    }
}

// maps a piece to its pre-embedded SVG image (assets/chess-pieces-svg)
fn piece_image(piece: Piece) -> egui::Image<'static> {
    let source = match (piece.piece_type(), piece.color()) {
        (1, PieceColor::White) => egui::include_image!("../assets/chess-pieces-svg/king-w.svg"),
        (1, PieceColor::Black) => egui::include_image!("../assets/chess-pieces-svg/king-b.svg"),
        (2, PieceColor::White) => egui::include_image!("../assets/chess-pieces-svg/pawn-w.svg"),
        (2, PieceColor::Black) => egui::include_image!("../assets/chess-pieces-svg/pawn-b.svg"),
        (3, PieceColor::White) => egui::include_image!("../assets/chess-pieces-svg/knight-w.svg"),
        (3, PieceColor::Black) => egui::include_image!("../assets/chess-pieces-svg/knight-b.svg"),
        (4, PieceColor::White) => egui::include_image!("../assets/chess-pieces-svg/bishop-w.svg"),
        (4, PieceColor::Black) => egui::include_image!("../assets/chess-pieces-svg/bishop-b.svg"),
        (5, PieceColor::White) => egui::include_image!("../assets/chess-pieces-svg/rook-w.svg"),
        (5, PieceColor::Black) => egui::include_image!("../assets/chess-pieces-svg/rook-b.svg"),
        (6, PieceColor::White) => egui::include_image!("../assets/chess-pieces-svg/queen-w.svg"),
        (6, PieceColor::Black) => egui::include_image!("../assets/chess-pieces-svg/queen-b.svg"),
        _ => unreachable!("unknown piece type"),
    };
    egui::Image::new(source)
}

pub fn run() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([720.0, 760.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Chess",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(ChessApp::new()))
        }),
    )
}
