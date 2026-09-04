// The right-hand panel: whose move it is, how the game stands, and what the last
// search at the current position cost.
//
// Everything in here is one column, top to bottom, and the whole column scrolls -
// so a narrow or short window hides nothing, it only asks to be scrolled.

use eframe::egui;
use std::time::Duration;

use super::theme::{
    ACCENT, BLACK_SIDE, CALM, DANGER, PANEL_BG, PANEL_BORDER, STAT_EVAL, STAT_SPEED, STAT_TIME,
    TEXT_MUTED, TEXT_PRIMARY, WARNING, WHITE_SIDE,
};
use super::{ChessApp, Tone};
use crate::board::chess_move::Move;
use crate::board::piece::Color;
use crate::board::square::{file_of, rank_of};
use crate::evaluate::MATE;

pub fn show(app: &mut ChessApp, ui: &mut egui::Ui, width: f32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    ui.painter().rect_filled(rect, 8.0, PANEL_BG);
    ui.painter().rect_stroke(
        rect,
        8.0,
        egui::Stroke::new(1.0, PANEL_BORDER),
        egui::StrokeKind::Inside,
    );

    // everything below is laid out inside the panel, not next to it
    // the layout has to be spelled out: a UiBuilder without one inherits the parent's,
    // and the parent is the horizontal row that puts this panel beside the board -
    // which would lay every label out side by side instead of stacking them
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink(18.0))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let ui = &mut child;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // the scroll area copies the layout it was given, but say it again so this
            // block cannot be broken by whatever encloses the panel later on
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new("CHESS")
                        .size(20.0)
                        .strong()
                        .color(TEXT_PRIMARY),
                );
                ui.add_space(12.0);
                divider(ui);
                ui.add_space(16.0);

                turn_block(app, ui);
                status_block(app, ui);
                evaluation_block(app, ui);
                phase_block(app, ui);

                divider(ui);
                ui.add_space(16.0);

                search_blocks(app, ui);

                ui.add_space(4.0);
                if new_game_button(ui) {
                    app.reset();
                }
            });
        });
}

// whose move it is, as a disc in that side's colour next to its name
fn turn_block(app: &ChessApp, ui: &mut egui::Ui) {
    let turn = app.board.turn();
    let (name, disc) = match turn {
        Color::White => ("White", WHITE_SIDE),
        Color::Black => ("Black", BLACK_SIDE),
    };

    label(ui, "TO MOVE");
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        // the disc gets an outline, otherwise the black one disappears into the panel
        let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 8.0, disc);
        ui.painter()
            .circle_stroke(rect.center(), 8.0, egui::Stroke::new(1.0, PANEL_BORDER));

        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(name)
                .font(egui::FontId::proportional(19.0))
                .color(TEXT_PRIMARY),
        );
    });
    ui.add_space(18.0);
}

// how the game stands, coloured by how much attention it wants
fn status_block(app: &ChessApp, ui: &mut egui::Ui) {
    let color = match app.tone {
        Tone::Calm => CALM,
        Tone::Warning => WARNING,
        Tone::Over => DANGER,
    };

    label(ui, "STATUS");
    ui.add_space(4.0);
    // wrapped, so a long line like "Draw - insufficient material" stays in the panel
    ui.label(
        egui::RichText::new(&app.status)
            .size(15.0)
            .strong()
            .color(color),
    );
    ui.add_space(18.0);
}

// how the position stands after the last move, in pawns from white's point of view
fn evaluation_block(app: &ChessApp, ui: &mut egui::Ui) {
    let (value, color) = match app.evaluation {
        Some(score) => (format_evaluation(score), STAT_EVAL),
        // the game is over, so the status line above is the whole story
        None => ("-".to_string(), TEXT_MUTED),
    };

    stat_block(ui, "EVALUATION (WHITE)", &value, color);
}

// the share of the opening pieces still on the board, for tuning
fn phase_block(app: &ChessApp, ui: &mut egui::Ui) {
    stat_block(ui, "GAME PHASE", &format!("{:.2}", app.phase), TEXT_PRIMARY);
}

// what the last search found, and what it cost, each in its own colour
fn search_blocks(app: &ChessApp, ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new("SEARCH")
            .size(12.0)
            .strong()
            .color(ACCENT),
    );
    ui.add_space(14.0);

    let Some(stats) = &app.last_search else {
        ui.label(
            egui::RichText::new("No search run yet")
                .size(13.0)
                .color(TEXT_MUTED),
        );
        ui.add_space(18.0);
        return;
    };

    let seconds = stats.duration.as_secs_f64();
    let positions_per_second = if seconds > 0.0 {
        stats.positions_searched as f64 / seconds
    } else {
        0.0
    };

    let best_move = match &stats.best_move {
        Some(chess_move) => format_move(chess_move),
        None => "-".to_string(),
    };

    stat_block(
        ui,
        &format!("BEST MOVE AT DEPTH {}", stats.depth),
        &best_move,
        ACCENT,
    );
    stat_block(ui, "SCORE (WHITE)", &format_score(stats.score), STAT_EVAL);
    stat_block(
        ui,
        "POSITIONS SEARCHED",
        &format_count(stats.positions_searched),
        TEXT_PRIMARY,
    );
    stat_block(
        ui,
        "TIME TAKEN",
        &format_duration(stats.duration),
        STAT_TIME,
    );
    stat_block(
        ui,
        "POSITIONS / SEC",
        &format_count(positions_per_second as u64),
        STAT_SPEED,
    );
}

fn new_game_button(ui: &mut egui::Ui) -> bool {
    let button = egui::Button::new(egui::RichText::new("New Game").size(14.0).color(TEXT_PRIMARY))
        .fill(PANEL_BG)
        .stroke(egui::Stroke::new(1.0, PANEL_BORDER))
        .corner_radius(6.0)
        .min_size(egui::vec2(ui.available_width(), 34.0));

    ui.add(button).clicked()
}

// a small caps label with the number underneath it, the way a scoreboard reads
fn stat_block(ui: &mut egui::Ui, text: &str, value: &str, value_color: egui::Color32) {
    label(ui, text);
    ui.add_space(2.0);
    ui.label(
        egui::RichText::new(value)
            .font(egui::FontId::monospace(21.0))
            .color(value_color),
    );
    ui.add_space(18.0);
}

// the muted small caps heading every block starts with
fn label(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(11.0).color(TEXT_MUTED));
}

// a hairline rule spanning the panel's inner width
fn divider(ui: &mut egui::Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 1.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, PANEL_BORDER);
}

// formats a count with thousands separators, e.g. 197281 -> "197,281"
fn format_count(count: u64) -> String {
    let digits = count.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out.chars().rev().collect()
}

// a move as the two squares it goes between, e.g. "e2e4", with the promoted piece
// after them when there is one
fn format_move(chess_move: &Move) -> String {
    let mut text = format!(
        "{}{}",
        square_name(chess_move.from),
        square_name(chess_move.to)
    );

    if let Some(promotion) = chess_move.promotion {
        text.push(promotion.letter());
    }

    text
}

fn square_name(square: u8) -> String {
    let file = (b'a' + file_of(square)) as char;
    let rank = (b'1' + rank_of(square)) as char;
    format!("{file}{rank}")
}

// a search score: a mate reads as the number of moves until it, everything else as
// pawns like the static evaluation
fn format_score(score: i32) -> String {
    // every mate score sits within one search's worth of plies of MATE
    let plies_to_mate = MATE - score.abs();
    if plies_to_mate < 1000 {
        // a mate `n` plies away is delivered on move (n + 1) / 2
        let moves = (plies_to_mate + 1) / 2;
        let sign = if score < 0 { "-" } else { "" };
        // a mate that is already on the board has no moves left to count
        return if moves == 0 {
            format!("{sign}#")
        } else {
            format!("{sign}#{moves}")
        };
    }

    format_evaluation(score)
}

// centipawns as pawns, the way an engine reads them out: "+0.40", "-1.25", "0.00"
fn format_evaluation(score: i32) -> String {
    if score == 0 {
        return "0.00".to_string();
    }

    format!("{:+.2}", score as f64 / 100.0)
}

fn format_duration(duration: Duration) -> String {
    let milliseconds = duration.as_secs_f64() * 1000.0;
    if milliseconds < 1000.0 {
        format!("{milliseconds:.1} ms")
    } else {
        format!("{:.2} s", duration.as_secs_f64())
    }
}
