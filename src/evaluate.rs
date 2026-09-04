// How good a position is, in centipawns.
//
// The score is material plus a piece-square bonus: every piece is worth a base
// value, and standing on a good square is worth a little more. On top of that come
// the bishop pair and where the king wants to stand, both read off the game phase.
//
// The phase counts pieces, not centipawns: TOTAL_PHASE with everything standing,
// 0 once only kings and pawns are left. Weights that differ between the two ends are
// written W(middlegame, endgame). The board keeps the count; nothing here walks for it.
//
// The score is always for the side to move: positive means the side whose turn it
// is stands better, whichever side that is. That is what negamax wants - it only
// ever asks "how good is this for me", and negates the answer one ply up.
//
// Everything is counted from white's point of view first and only flipped at the
// very end, so the two sides can never drift apart.

use crate::board::Board;
use crate::board::board::insufficient_minors;
use crate::board::piece::{Color, PieceType};

type Psqt = [i16; 64];
type PsqtSet = [Psqt; 6];

// W(middlegame, endgame)
struct W(i16, i16);

impl W {
    fn at(self, phase: i32) -> i32 {
        interpolate(self.0, self.1, phase)
    }
}

const KING_BASE: i16 = 0;
const QUEEN_BASE: i16 = 900;
const ROOK_BASE: i16 = 500;
const KNIGHT_BASE: i16 = 300;
const BISHOP_BASE: i16 = 300;
const PAWN_BASE: i16 = 100;

pub const MATE: i32 = 100_000;


// the pair tells more as the board empties
const BISHOP_PAIR: W = W(10, 40);

const QUEEN: Psqt = [
    -20,-10,-10, -5, -5,-10,-10,-20,
    -10,  0,  0,  0,  0,  0,  0,-10,
    -10,  0,  5,  5,  5,  5,  0,-10,
    -5,  0,  5,  5,  5,  5,  0, -5,
    0,  0,  5,  5,  5,  5,  0, -5,
    -10,  5,  5,  5,  5,  5,  0,-10,
    -10,  0,  5,  0,  0,  0,  0,-10,
    -20,-10,-10, -5, -5,-10,-10,-20
];

const ROOK: Psqt = [
    0,  0,  0,  0,  0,  0,  0,  0,
    5, 10, 10, 10, 10, 10, 10,  5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    0,  0,  0,  5,  5,  0,  0,  0
];

const KNIGHT: Psqt = [
    -50,-40,-30,-30,-30,-30,-40,-50,
    -40,-20,  0,  0,  0,  0,-20,-40,
    -30,  0, 10, 15, 15, 10,  0,-30,
    -30,  5, 15, 20, 20, 15,  5,-30,
    -30,  0, 15, 20, 20, 15,  0,-30,
    -30,  5, 10, 15, 15, 10,  5,-30,
    -40,-20,  0,  5,  5,  0,-20,-40,
    -50,-40,-30,-30,-30,-30,-40,-50,
];

const BISHOP: Psqt = [
    -20,-10,-10,-10,-10,-10,-10,-20,
    -10,  0,  0,  0,  0,  0,  0,-10,
    -10,  0,  5, 10, 10,  5,  0,-10,
    -10,  5,  5, 10, 10,  5,  5,-10,
    -10,  0, 10, 10, 10, 10,  0,-10,
    -10, 10, 10, 10, 10, 10, 10,-10,
    -10,  5,  0,  0,  0,  0,  5,-10,
    -20,-10,-10,-10,-10,-10,-10,-20,
];

const PAWN: Psqt = [
    0,  0,  0,  0,  0,  0,  0,  0,
    50, 50, 50, 50, 50, 50, 50, 50,
    10, 10, 20, 30, 30, 20, 10, 10,
    5,  5, 10, 25, 25, 10,  5,  5,
    0,  0,  0, 20, 20,  0,  0,  0,
    5, -5,-10,  0,  0,-10, -5,  5,
    5, 10, 10,-20,-20, 10, 10,  5,
    0,  0,  0,  0,  0,  0,  0,  0
];

const KING_MID: Psqt = [
    -30,-40,-40,-50,-50,-40,-40,-30,
    -30,-40,-40,-50,-50,-40,-40,-30,
    -30,-40,-40,-50,-50,-40,-40,-30,
    -30,-40,-40,-50,-50,-40,-40,-30,
    -20,-30,-30,-40,-40,-30,-30,-20,
    -10,-20,-20,-20,-20,-20,-20,-10,
    20, 20,  0,  0,  0,  0, 20, 20,
    20, 30, 10,  0,  0, 10, 30, 20
];

// the king is the one piece whose good squares turn around by the endgame - corner
// early, centre late, interpolated between
const KING_END: Psqt = [
    -50,-40,-30,-20,-20,-30,-40,-50,
    -30,-20,-10,  0,  0,-10,-20,-30,
    -30,-10, 20, 30, 30, 20,-10,-30,
    -30,-10, 30, 40, 40, 30,-10,-30,
    -30,-10, 30, 40, 40, 30,-10,-30,
    -30,-10, 20, 30, 30, 20,-10,-30,
    -30,-30,  0,  0,  0,  0,-30,-30,
    -50,-30,-30,-30,-30,-30,-30,-50
];

const FLIP: [usize; 64] = [
    56, 57, 58, 59, 60, 61, 62, 63,
    48, 49, 50, 51, 52, 53, 54, 55,
    40, 41, 42, 43, 44, 45, 46, 47,
    32, 33, 34, 35, 36, 37, 38, 39,
    24, 25, 26, 27, 28, 29, 30, 31,
    16, 17, 18, 19, 20, 21, 22, 23,
    8,  9, 10, 11, 12, 13, 14, 15,
    0,  1,  2,  3,  4,  5,  6,  7,
];

const TABLES: PsqtSet = [KING_MID, PAWN, KNIGHT, BISHOP, ROOK, QUEEN];
const BASE_VALUES: [i16; 6] = [
    KING_BASE,
    PAWN_BASE,
    KNIGHT_BASE,
    BISHOP_BASE,
    ROOK_BASE,
    QUEEN_BASE,
];

// a full board, summed from the weights so retuning one cannot leave this behind
const TOTAL_PHASE: i32 = 2 * (2 * PieceType::Knight.phase_weight()
    + 2 * PieceType::Bishop.phase_weight()
    + 2 * PieceType::Rook.phase_weight()
    + PieceType::Queen.phase_weight());

// evaluate the current side vs the enemy side: positive is good for the side to move
pub fn evaluate(board: &Board) -> i32 {
    let mut score = 0;
    let mut can_mate = false;
    let mut bishops = [0; 2];
    let mut knights = [0; 2];

    for square in 0..64 {
        let Some(piece) = board.piece_at(square) else {
            continue;
        };
        let piece_type = piece.piece_type();

        match piece_type {
            PieceType::Pawn | PieceType::Rook | PieceType::Queen => can_mate = true,
            PieceType::Bishop => bishops[color_index(piece.color())] += 1,
            PieceType::Knight => knights[color_index(piece.color())] += 1,
            // scored in king_score instead, once the phase is in hand
            PieceType::King => continue,
        }

        let value = piece_score(piece_type, piece.color(), square) as i32;
        // white counts up, black counts down
        score += match piece.color() {
            Color::White => value,
            Color::Black => -value,
        };
    }

    // a game nobody can win any more is worth the same as one that already ended
    if !can_mate && insufficient_minors(bishops, knights) {
        return 0;
    }

    // clamped because a promotion can put more on than the game started with
    let phase = board.phase().clamp(0, TOTAL_PHASE);
    score += king_score(board, phase);
    score += bishop_pair_score(bishops, phase);

    // and now out of white's point of view and into the side to move's
    match board.turn() {
        Color::White => score,
        Color::Black => -score,
    }
}

pub fn piece_value(piece_type: PieceType) -> i16 {
    BASE_VALUES[piece_type as usize - 1]
}


pub fn game_phase_of(board: &Board) -> f32 {
    board.phase().clamp(0, TOTAL_PHASE) as f32 / TOTAL_PHASE as f32
}

// the two sides as the rows of the counting tables above
fn color_index(color: Color) -> usize {
    match color {
        Color::White => 0,
        Color::Black => 1,
    }
}

fn piece_score(piece_type: PieceType, color: Color, square: u8) -> i16 {
    let index = piece_type as usize - 1;
    BASE_VALUES[index] + TABLES[index][table_index(square, color)]
}

fn table_index(square: u8, color: Color) -> usize {
    match color {
        Color::White => FLIP[square as usize],
        Color::Black => square as usize,
    }
}

// integer on purpose: this is the innermost thing evaluate does
fn interpolate(middlegame: i16, endgame: i16, phase: i32) -> i32 {
    (middlegame as i32 * phase + endgame as i32 * (TOTAL_PHASE - phase)) / TOTAL_PHASE
}

fn king_score(board: &Board, phase: i32) -> i32 {
    let mut score = 0;

    for color in Color::BOTH {
        // None only on a hand built board
        let Some(square) = board.king_square(color) else {
            continue;
        };

        let index = table_index(square, color);
        let value = interpolate(KING_MID[index], KING_END[index], phase);

        score += match color {
            Color::White => value,
            Color::Black => -value,
        };
    }

    score
}

// two or more, so a promoted third bishop does not pay twice
fn bishop_pair_score(bishops: [usize; 2], phase: i32) -> i32 {
    let bonus = BISHOP_PAIR.at(phase);
    let mut score = 0;

    if bishops[color_index(Color::White)] >= 2 {
        score += bonus;
    }
    if bishops[color_index(Color::Black)] >= 2 {
        score -= bonus;
    }

    score
}
// TODO: evaluate own Pawn structure
// minus points for doubled pawns, and lone pawns,
// + points for a passed pawn
// - points for weak pawns
fn pawn_structure() -> i32 {
    return 0
}








#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::piece::Piece;

    fn start_position() -> Board {
        let mut board = Board::new();
        board.set_start_position();
        board
    }

    // the start position is the same for both sides, so nobody is ahead
    #[test]
    fn the_start_position_is_equal() {
        assert_eq!(evaluate(&start_position()), 0);
    }

    // the clock everything else is read off has to sit at the two ends of its range
    // where the range says it does, or every weight hanging off it is skewed
    #[test]
    fn the_phase_runs_from_a_full_board_down_to_bare_kings() {
        assert_eq!(game_phase_of(&start_position()), 1.0);

        let mut bare = Board::new();
        bare.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        bare.add_piece(Piece::new(PieceType::King, Color::Black), 60); // e8

        assert_eq!(game_phase_of(&bare), 0.0);
    }

    // the case the piece count is for - counting centipawns read this as 0.205
    #[test]
    fn a_pawn_endgame_is_all_the_way_into_the_endgame() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        board.add_piece(Piece::new(PieceType::King, Color::Black), 60); // e8

        for file in 0..8 {
            board.add_piece(Piece::new(PieceType::Pawn, Color::White), 8 + file);
            board.add_piece(Piece::new(PieceType::Pawn, Color::Black), 48 + file);
        }

        assert_eq!(game_phase_of(&board), 0.0);
    }

    #[test]
    fn the_queens_carry_a_third_of_the_phase() {
        let queens = 2 * PieceType::Queen.phase_weight();

        assert_eq!(queens * 3, TOTAL_PHASE);
        assert_eq!(PieceType::Pawn.phase_weight(), 0);
        assert_eq!(PieceType::King.phase_weight(), 0);
    }

    #[test]
    fn the_bishop_pair_grows_towards_the_endgame() {
        assert_eq!(bishop_pair_score([2, 0], TOTAL_PHASE), 10);
        assert_eq!(bishop_pair_score([2, 0], 0), 40);

        assert_eq!(bishop_pair_score([1, 0], 0), 0, "one bishop is not a pair");
        assert_eq!(bishop_pair_score([2, 2], 0), 0, "both sides have it");
    }

    // same board, both ends of the phase - the answer has to change sign
    #[test]
    fn the_king_turns_around_towards_the_endgame() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 28); // e4
        board.add_piece(Piece::new(PieceType::King, Color::Black), 62); // g8

        assert!(king_score(&board, TOTAL_PHASE) < 0, "centre is bad early");
        assert!(king_score(&board, 0) > 0, "centre is good late");
    }

    #[test]
    fn the_score_is_for_the_side_to_move() {
        let mut board = start_position();
        board.make_move_from_squares(12, 28, None); // e2e4

        assert_eq!(board.turn(), Color::Black);

        let score = evaluate(&board);
        assert!(score < 0, "black to move scored {score}");
    }

    // once black has answered with the mirror image of the move, the position is
    // symmetric again and back to equal - which only comes out if the flip at the
    // end follows the side to move
    #[test]
    fn a_symmetric_position_is_equal_again() {
        let mut board = start_position();
        board.make_move_from_squares(12, 28, None); // e2e4
        board.make_move_from_squares(52, 36, None); // e7e5

        assert_eq!(evaluate(&board), 0);
    }

    // a side up a queen for nothing is up about a queen
    #[test]
    fn material_counts() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 4);
        board.add_piece(Piece::new(PieceType::King, Color::Black), 60);
        board.add_piece(Piece::new(PieceType::Queen, Color::White), 3);

        // white is the side to move on a fresh board, so the queen counts up
        let score = evaluate(&board);
        assert!(
            (QUEEN_BASE as i32 - 50..=QUEEN_BASE as i32 + 50).contains(&score),
            "a lone queen scored {score}"
        );
    }

    // the same position mirrored has to score the same for the mirrored side, which
    // is what tells a table lookup that is off by a rank from a correct one
    #[test]
    fn mirrored_positions_score_the_same() {
        let mut white_side = Board::new();
        white_side.add_piece(Piece::new(PieceType::King, Color::White), 4);
        white_side.add_piece(Piece::new(PieceType::King, Color::Black), 60);
        white_side.add_piece(Piece::new(PieceType::Knight, Color::White), 18); // c3
        white_side.add_piece(Piece::new(PieceType::Pawn, Color::White), 28); // e4

        let mut black_side = Board::new();
        black_side.add_piece(Piece::new(PieceType::King, Color::White), 4);
        black_side.add_piece(Piece::new(PieceType::King, Color::Black), 60);
        black_side.add_piece(Piece::new(PieceType::Knight, Color::Black), 42); // c6
        black_side.add_piece(Piece::new(PieceType::Pawn, Color::Black), 36); // e5

        // both boards have white to move, so the mirrored one scores the opposite
        assert_eq!(evaluate(&white_side), -evaluate(&black_side));
    }

    // two knights cannot force a mate against a bare king, so being two knights up
    // is being nothing up - the one case the counting has to get exactly right
    #[test]
    fn two_knights_against_a_bare_king_is_a_draw() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        board.add_piece(Piece::new(PieceType::King, Color::Black), 60); // e8
        board.add_piece(Piece::new(PieceType::Knight, Color::White), 1); // b1
        board.add_piece(Piece::new(PieceType::Knight, Color::White), 6); // g1

        assert_eq!(evaluate(&board), 0);
    }

    // but the same two knights against a knight is an ordinary position: black has a
    // piece to lose, so white being a knight up counts
    #[test]
    fn a_knight_up_against_a_knight_still_counts() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        board.add_piece(Piece::new(PieceType::King, Color::Black), 60); // e8
        board.add_piece(Piece::new(PieceType::Knight, Color::White), 1); // b1
        board.add_piece(Piece::new(PieceType::Knight, Color::White), 6); // g1
        board.add_piece(Piece::new(PieceType::Knight, Color::Black), 57); // b8

        let score = evaluate(&board);
        assert!(score > 200, "a knight up scored {score}");
    }

    // two bare kings: nobody can mate anybody, so there is nothing to be ahead by
    #[test]
    fn a_draw_by_material_is_worth_nothing() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        board.add_piece(Piece::new(PieceType::King, Color::Black), 60); // e8
        board.add_piece(Piece::new(PieceType::Bishop, Color::White), 2); // c1

        assert_eq!(evaluate(&board), 0);
    }
}
