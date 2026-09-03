// Drawing the board and turning clicks on it into moves.

use eframe::egui;

use super::ChessApp;
use super::theme::{DARK_SQUARE, LEGAL_TARGET_DOT, LIGHT_SQUARE, SELECTED_OUTLINE};
use crate::board::piece::{Color, Piece, PieceType};

// draws the eight by eight grid, with rank 1 at the bottom, and forwards clicks
pub fn show(app: &mut ChessApp, ui: &mut egui::Ui, board_size: f32) {
    let cell = board_size / 8.0;

    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(board_size, board_size), egui::Sense::hover());
    let origin = rect.min;

    for row in 0..8u8 {
        for col in 0..8u8 {
            // row 0 is drawn at the top and holds rank 8
            let rank = 7 - row;
            let file = col;
            let square = rank * 8 + file;

            let cell_rect = egui::Rect::from_min_size(
                origin + egui::vec2(col as f32 * cell, row as f32 * cell),
                egui::vec2(cell, cell),
            );

            let is_light = (rank + file) % 2 == 1;
            let occupant = app.board.piece_at(square);

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
            if app.legal_targets.contains(&square) {
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

            if app.selected == Some(square) {
                ui.painter().rect_stroke(
                    cell_rect,
                    0.0,
                    egui::Stroke::new(3.0, SELECTED_OUTLINE),
                    egui::StrokeKind::Inside,
                );
            }

            let response = ui.interact(
                cell_rect,
                egui::Id::new(("square", square)),
                egui::Sense::click(),
            );
            if response.clicked() {
                app.handle_click(square);
            }
        }
    }
}

// maps a piece to its pre-embedded SVG image (assets/chess-pieces-svg)
fn piece_image(piece: Piece) -> egui::Image<'static> {
    let source = match (piece.piece_type(), piece.color()) {
        (PieceType::King, Color::White) => {
            egui::include_image!("../../assets/chess-pieces-svg/king-w.svg")
        }
        (PieceType::King, Color::Black) => {
            egui::include_image!("../../assets/chess-pieces-svg/king-b.svg")
        }
        (PieceType::Pawn, Color::White) => {
            egui::include_image!("../../assets/chess-pieces-svg/pawn-w.svg")
        }
        (PieceType::Pawn, Color::Black) => {
            egui::include_image!("../../assets/chess-pieces-svg/pawn-b.svg")
        }
        (PieceType::Knight, Color::White) => {
            egui::include_image!("../../assets/chess-pieces-svg/knight-w.svg")
        }
        (PieceType::Knight, Color::Black) => {
            egui::include_image!("../../assets/chess-pieces-svg/knight-b.svg")
        }
        (PieceType::Bishop, Color::White) => {
            egui::include_image!("../../assets/chess-pieces-svg/bishop-w.svg")
        }
        (PieceType::Bishop, Color::Black) => {
            egui::include_image!("../../assets/chess-pieces-svg/bishop-b.svg")
        }
        (PieceType::Rook, Color::White) => {
            egui::include_image!("../../assets/chess-pieces-svg/rook-w.svg")
        }
        (PieceType::Rook, Color::Black) => {
            egui::include_image!("../../assets/chess-pieces-svg/rook-b.svg")
        }
        (PieceType::Queen, Color::White) => {
            egui::include_image!("../../assets/chess-pieces-svg/queen-w.svg")
        }
        (PieceType::Queen, Color::Black) => {
            egui::include_image!("../../assets/chess-pieces-svg/queen-b.svg")
        }
    };
    egui::Image::new(source)
}
