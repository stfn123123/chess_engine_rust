// Walking the move tree.
//
// The search is negamax with alpha-beta pruning. Negamax, because the evaluation
// always scores for the side to move: a node asks "how good is this for me", and
// the node one ply up negates the answer, which is exactly what "good for you is
// bad for me" means.
//
// Alpha is the best the side to move has already been promised somewhere else in
// the tree, beta the best its opponent has. Once a move here beats beta, the
// opponent would never let the game reach this position at all, so the moves after
// it need not be looked at. The cutoff changes nothing about the move that comes
// out - it only saves work.
//
// The depth asked for is only where the full move list stops - quiescence carries on
// with captures from there, so no line is scored in the middle of a trade.
//
// count_positions is the perft: it plays out every legal move sequence up to a
// given depth and counts the leaves. The counts for known positions are published,
// so they are the way to tell whether move generation is correct.

use crate::board::Board;
use crate::board::chess_move::Move;
use crate::board::piece::PieceType;
use crate::evaluate::{MATE, evaluate, piece_value};

// how deep the search runs unless something asks for another depth
pub const DEFAULT_DEPTH: u32 = 4;

const INFINITY: i32 = 1_000_000;

// how far short of alpha a capture may fall and still be worth looking at - covers
// the piece-square swing of both pieces, which is not known before the move is played
const DELTA_MARGIN: i32 = 200;

pub struct SearchResult {
    pub depth: u32,
    pub best_move: Option<Move>,
    pub score: i32,
    pub positions_searched: u64,
}

// the best move for the side to move, searched `depth` plies deep
pub fn find_best_move(board: &mut Board, depth: u32) -> SearchResult {
    let mut result = SearchResult {
        depth,
        best_move: None,
        score: 0,
        positions_searched: 1,
    };

    // nothing to search: the game is over, or the caller asked for no depth at all
    let moves = board.legal_moves();
    if moves.is_empty() {
        result.score = terminal_score(board, 0);
        return result;
    }

    if depth == 0 {
        result.score = quiescence(
            board,
            -INFINITY,
            INFINITY,
            0,
            &mut result.positions_searched,
        );
        return result;
    }

    let mut alpha = -INFINITY;

    for chess_move in moves {
        board.make_move(&chess_move);
        let score = -alpha_beta(
            board,
            depth - 1,
            -INFINITY,
            -alpha,
            1,
            &mut result.positions_searched,
        );
        board.undo_move();

        if result.best_move.is_none() || score > alpha {
            alpha = score;
            result.best_move = Some(chess_move);
        }
    }

    result.score = alpha;
    result
}

fn alpha_beta(
    board: &mut Board,
    depth: u32,
    mut alpha: i32,
    beta: i32,
    ply: u32,
    positions_searched: &mut u64,
) -> i32 {
    *positions_searched += 1;

    // a position that has stood on the board three times is a draw whatever the
    // pieces say, and it is the moves played to get here that make it one - so the
    // search is the only place that can see it, and it holds at every node rather
    // than only at the ones it stops on
    if board.is_threefold_repetition() {
        return 0;
    }

    if depth == 0 {
        return quiescence(board, alpha, beta, ply, positions_searched);
    }

    // the move list is in hand here, so whether the game ended costs nothing to ask
    let moves = board.legal_moves();
    if moves.is_empty() {
        return terminal_score(board, ply);
    }

    for chess_move in moves {
        board.make_move(&chess_move);
        let score = -alpha_beta(board, depth - 1, -beta, -alpha, ply + 1, positions_searched);
        board.undo_move();

        if score >= beta {
            return beta;
        }

        if score > alpha {
            alpha = score;
        }
    }

    alpha
}

// what a node with no legal move left is worth to the side to move: it has been
// mated, or it is stalemated and the game is a draw
// the mate is worth a little less the further down the tree it is, so of two winning
// lines the search takes the shorter one
// only a caller holding the move list knows this applies, which is why it lives here
// and not in the evaluation
fn terminal_score(board: &Board, ply: u32) -> i32 {
    if board.is_check(board.turn()) {
        -MATE + ply as i32
    } else {
        0
    }
}

// captures only, until nothing is hanging, then evaluate
// the window is the caller's, not a fresh one - that is where most of the pruning is
fn quiescence(
    board: &mut Board,
    mut alpha: i32,
    beta: i32,
    ply: u32,
    positions_searched: &mut u64,
) -> i32 {
    *positions_searched += 1;

    // no standing pat out of a check, and every evasion counts, not only the captures
    if board.is_check(board.turn()) {
        let moves = board.legal_moves();
        if moves.is_empty() {
            return terminal_score(board, ply);
        }

        for chess_move in moves {
            board.make_move(&chess_move);
            let score = -quiescence(board, -beta, -alpha, ply + 1, positions_searched);
            board.undo_move();

            if score >= beta {
                return beta;
            }
            if score > alpha {
                alpha = score;
            }
        }

        return alpha;
    }

    // nobody is forced to capture, so this is a floor - asked before generating
    // anything, because most nodes down here cut off on it
    let stand_pat = evaluate(board);
    if stand_pat >= beta {
        return beta;
    }
    if stand_pat > alpha {
        alpha = stand_pat;
    }

    let mut captures = board.legal_captures();
    order_captures(&mut captures);

    for chess_move in captures {
        // delta pruning: winning this piece for free still would not reach alpha
        if stand_pat + optimistic_gain(&chess_move) + DELTA_MARGIN < alpha {
            continue;
        }

        board.make_move(&chess_move);
        let score = -quiescence(board, -beta, -alpha, ply + 1, positions_searched);
        board.undo_move();

        if score >= beta {
            return beta;
        }
        if score > alpha {
            alpha = score;
        }
    }

    alpha
}

// MVV-LVA: the victim weighs eight times the attacker, so what is taken decides the
// order and what takes it only breaks ties
fn order_captures(captures: &mut [Move]) {
    captures.sort_unstable_by_key(|chess_move| {
        let victim = chess_move
            .captured
            .map_or(0, |piece| piece_value(piece.piece_type()));
        let attacker = piece_value(chess_move.piece.piece_type());

        // negated because sort_unstable_by_key orders ascending
        -(victim * 8 - attacker)
    });
}

// the most a capture could bring in: what it takes, plus what a promotion turns into
fn optimistic_gain(chess_move: &Move) -> i32 {
    let victim = chess_move
        .captured
        .map_or(0, |piece| piece_value(piece.piece_type()));
    let promotion = chess_move
        .promotion
        .map_or(0, |piece_type| {
            piece_value(piece_type) - piece_value(PieceType::Pawn)
        });

    (victim + promotion) as i32
}

// how many positions are `depth` plies away - the check that move generation is
// right, not part of playing a game
#[allow(dead_code)]
pub fn count_positions(board: &mut Board, depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }

    let mut positions = 0;
    for chess_move in board.legal_moves() {
        board.make_move(&chess_move);
        positions += count_positions(board, depth - 1);
        board.undo_move();
    }

    positions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::piece::{Color, Piece, PieceType};

    fn start_position() -> Board {
        let mut board = Board::new();
        board.set_start_position();
        board
    }

    // the published perft numbers for the starting position
    // depth 5 (4,865,609) is left out because it is slow in a debug build
    #[test]
    fn perft_from_the_start_position() {
        let expected = [1, 20, 400, 8_902, 197_281];
        let mut board = start_position();

        for (depth, &positions) in expected.iter().enumerate() {
            assert_eq!(count_positions(&mut board, depth as u32), positions, "at depth {depth}");
        }
    }

    // the search has to leave the board exactly as it found it
    #[test]
    fn searching_does_not_change_the_position() {
        let mut board = start_position();
        let hash_before = board.hash();

        find_best_move(&mut board, 3);

        assert_eq!(board.hash(), hash_before);
    }

    // plain negamax, no window and no cutoffs: the answer alpha-beta has to match
    fn negamax(board: &mut Board, depth: u32, ply: u32) -> i32 {
        if ply > 0 && board.is_threefold_repetition() {
            return 0;
        }

        if depth == 0 {
            // full window, so delta pruning can never fire
            return quiescence(board, -INFINITY, INFINITY, ply, &mut 0);
        }

        let moves = board.legal_moves();
        if moves.is_empty() {
            return terminal_score(board, ply);
        }

        let mut best = -INFINITY;
        for chess_move in moves {
            board.make_move(&chess_move);
            best = best.max(-negamax(board, depth - 1, ply + 1));
            board.undo_move();
        }

        best
    }

    // find_best_move's root loop, without a window and without cutoffs
    fn negamax_best(board: &mut Board, depth: u32) -> (Option<Move>, i32) {
        let moves = board.legal_moves();
        if moves.is_empty() {
            return (None, terminal_score(board, 0));
        }

        let mut best_move = None;
        let mut best_score = -INFINITY;
        for chess_move in moves {
            board.make_move(&chess_move);
            let score = -negamax(board, depth - 1, 1);
            board.undo_move();

            if best_move.is_none() || score > best_score {
                best_score = score;
                best_move = Some(chess_move);
            }
        }

        (best_move, best_score)
    }

    // a queen against two pawns, with the e6 pawn covering d5
    fn queen_against_pawns() -> Board {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        board.add_piece(Piece::new(PieceType::Queen, Color::White), 3); // d1
        board.add_piece(Piece::new(PieceType::Pawn, Color::White), 8); // a2
        board.add_piece(Piece::new(PieceType::King, Color::Black), 63); // h8
        board.add_piece(Piece::new(PieceType::Pawn, Color::Black), 44); // e6
        board.add_piece(Piece::new(PieceType::Pawn, Color::Black), 55); // h7
        board
    }

    // at one ply the search moves and stops, so Qxd5 reads as a free pawn - only
    // quiescence finds exd5
    #[test]
    fn a_defended_pawn_is_not_taken_by_the_queen() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        board.add_piece(Piece::new(PieceType::Queen, Color::White), 3); // d1
        board.add_piece(Piece::new(PieceType::Pawn, Color::White), 8); // a2
        board.add_piece(Piece::new(PieceType::King, Color::Black), 63); // h8
        board.add_piece(Piece::new(PieceType::Pawn, Color::Black), 35); // d5
        board.add_piece(Piece::new(PieceType::Pawn, Color::Black), 44); // e6
        board.add_piece(Piece::new(PieceType::Pawn, Color::Black), 55); // h7

        let result = find_best_move(&mut board, 1);
        let best = result.best_move.expect("white has moves");

        assert_ne!(
            (best.from, best.to),
            (3, 35),
            "the queen took a pawn that the e6 pawn defends"
        );
    }

    // exd5 is two plies inside a four ply search, so the horizon is no excuse
    #[test]
    fn the_queen_is_not_walked_onto_a_pawn() {
        let mut board = queen_against_pawns();

        let result = find_best_move(&mut board, 4);
        let best = result.best_move.expect("white has moves");

        assert_ne!(
            (best.from, best.to),
            (3, 35),
            "the queen stepped onto d5, where the e6 pawn takes it"
        );
        assert!(
            result.score > 500,
            "white is a queen up but the search scored {}",
            result.score
        );
    }

    // the same check as below, but somewhere there is something to win or lose
    #[test]
    fn pruning_does_not_change_the_score_in_a_tactical_position() {
        let mut board = queen_against_pawns();

        for depth in 1..=4 {
            let searched = find_best_move(&mut board, depth);
            let (_, reference) = negamax_best(&mut board, depth);

            assert_eq!(searched.score, reference, "at depth {depth}");
        }
    }

    // the whole point of the pruning: it only skips moves that cannot change the
    // outcome, so the score has to come out the same as a full search
    #[test]
    fn pruning_does_not_change_the_score() {
        let mut board = start_position();

        for depth in 1..=3 {
            let searched = find_best_move(&mut board, depth);
            assert_eq!(
                searched.score,
                negamax(&mut board, depth, 0),
                "at depth {depth}"
            );
        }
    }

    // a game that is already over scores as what it is, not as what is left standing
    #[test]
    fn a_finished_game_scores_as_mate_or_draw() {
        // fool's mate: 1. f3 e5 2. g4 Qh4#
        let mut mated = start_position();
        mated.make_move_from_squares(13, 21, None); // f2f3
        mated.make_move_from_squares(52, 36, None); // e7e5
        mated.make_move_from_squares(14, 30, None); // g2g4
        mated.make_move_from_squares(59, 31, None); // d8h4

        assert!(mated.is_checkmate());
        assert_eq!(find_best_move(&mut mated, 3).score, -MATE);

        // black is a queen down and has no move to make, which is a draw, not a loss
        let mut stalemated = Board::new();
        stalemated.add_piece(Piece::new(PieceType::King, Color::Black), 63); // h8
        stalemated.add_piece(Piece::new(PieceType::King, Color::White), 53); // f7
        stalemated.add_piece(Piece::new(PieceType::Queen, Color::White), 38); // g5
        // Qg5-g6 takes g7, g8 and h7 away without giving check
        stalemated.make_move_from_squares(38, 46, None);

        assert!(stalemated.is_stalemate());
        assert_eq!(find_best_move(&mut stalemated, 3).score, 0);
    }

    // the mate is delivered by the deepest move the search makes, so the position it
    // leads to is a leaf - which is exactly where a search that only looks for mate
    // where it has a move list anyway would miss it
    #[test]
    fn a_mate_on_the_horizon_is_found() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        board.add_piece(Piece::new(PieceType::King, Color::Black), 63); // h8
        board.add_piece(Piece::new(PieceType::Rook, Color::White), 48); // a7
        board.add_piece(Piece::new(PieceType::Rook, Color::White), 1); // b1

        // one ply: white moves, and whatever it moves to is scored as a leaf
        let result = find_best_move(&mut board, 1);

        assert_eq!(result.score, MATE - 1, "the mate was not seen at the leaf");
    }

    // a queen standing there for nothing gets taken
    #[test]
    fn a_hanging_queen_is_taken() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        board.add_piece(Piece::new(PieceType::King, Color::Black), 60); // e8
        board.add_piece(Piece::new(PieceType::Rook, Color::White), 0); // a1
        board.add_piece(Piece::new(PieceType::Queen, Color::Black), 8); // a2

        let result = find_best_move(&mut board, 2);
        let best = result.best_move.expect("white has moves");

        assert_eq!((best.from, best.to), (0, 8), "white played something else");

        // taking the queen leaves white a rook against a bare king, so the score is
        // the rook - not the queen, which is off the board rather than white's - less
        // whatever the tables say about where the two kings and the rook end up
        assert!(
            result.score > 400,
            "a rook against a bare king scored {}",
            result.score
        );
    }

    // mate in one, found and scored as a mate rather than as material
    #[test]
    fn mate_in_one_is_found() {
        // the two rook mate: Rb1-b8 checks along the eighth rank while the rook on
        // a7 takes the seventh away from the king
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        board.add_piece(Piece::new(PieceType::King, Color::Black), 63); // h8
        board.add_piece(Piece::new(PieceType::Rook, Color::White), 48); // a7
        board.add_piece(Piece::new(PieceType::Rook, Color::White), 1); // b1

        let result = find_best_move(&mut board, 3);
        let best = result.best_move.expect("white has moves");

        // a mate at the very next move, not material worth a few pawns
        assert!(
            result.score > MATE - 100,
            "the mate scored {} instead",
            result.score
        );

        board.make_move(&best);
        assert!(board.is_checkmate(), "the move played was not mate");
    }
}
