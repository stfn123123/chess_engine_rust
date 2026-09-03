// How good a position is, in centipawns.
//
// The score is material plus a piece-square bonus: every piece is worth a base
// value, and standing on a good square is worth a little more. That is the whole
// evaluation for now - the bishop pair and the interpolation between a middlegame
// and an endgame score are prepared below but not used yet.
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
struct W(i16, i16);

const KING_BASE: i16 = 0;
const QUEEN_BASE: i16 = 900;
const ROOK_BASE: i16 = 500;
const KNIGHT_BASE: i16 = 300;
const BISHOP_BASE: i16 = 300;
const PAWN_BASE: i16 = 100;

// being checkmated, i.e. the worst a position can be for the side to move
// it has to sit far above what any material can be worth, so that no amount of
// pieces ever looks as good as a mate does
pub const MATE: i32 = 100_000;


#[allow(dead_code)]
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

// the king is the only piece whose good squares change completely towards the end
// of the game, so this one waits for the interpolation
#[allow(dead_code)]
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

        match piece.piece_type() {
            // a pawn promotes, a rook and a queen mate on their own: any one of them
            // settles the question
            PieceType::Pawn | PieceType::Rook | PieceType::Queen => can_mate = true,
            PieceType::Bishop => bishops[color_index(piece.color())] += 1,
            PieceType::Knight => knights[color_index(piece.color())] += 1,
            PieceType::King => {}
        }

        let value = piece_score(piece.piece_type(), piece.color(), square) as i32;
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

    // and now out of white's point of view and into the side to move's
    match board.turn() {
        Color::White => score,
        Color::Black => -score,
    }
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
