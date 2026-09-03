// Walking the move tree.
//
// count_positions is a perft: it plays out every legal move sequence up to a given
// depth and counts the leaves. The counts for known positions are published, so
// they are the way to tell whether move generation is correct - and the timing is
// the way to tell whether it is fast.

use crate::board::Board;

// how deep count_positions runs unless something asks for another depth
// the tree grows by a factor of about 30 per ply, so every step up here costs
// roughly thirty times the time the step before it did
pub const DEFAULT_DEPTH: u32 = 3;

pub struct SearchResult {
    // leaves reached, i.e. positions at exactly `depth` plies
    pub positions_found: u64,
    // every node visited on the way there, the leaves included
    pub positions_searched: u64,
}

pub fn count_positions(board: &mut Board, depth: u32) -> SearchResult {
    let mut result = SearchResult {
        positions_found: 0,
        positions_searched: 0,
    };
    // count_positions_inner(board, depth, &mut result);
    result
}

fn count_positions_inner(board: &mut Board, depth: u32, result: &mut SearchResult) {
    result.positions_searched += 1;

    if depth == 0 {
        result.positions_found += 1;
        return;
    }

    for chess_move in board.legal_moves() {
        board.make_move(&chess_move);
        count_positions_inner(board, depth - 1, result);
        board.undo_move();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            let found = count_positions(&mut board, depth as u32).positions_found;
            assert_eq!(found, positions, "at depth {depth}");
        }
    }

    // the search has to leave the board exactly as it found it
    #[test]
    fn searching_does_not_change_the_position() {
        let mut board = start_position();
        let hash_before = board.hash();

        count_positions(&mut board, 3);

        assert_eq!(board.hash(), hash_before);
    }
}
