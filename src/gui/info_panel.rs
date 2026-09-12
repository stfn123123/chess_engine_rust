// The right-hand panel: whose move it is, how the game stands, and what the last
// search at the current position cost.
//
// The header and the score/search stats sit in a left column; the testing controls
// (toggles, perft, saved positions) sit in a right column beside them. The whole
// panel scrolls as one block, so a short window hides nothing, it only asks to scroll.

use eframe::egui;
use std::time::Duration;

use super::theme::{
    ACCENT, BLACK_SIDE, CALM, DANGER, PANEL_BG, PANEL_BORDER, STAT_EVAL, STAT_SPEED, STAT_TIME,
    TEXT_MUTED, TEXT_PRIMARY, WHITE_SIDE,
};
use super::{ChessApp, SearchStats};
use crate::board::piece::{Color, PieceType};
use crate::clock::{TimeControl, format_remaining};
use crate::evaluate::MATE;
use crate::stockfish;

// the Start/Pause button beside the two clocks
const BUTTON_WIDTH: f32 = 96.0;

pub fn show(app: &mut ChessApp, ui: &mut egui::Ui, width: f32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    ui.painter().rect_filled(rect, 8.0, PANEL_BG);
    ui.painter().rect_stroke(
        rect,
        8.0,
        egui::Stroke::new(1.0, PANEL_BORDER),
        egui::StrokeKind::Inside,
    );

    // layout must be spelled out, or it inherits the parent's horizontal row
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
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("CHESS")
                            .size(20.0)
                            .strong()
                            .color(TEXT_PRIMARY),
                    );

                    let button_size = egui::vec2(150.0, 28.0);
                    let gap = ui.spacing().item_spacing.x;
                    let used = button_size.x * 2.0 + gap;
                    ui.add_space((ui.available_width() - used).max(0.0));

                    autoplay_buttons(app, ui, button_size);
                });
                ui.add_space(12.0);
                divider(ui);
                ui.add_space(16.0);

                replay_controls_block(app, ui);

                clock_block(app, ui);

                ui.columns(2, |columns| {
                    let left = &mut columns[0];
                    turn_block(app, left);
                    evaluation_block(app, left);
                    phase_block(app, left);
                    library_position_block(app, left);

                    divider(left);
                    left.add_space(16.0);

                    search_blocks(app, left);

                    testing_blocks(app, &mut columns[1]);
                    bitboard_blocks(app, &mut columns[1]);
                });

                ui.add_space(4.0);
                if new_game_button(ui) {
                    app.reset();
                }
            });
        });
}

// the two clocks, side by side, with the side on the clock lit up. Only a clocked time
// control has anything to show here - a depth or a fixed time per move has no clock
fn clock_block(app: &mut ChessApp, ui: &mut egui::Ui) {
    if !app.time_control.is_clocked() {
        return;
    }

    let to_move = app.board.turn();
    let running = app.clock.is_running() && !app.game_over();
    let flagged = app.clock.flagged();

    let width = ui.available_width();
    let gap = ui.spacing().item_spacing.x;
    let clock_width = (width - gap * 2.0 - BUTTON_WIDTH) / 2.0;

    ui.horizontal(|ui| {
        for side in Color::BOTH {
            let remaining = app.clock.remaining_now(side);
            // the side on the clock is the one burning it, so it is the one to read
            let color = if flagged == Some(side) {
                DANGER
            } else if running && side == to_move {
                CALM
            } else {
                TEXT_MUTED
            };

            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(clock_width, 52.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 6.0, PANEL_BG);
            ui.painter().rect_stroke(
                rect,
                6.0,
                egui::Stroke::new(1.0, PANEL_BORDER),
                egui::StrokeKind::Inside,
            );

            let name = match side {
                Color::White => "WHITE",
                Color::Black => "BLACK",
            };
            ui.painter().text(
                rect.left_top() + egui::vec2(10.0, 8.0),
                egui::Align2::LEFT_TOP,
                name,
                egui::FontId::proportional(11.0),
                TEXT_MUTED,
            );
            ui.painter().text(
                rect.left_bottom() + egui::vec2(10.0, -8.0),
                egui::Align2::LEFT_BOTTOM,
                format_remaining(remaining),
                egui::FontId::monospace(24.0),
                color,
            );
        }

        let (text, text_color) = match (app.game_over(), running) {
            (true, _) => ("Game over", TEXT_MUTED),
            (false, true) => ("Pause", CALM),
            (false, false) => ("Start", ACCENT),
        };
        if panel_button(ui, text, text_color, egui::vec2(BUTTON_WIDTH, 52.0)) {
            app.toggle_clock();
        }
    });

    ui.add_space(16.0);
}

// whose move it is, as a disc in that side's colour next to its name - once the
// game has ended this shows the result instead, under the same heading
fn turn_block(app: &ChessApp, ui: &mut egui::Ui) {
    let (name, disc) = if let Some(flagged) = app.clock.flagged() {
        // a flag decides the game before the position does, whatever stands on the board
        match flagged {
            Color::White => ("Black won on time", BLACK_SIDE),
            Color::Black => ("White won on time", WHITE_SIDE),
        }
    } else if app.board.is_checkmate() {
        match app.board.winner() {
            Color::White => ("White won", WHITE_SIDE),
            Color::Black => ("Black won", BLACK_SIDE),
        }
    } else if app.game_over() {
        ("Draw", TEXT_MUTED)
    } else {
        match app.board.turn() {
            Color::White => ("White", WHITE_SIDE),
            Color::Black => ("Black", BLACK_SIDE),
        }
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

// how the position stands after the last move, in pawns from white's point of view
fn evaluation_block(app: &ChessApp, ui: &mut egui::Ui) {
    let (value, color) = match app.evaluation {
        Some(score) => (format_evaluation(score), STAT_EVAL),
        // nothing was worked out for this position, because the engine is turned off
        None if !app.analysis_enabled => ("off".to_string(), TEXT_MUTED),
        // the game is over, so the to-move block above is the whole story
        None => ("-".to_string(), TEXT_MUTED),
    };

    stat_block(ui, "EVALUATION (WHITE)", &value, color);
}

// the share of the opening pieces still on the board, for tuning
fn phase_block(app: &ChessApp, ui: &mut egui::Ui) {
    stat_block(ui, "GAME PHASE", &format!("{:.2}", app.phase), TEXT_PRIMARY);
}

// what the library recorded for this position, once one has been loaded from the
// "Stored" window - the moves as a plain list, since the library keeps only one
// evaluation per position rather than one per move
fn library_position_block(app: &ChessApp, ui: &mut egui::Ui) {
    let Some(loaded) = &app.loaded_library_position else {
        return;
    };

    label(ui, "LIBRARY MOVES");
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(loaded.moves.join(", "))
            .size(14.0)
            .color(TEXT_PRIMARY),
    );
    ui.add_space(18.0);

    stat_block(ui, "LIBRARY EVAL", &format_stockfish_score(&loaded.eval), STAT_EVAL);
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
        let hint = if app.analysis_enabled {
            "No search run yet"
        } else {
            "Evaluation is off - Run Search does one"
        };

        ui.label(egui::RichText::new(hint).size(13.0).color(TEXT_MUTED));
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
        Some(chess_move) => chess_move.coordinates(),
        None => "-".to_string(),
    };

    // a book move was not searched for, so it has no depth and no score to show, and
    // the counts below it are all zero because nothing was done to run them up
    let heading = if stats.from_book {
        "BOOK MOVE"
    } else {
        "BEST MOVE"
    };
    let score = if stats.from_book {
        "-".to_string()
    } else {
        format_score(stats.score)
    };
    // the deepest pass that finished. Under a time limit this is a result rather than a
    // setting - how far the engine got for what it was given - so it gets its own line
    let depth = if stats.from_book {
        "-".to_string()
    } else {
        stats.depth.to_string()
    };

    stat_block(ui, heading, &best_move, ACCENT);
    stat_block(ui, "SCORE (WHITE)", &score, STAT_EVAL);
    stat_block(ui, "DEPTH REACHED", &depth, ACCENT);
    stat_block(
        ui,
        "POSITIONS SEARCHED",
        &format_count(stats.positions_searched),
        TEXT_PRIMARY,
    );
    stat_block(
        ui,
        "QUIESCENCE POSITIONS",
        &format_count(stats.positions_searched_quiescience),
        TEXT_PRIMARY,
    );
    stat_block(
        ui,
        "TIME TAKEN",
        &format_duration(stats.duration),
        STAT_TIME,
    );
    // what the last move the engine played was given, to read the time taken against -
    // a search that lands well short of its budget is one that ran out of depth first
    if let Some(budget) = app.last_budget {
        stat_block(ui, "TIME BUDGET", &format_duration(budget), STAT_TIME);
    }
    stat_block(
        ui,
        "POSITIONS / SEC",
        &format_count(positions_per_second as u64),
        STAT_SPEED,
    );
    // what the transposition table saved, and how full it is: once the fill nears
    // 100% the table is throwing away as much as it keeps
    stat_block(
        ui,
        "TABLE CUTOFFS",
        &format_count(stats.table_cutoffs),
        STAT_EVAL,
    );
    stat_block(
        ui,
        "TABLE FILL",
        &format!("{:.1}%", stats.table_fill * 100.0),
        TEXT_MUTED,
    );

    // what share of the cutoffs came from a quiet move the ply had already seen cut:
    // the killers are only earning their keep while this stays well clear of zero
    let killer_share = if stats.beta_cutoffs > 0 {
        stats.killer_cutoffs as f64 / stats.beta_cutoffs as f64 * 100.0
    } else {
        0.0
    };
    stat_block(
        ui,
        "KILLER CUTOFFS",
        &format!(
            "{} ({killer_share:.1}%)",
            format_count(stats.killer_cutoffs)
        ),
        STAT_EVAL,
    );

    // how often the move ordering had the refutation in hand right away: above ~90% the
    // ordering is close to all it can be, and a narrower window has little left to save
    let first_move_share = if stats.beta_cutoffs > 0 {
        stats.first_move_cutoffs as f64 / stats.beta_cutoffs as f64 * 100.0
    } else {
        0.0
    };
    stat_block(
        ui,
        "FIRST MOVE CUTOFFS",
        &format!(
            "{} ({first_move_share:.1}%)",
            format_count(stats.first_move_cutoffs)
        ),
        STAT_EVAL,
    );

    // how often a reduced move turned out to be worth a full search after all: the reductions
    // are paying while this stays low, and every one of them cost a second search
    let research_share = if stats.lmr_reductions > 0 {
        stats.lmr_researches as f64 / stats.lmr_reductions as f64 * 100.0
    } else {
        0.0
    };
    stat_block(
        ui,
        "LMR REDUCTIONS",
        &format!(
            "{} ({research_share:.1}% redone)",
            format_count(stats.lmr_reductions)
        ),
        STAT_EVAL,
    );

    deepening_block(app, ui, stats);
}

// one row per pass of the deepening: what it played, what it scored it, and what the
// search had cost by the time it got there. The move should settle on one and stay
// there, and the node counts show whether the passes are paying for themselves
fn deepening_block(app: &ChessApp, ui: &mut egui::Ui, stats: &SearchStats) {
    if stats.passes.is_empty() {
        return;
    }

    label(ui, "DEEPENING");
    ui.add_space(4.0);

    // the pass before, so the node count can be read as this pass on its own as well
    let mut before = 0;

    for pass in &stats.passes {
        let best_move = match &pass.best_move {
            Some(chess_move) => chess_move.coordinates(),
            None => "-".to_string(),
        };

        let row = format!(
            "d{:<2} {:<6} {:>8} {:>10} {:>10}",
            pass.depth,
            best_move,
            format_score(app.white_view(pass.score)),
            format_count(pass.positions_searched - before),
            format_duration(pass.elapsed),
        );
        before = pass.positions_searched;

        ui.label(
            egui::RichText::new(row)
                .font(egui::FontId::monospace(12.0))
                .color(TEXT_PRIMARY),
        );
    }

    ui.add_space(18.0);
}

// turning the engine off and on, running it by hand, and the positions put aside to
// come back to - everything that is about testing rather than about playing
fn testing_blocks(app: &mut ChessApp, ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new("TESTING")
            .size(12.0)
            .strong()
            .color(ACCENT),
    );
    ui.add_space(14.0);

    let full_width = ui.available_width();

    // the state it is in now, not the state a click would put it in - a button reading
    // "Off" while the engine is running would be read as a label, not as a switch
    let (toggle, toggle_color) = match app.analysis_enabled {
        true => ("Evaluation: On", CALM),
        false => ("Evaluation: Off", TEXT_MUTED),
    };

    if panel_button(ui, toggle, toggle_color, egui::vec2(full_width, 34.0)) {
        app.set_analysis(!app.analysis_enabled);
    }
    ui.add_space(6.0);

    // one run whatever the toggle says: with the engine off this is the only way to
    // search, and with it on it searches the same position again, for a second timing.
    // the depth field feeds every search, automatic or manual, so it can be tuned
    // without a recompile
    let mut run_search = false;
    ui.horizontal(|ui| {
        let field = 56.0;
        let gap = ui.spacing().item_spacing.x;
        let rest = (full_width - field - gap).max(0.0);

        ui.add_sized(
            [field, 34.0],
            egui::DragValue::new(&mut app.search_depth).range(1..=20),
        );
        run_search = panel_button(ui, "Run Search", ACCENT, egui::vec2(rest, 34.0));
    });
    if run_search {
        app.analyse_once();
    }
    ui.add_space(6.0);

    time_control_block(app, ui, full_width);

    // hands the position to the bundled Stockfish and shows its top answers below,
    // for checking this engine's move choices against a much stronger one
    if panel_button(ui, "Ask Stockfish", ACCENT, egui::vec2(full_width, 34.0)) {
        app.ask_stockfish();
    }
    stockfish_result_block(app, ui);
    ui.add_space(6.0);

    if panel_button(
        ui,
        "Store Position",
        TEXT_PRIMARY,
        egui::vec2(full_width, 34.0),
    ) {
        app.store_position();
    }
    ui.add_space(6.0);

    if panel_button(ui, "Save Game", TEXT_PRIMARY, egui::vec2(full_width, 34.0)) {
        app.save_game();
    }
    ui.add_space(6.0);

    let stored_color = if app.show_stored_window { CALM } else { TEXT_PRIMARY };
    if panel_button(ui, "Stored", stored_color, egui::vec2(full_width, 34.0)) {
        app.show_stored_window = !app.show_stored_window;
    }
    ui.add_space(6.0);

    perft_block(app, ui, full_width);

    stored_window(app, ui.ctx());
}

// the stored positions, saved games and the stockfish library, behind the "Stored"
// button rather than always taking up panel space for lists that are usually empty
fn stored_window(app: &mut ChessApp, ctx: &egui::Context) {
    if !app.show_stored_window {
        return;
    }

    let mut open = true;
    egui::Window::new("Stored")
        .open(&mut open)
        .resizable(true)
        .default_width(320.0)
        .default_height(480.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    saved_positions_block(app, ui);
                    ui.add_space(16.0);
                    saved_games_block(app, ui);
                    ui.add_space(16.0);
                    library_positions_block(app, ui);
                });
        });

    if !open {
        app.show_stored_window = false;
    }
}

// the pre-computed stockfish library: click a row to load that position, and its
// moves and evaluation show in the left column without asking stockfish again
fn library_positions_block(app: &mut ChessApp, ui: &mut egui::Ui) {
    label(ui, "STOCKFISH LIBRARY");
    ui.add_space(6.0);

    if app.library_positions.is_empty() {
        ui.label(
            egui::RichText::new("No positions generated yet")
                .size(13.0)
                .color(TEXT_MUTED),
        );
        ui.add_space(18.0);
        return;
    }

    let mut load = None;

    for (index, position) in app.library_positions.iter().enumerate() {
        let row = format!(
            "#{} {} ({})",
            index + 1,
            position.moves.first().map(String::as_str).unwrap_or("-"),
            format_stockfish_score(&position.eval)
        );

        if panel_button(ui, &row, TEXT_PRIMARY, egui::vec2(ui.available_width(), 28.0)) {
            load = Some(index);
        }
        ui.add_space(4.0);
    }

    ui.add_space(14.0);

    if let Some(index) = load {
        app.load_library_position(index);
    }
}

// the game currently being stepped through, if any: which move it is on, and the
// buttons to move through it or leave it - drawn full width, above the two columns,
// since it is a mode the whole panel is in rather than one more testing control
fn replay_controls_block(app: &mut ChessApp, ui: &mut egui::Ui) {
    let Some((ply, total)) = app
        .replay
        .as_ref()
        .map(|replay| (replay.ply, replay.moves.len()))
    else {
        return;
    };

    label(ui, "REPLAYING SAVED GAME");
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(format!("Move {ply} / {total}"))
            .size(15.0)
            .strong()
            .color(TEXT_PRIMARY),
    );
    ui.add_space(8.0);

    let full_width = ui.available_width();
    ui.horizontal(|ui| {
        let gap = ui.spacing().item_spacing.x;
        let half = (full_width - gap) / 2.0;

        if panel_button(ui, "< Previous", TEXT_PRIMARY, egui::vec2(half, 34.0)) {
            app.replay_step(-1);
        }
        if panel_button(ui, "Next >", TEXT_PRIMARY, egui::vec2(half, 34.0)) {
            app.replay_step(1);
        }
    });
    ui.add_space(6.0);

    if panel_button(ui, "Exit Replay", DANGER, egui::vec2(full_width, 30.0)) {
        app.exit_replay();
    }
    ui.add_space(16.0);
    divider(ui);
    ui.add_space(16.0);
}

// the saved games, one row each: the label starts a replay, the cross forgets it
fn saved_games_block(app: &mut ChessApp, ui: &mut egui::Ui) {
    label(ui, "SAVED GAMES");
    ui.add_space(6.0);

    if app.saved_games.is_empty() {
        ui.label(
            egui::RichText::new("No games saved yet")
                .size(13.0)
                .color(TEXT_MUTED),
        );
        ui.add_space(18.0);
        return;
    }

    // which row was clicked, decided while the list is only being read - starting a
    // replay or forgetting a game on the spot would be changing the list being walked
    let mut replay = None;
    let mut forget = None;

    for (index, saved) in app.saved_games.iter().enumerate() {
        ui.horizontal(|ui| {
            let cross = 28.0;
            let gap = ui.spacing().item_spacing.x;
            let rest = (ui.available_width() - cross - gap).max(0.0);

            if panel_button(ui, &saved.label, TEXT_PRIMARY, egui::vec2(rest, 28.0)) {
                replay = Some(index);
            }
            if panel_button(ui, "\u{00d7}", DANGER, egui::vec2(cross, 28.0)) {
                forget = Some(index);
            }
        });
        ui.add_space(4.0);
    }

    ui.add_space(14.0);

    if let Some(index) = replay {
        app.start_replay(index);
    }
    if let Some(index) = forget {
        app.forget_game(index);
    }
}

// lets each side's moves be handed to the engine instead of played by hand - the
// state it is in now, not the state a click would put it in, same as the evaluation
// toggle. Sits in the header next to the title, so it reads the same regardless of
// which layout direction `ui` is in when it's called
fn autoplay_buttons(app: &mut ChessApp, ui: &mut egui::Ui, size: egui::Vec2) {
    let (label, color) = match app.autoplay_white {
        true => ("Autoplay White: On", CALM),
        false => ("Autoplay White: Off", TEXT_MUTED),
    };
    if panel_button(ui, label, color, size) {
        app.autoplay_white = !app.autoplay_white;
    }
    ui.add_space(6.0);

    let (label, color) = match app.autoplay_black {
        true => ("Autoplay Black: On", CALM),
        false => ("Autoplay Black: Off", TEXT_MUTED),
    };
    if panel_button(ui, label, color, size) {
        app.autoplay_black = !app.autoplay_black;
    }
}

// what bounds a move the engine plays: the depth field above, a flat time per move, or
// a clock for the whole game. Changing any of it starts the clocks over, so a control is
// picked before a game rather than in the middle of one
fn time_control_block(app: &mut ChessApp, ui: &mut egui::Ui, full_width: f32) {
    label(ui, "TIME CONTROL");
    ui.add_space(4.0);

    let gap = ui.spacing().item_spacing.x;
    let third = (full_width - gap * 2.0) / 3.0;

    // what a click on each mode would switch to, built from the fields as they stand
    let modes = [
        ("Depth", TimeControl::Depth),
        (
            "Per move",
            TimeControl::MoveTime(Duration::from_secs(app.move_seconds.max(1))),
        ),
        (
            "Clock",
            TimeControl::clock(app.clock_minutes.max(1), app.clock_increment),
        ),
    ];

    let mut picked = None;
    ui.horizontal(|ui| {
        for (name, control) in modes {
            // the mode it is in now, the same way the toggles above read
            let color = if same_mode(app.time_control, control) {
                CALM
            } else {
                TEXT_MUTED
            };
            if panel_button(ui, name, color, egui::vec2(third, 30.0)) {
                picked = Some(control);
            }
        }
    });

    // the numbers behind the mode in force; the other modes' fields stay as they were
    match app.time_control {
        TimeControl::Depth => {}
        TimeControl::MoveTime(_) => {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_sized(
                    [third, 30.0],
                    egui::DragValue::new(&mut app.move_seconds)
                        .range(1..=600)
                        .suffix(" s"),
                );
                ui.label(
                    egui::RichText::new("per move")
                        .size(13.0)
                        .color(TEXT_MUTED),
                );
            });
            picked = picked.or(Some(TimeControl::MoveTime(Duration::from_secs(
                app.move_seconds.max(1),
            ))));
        }
        TimeControl::Clock { .. } => {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_sized(
                    [third, 30.0],
                    egui::DragValue::new(&mut app.clock_minutes)
                        .range(1..=180)
                        .suffix(" min"),
                );
                ui.add_sized(
                    [third, 30.0],
                    egui::DragValue::new(&mut app.clock_increment)
                        .range(0..=60)
                        .suffix(" s"),
                );
            });

            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                for (name, minutes, increment) in TimeControl::PRESETS {
                    let preset = TimeControl::clock(minutes, increment);
                    let color = if app.time_control == preset {
                        CALM
                    } else {
                        TEXT_MUTED
                    };
                    if panel_button(ui, name, color, egui::vec2(third * 0.6, 26.0)) {
                        app.clock_minutes = minutes;
                        app.clock_increment = increment;
                        picked = Some(preset);
                    }
                }
            });

            picked = picked.or(Some(TimeControl::clock(
                app.clock_minutes.max(1),
                app.clock_increment,
            )));
        }
    }

    // set_time_control does nothing when the control has not actually changed, so the
    // dragged fields above can hand it a control every frame
    if let Some(control) = picked {
        app.set_time_control(control);
    }

    ui.add_space(6.0);
}

// whether two controls are the same kind, regardless of the numbers in them - which is
// what the mode buttons highlight
fn same_mode(left: TimeControl, right: TimeControl) -> bool {
    matches!(
        (left, right),
        (TimeControl::Depth, TimeControl::Depth)
            | (TimeControl::MoveTime(_), TimeControl::MoveTime(_))
            | (TimeControl::Clock { .. }, TimeControl::Clock { .. })
    )
}

// perft: counts positions at a given depth, checked against published numbers
fn perft_block(app: &mut ChessApp, ui: &mut egui::Ui, full_width: f32) {
    let mut run = false;

    ui.horizontal(|ui| {
        let field = 56.0;
        let gap = ui.spacing().item_spacing.x;
        let rest = (full_width - field - gap).max(0.0);

        // dragged or typed into, and capped where a count still finishes in a moment
        ui.add_sized(
            [field, 34.0],
            egui::DragValue::new(&mut app.perft_depth).range(1..=8),
        );
        run = panel_button(ui, "Count Positions", ACCENT, egui::vec2(rest, 34.0));
    });

    // read out of the app before the click is answered, so what is drawn is the count
    // that was on screen when the button was pressed
    let last = app
        .last_perft
        .as_ref()
        .map(|perft| (perft.depth, perft.positions, perft.duration));

    if let Some((depth, positions, duration)) = last {
        ui.add_space(14.0);
        stat_block(
            ui,
            &format!("POSITIONS AT DEPTH {depth}"),
            &format_count(positions),
            TEXT_PRIMARY,
        );
        stat_block(ui, "COUNT TIME", &format_duration(duration), STAT_TIME);
    }

    if run {
        app.count_positions();
    }
}

// stockfish's top answers to the last "Ask Stockfish" click, ranked best first - or
// why there is nothing to show, if the binary would not talk UCI
fn stockfish_result_block(app: &ChessApp, ui: &mut egui::Ui) {
    match &app.stockfish_result {
        None => {}
        Some(Err(error)) => {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(error).size(12.0).color(DANGER));
        }
        Some(Ok(lines)) => {
            ui.add_space(6.0);
            for line in lines {
                ui.label(
                    egui::RichText::new(format!(
                        "{}. {} ({})",
                        line.rank,
                        line.best_move,
                        format_stockfish_score(&line.score)
                    ))
                    .size(14.0)
                    .color(TEXT_PRIMARY),
                );
                ui.add_space(2.0);
            }
        }
    }
}

fn format_stockfish_score(score: &stockfish::StockfishScore) -> String {
    match score {
        stockfish::StockfishScore::Centipawns(cp) => format!("{:+.2}", *cp as f32 / 100.0),
        stockfish::StockfishScore::MateIn(moves) => format!("mate in {}", moves.abs()),
    }
}

// the stored positions, one row each: the label recalls it, the cross forgets it
fn saved_positions_block(app: &mut ChessApp, ui: &mut egui::Ui) {
    label(ui, "STORED POSITIONS");
    ui.add_space(6.0);

    if app.saved_positions.is_empty() {
        ui.label(
            egui::RichText::new("Nothing stored yet")
                .size(13.0)
                .color(TEXT_MUTED),
        );
        ui.add_space(18.0);
        return;
    }

    // which row was clicked, decided while the list is only being read - recalling or
    // forgetting one on the spot would be changing the list that is being walked
    let mut recall = None;
    let mut forget = None;

    for (index, saved) in app.saved_positions.iter().enumerate() {
        ui.horizontal(|ui| {
            let cross = 28.0;
            let gap = ui.spacing().item_spacing.x;
            let rest = (ui.available_width() - cross - gap).max(0.0);

            if panel_button(ui, &saved.label, TEXT_PRIMARY, egui::vec2(rest, 28.0)) {
                recall = Some(index);
            }
            if panel_button(ui, "\u{00d7}", DANGER, egui::vec2(cross, 28.0)) {
                forget = Some(index);
            }
        });
        ui.add_space(4.0);
    }

    ui.add_space(14.0);

    if let Some(index) = recall {
        app.recall_position(index);
    }
    if let Some(index) = forget {
        app.forget_position(index);
    }
}

fn new_game_button(ui: &mut egui::Ui) -> bool {
    panel_button(
        ui,
        "New Game",
        TEXT_PRIMARY,
        egui::vec2(ui.available_width(), 34.0),
    )
}

// the panel's button, in the one shape they all share - the text colour is the only
// thing that varies, so a toggle can read on or off at a glance
fn panel_button(
    ui: &mut egui::Ui,
    text: &str,
    text_color: egui::Color32,
    size: egui::Vec2,
) -> bool {
    let button = egui::Button::new(egui::RichText::new(text).size(14.0).color(text_color))
        .fill(PANEL_BG)
        .stroke(egui::Stroke::new(1.0, PANEL_BORDER))
        .corner_radius(6.0)
        .min_size(size);

    ui.add(button).clicked()
}

// one switch per bitboard, so a board can be put on the position and watched through
// the move types that are worth seeing rather than trusting - tucked behind a toggle
// button, since most of the time none of this is worth looking at
fn bitboard_blocks(app: &mut ChessApp, ui: &mut egui::Ui) {
    ui.add_space(10.0);
    divider(ui);
    ui.add_space(14.0);

    let full_width = ui.available_width();
    let text_color = if app.bitboards_expanded { CALM } else { TEXT_MUTED };
    if panel_button(ui, "Bitboards", text_color, egui::vec2(full_width, 30.0)) {
        app.bitboards_expanded = !app.bitboards_expanded;
    }

    if !app.bitboards_expanded {
        return;
    }

    ui.add_space(10.0);

    ui.columns(2, |columns| {
        for (column, color) in columns.iter_mut().zip(Color::BOTH) {
            label(
                column,
                match color {
                    Color::White => "WHITE",
                    Color::Black => "BLACK",
                },
            );
            column.add_space(4.0);

            let width = column.available_width();
            for piece_type in PieceType::ALL {
                let shown = &mut app.shown_bitboards[color.index()][piece_type.board_index()];
                // lit when the board is on the position, the same way the toggles above read
                let text_color = if *shown { CALM } else { TEXT_MUTED };

                if panel_button(column, piece_name(piece_type), text_color, egui::vec2(width, 26.0))
                {
                    *shown = !*shown;
                }
                column.add_space(4.0);
            }
        }
    });

    ui.add_space(4.0);
    let full_width = ui.available_width();
    if panel_button(ui, "Clear All", TEXT_MUTED, egui::vec2(full_width, 30.0)) {
        app.shown_bitboards = [[false; 6]; 2];
    }

    // the attack tables answering for one piece, which is how they are checked by eye:
    // click a piece and the squares it covers get a ring, blockers and all
    ui.add_space(8.0);
    let text_color = if app.show_attacks { CALM } else { TEXT_MUTED };
    if panel_button(
        ui,
        "Attacks of Selected",
        text_color,
        egui::vec2(full_width, 30.0),
    ) {
        app.show_attacks = !app.show_attacks;
    }
}

fn piece_name(piece_type: PieceType) -> &'static str {
    match piece_type {
        PieceType::King => "King",
        PieceType::Pawn => "Pawn",
        PieceType::Knight => "Knight",
        PieceType::Bishop => "Bishop",
        PieceType::Rook => "Rook",
        PieceType::Queen => "Queen",
    }
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
