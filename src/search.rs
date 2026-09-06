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
// How early the cutoff comes is down to the order the moves are tried in, which is
// what MoveOrder is for: it guesses at the good ones and hands them out first.
//
// The same position is reached over and over by different move orders, so a search
// that remembers what it found is spared searching it again: the transposition table
// answers a node outright when what it holds was searched at least as deep, and hands
// over the move that was best there when it cannot. The table outlives the single
// search, which is what makes the move it kept worth having at the root too.
//
// None of that is asked at all while the game is still in the opening book: a
// position somebody has already written a move down for is answered out of the book,
// and the search starts where the book runs out.
//
// The depth asked for is only where the full move list stops - quiescence carries on
// with captures from there, so no line is scored in the middle of a trade.
//
// count_positions is the perft: it plays out every legal move sequence up to a
// given depth and counts the leaves. The counts for known positions are published,
// so they are the way to tell whether move generation is correct.

use crate::board::Board;
use crate::board::chess_move::Move;
use crate::board::piece::{Color, PieceType};
use crate::board::square::offset;
use crate::evaluate::{MATE, evaluate, piece_value};
use crate::opening::OpeningBook;
use crate::transposition::{NodeType, TranspositionTable};

// how deep the search runs unless something asks for another depth
pub const DEFAULT_DEPTH: u32 = 6;

const INFINITY: i32 = 1_000_000;

// the most moves a position has ever been found to allow is 218, so a list never
// outgrows this - it is the size of the score buffer the ordering picks out of
const MAX_MOVES: usize = 256;

// how far short of alpha a capture may fall and still be worth looking at - covers
// the piece-square swing of both pieces, which is not known before the move is played
const DELTA_MARGIN: i32 = 200;

// the move the table kept, put ahead of anything the guesswork below can score
const TABLE_MOVE_SCORE: i32 = 1_000_000;

pub struct SearchResult {
    pub depth: u32,
    pub best_move: Option<Move>,
    pub score: i32,
    pub positions_searched: u64,
    // how many nodes the table answered without searching them
    pub table_cutoffs: u64,
    // how much of the table has been written, 0.0 to 1.0
    pub table_fill: f32,
    // whether the move was read out of the opening book instead of searched for
    pub from_book: bool,
}

// The engine, as much of it as outlives a single search.
//
// That is the opening book, which is the same book all game, and the transposition
// table: a game is one position after another, and most of what was learned about the
// last one is still true of this one.
pub struct Search {
    table: TranspositionTable,
    // asked before any searching, and None for a caller that wants the search itself
    book: Option<OpeningBook>,
    // the nodes of the search that is running, counted from its root
    positions_searched: u64,
}

impl Search {
    pub fn new(table_megabytes: usize) -> Search {
        Search {
            table: TranspositionTable::new(table_megabytes),
            book: Some(OpeningBook::new()),
            positions_searched: 0,
        }
    }

    // an engine that searches every position, opening or not - what analysing a
    // position means, as against playing it
    pub fn without_book(table_megabytes: usize) -> Search {
        Search {
            book: None,
            ..Search::new(table_megabytes)
        }
    }

    // the best move for the side to move, searched `depth` plies deep
    pub fn find_best_move(&mut self, board: &mut Board, depth: u32) -> SearchResult {
        // a position the book holds is answered without searching anything at all,
        // which is what the book is for
        if let Some(opening) = self.book.as_ref().and_then(|book| book.move_for(board)) {
            return SearchResult {
                depth: 0,
                best_move: Some(opening),
                score: 0,
                positions_searched: 0,
                table_cutoffs: 0,
                table_fill: self.table.fill(),
                from_book: true,
            };
        }

        self.table.start_search();
        self.positions_searched = 1;

        let (best_move, score) = self.search_root(board, depth);

        SearchResult {
            depth,
            best_move,
            score,
            positions_searched: self.positions_searched,
            table_cutoffs: self.table.cutoffs(),
            table_fill: self.table.fill(),
            from_book: false,
        }
    }

    // the root: like alpha_beta with the window wide open, except that it has to come
    // out with a move and so can never be answered by the table alone
    fn search_root(&mut self, board: &mut Board, depth: u32) -> (Option<Move>, i32) {
        // nothing to search: the game is over, or the caller asked for no depth at all
        let moves = board.legal_moves();
        if moves.is_empty() {
            return (None, terminal_score(board, 0));
        }

        if depth == 0 {
            return (None, self.quiescence(board, -INFINITY, INFINITY, 0));
        }

        let mut best_move = None;
        let mut alpha = -INFINITY;

        // the move that was best here last time, which after a move is played is
        // usually still the answer to what the opponent just did
        let table_move = self.table.best_move(board.hash());

        for chess_move in MoveOrder::new(board, moves, table_move) {
            board.make_move(&chess_move);
            let score = -self.alpha_beta(board, depth - 1, -INFINITY, -alpha, 1);
            board.undo_move();

            if best_move.is_none() || score > alpha {
                alpha = score;
                best_move = Some(chess_move);
            }
        }

        // nothing was cut off up here, so this is what the position is worth
        self.table
            .store(board.hash(), depth, 0, alpha, NodeType::Exact, best_move);

        (best_move, alpha)
    }

    fn alpha_beta(
        &mut self,
        board: &mut Board,
        depth: u32,
        mut alpha: i32,
        beta: i32,
        ply: u32,
    ) -> i32 {
        self.positions_searched += 1;

        // a repeated position is a draw whatever the pieces say, and it is the moves played
        // to get here that make it one - so the search is the only place that can see it,
        // and it holds at every node rather than only at the ones it stops on
        if board.is_repetition_draw(ply) {
            return 0;
        }

        // asked after the repetition, which the table cannot answer: it files positions,
        // and whether one of them is a draw is a matter of the moves that led to it
        let probe = self.table.probe(board.hash(), depth, ply, alpha, beta);

        // this position was searched before, at least as deep, and what came out of it
        // settles the question this window is asking
        if let Some(score) = probe.cutoff {
            // a stored bound can lie outside the window, and a node answers within it
            return score.clamp(alpha, beta);
        }

        if depth == 0 {
            return self.quiescence(board, alpha, beta, ply);
        }

        // the move list is in hand here, so whether the game ended costs nothing to ask
        let moves = board.legal_moves();
        if moves.is_empty() {
            return terminal_score(board, ply);
        }

        // asked only once there is a move to make: a mate delivered on the last ply of the
        // fifty ends the game as a mate, not as a draw
        // the table cannot see this coming, since the counter is not part of the key -
        // right at the fifty it can hand back a score for a position that is a draw here
        if board.is_fifty_move_draw() {
            return 0;
        }

        // until a move beats alpha there is nothing to say about this node but that it
        // is worth no more than alpha
        let mut node_type = NodeType::UpperBound;
        let mut best_move = None;

        for chess_move in MoveOrder::new(board, moves, probe.best_move) {
            board.make_move(&chess_move);
            let score = -self.alpha_beta(board, depth - 1, -beta, -alpha, ply + 1);
            board.undo_move();

            if score >= beta {
                // the rest of the moves were never looked at, so beta is only a floor -
                // and this move is the one to try first next time
                self.table.store(
                    board.hash(),
                    depth,
                    ply,
                    beta,
                    NodeType::LowerBound,
                    Some(chess_move),
                );
                return beta;
            }

            if score > alpha {
                alpha = score;
                best_move = Some(chess_move);
                node_type = NodeType::Exact;
            }
        }

        self.table
            .store(board.hash(), depth, ply, alpha, node_type, best_move);

        alpha
    }

    // captures only, until nothing is hanging, then evaluate
    // the window is the caller's, not a fresh one - that is where most of the pruning is
    // nothing is filed here: these scores stop at the first quiet position rather than
    // at a depth, so there is no depth to file them under
    fn quiescence(&mut self, board: &mut Board, mut alpha: i32, beta: i32, ply: u32) -> i32 {
        self.positions_searched += 1;

        // no standing pat out of a check, and every evasion counts, not only the captures
        if board.is_check(board.turn()) {
            let moves = board.legal_moves();
            if moves.is_empty() {
                return terminal_score(board, ply);
            }

            for chess_move in moves {
                board.make_move(&chess_move);
                let score = -self.quiescence(board, -beta, -alpha, ply + 1);
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

        for chess_move in MoveOrder::captures(board.legal_captures()) {
            // delta pruning: winning this piece for free still would not reach alpha
            if stand_pat + optimistic_gain(&chess_move) + DELTA_MARGIN < alpha {
                continue;
            }

            board.make_move(&chess_move);
            let score = -self.quiescence(board, -beta, -alpha, ply + 1);
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
}

// The moves of one node, handed out best first.
//
// Nothing is sorted. Every move is scored once up front, and each `next` walks what
// is left over for the best of it - a selection sort that stops when the caller
// stops asking. Most nodes cut off after a move or two, and a sort would have put
// the whole list in order to get there.
struct MoveOrder {
    moves: Vec<Move>,
    // scored in the same order as `moves`, and swapped along with them
    // an array rather than a Vec: this is built at every node, and a Vec here would
    // be an allocation at every node
    scores: [i32; MAX_MOVES],
    handed_out: usize,
}

impl MoveOrder {
    // the full move list, scored on everything move_score knows, with the move the
    // table kept for this position - if it has one - out in front
    fn new(board: &Board, moves: Vec<Move>, table_move: Option<Move>) -> MoveOrder {
        let mut scores = [0; MAX_MOVES];
        for (index, chess_move) in moves.iter().enumerate() {
            // matched against the generated list, so two positions sharing a slot can
            // never hand this node a move that is not legal in it
            scores[index] = if Some(*chess_move) == table_move {
                TABLE_MOVE_SCORE
            } else {
                move_score(board, chess_move)
            };
        }

        MoveOrder {
            moves,
            scores,
            handed_out: 0,
        }
    }

    // what quiescence walks: captures, with nothing but MVV-LVA to tell them apart
    fn captures(captures: Vec<Move>) -> MoveOrder {
        let mut scores = [0; MAX_MOVES];
        for (index, chess_move) in captures.iter().enumerate() {
            scores[index] = capture_score(chess_move);
        }

        MoveOrder {
            moves: captures,
            scores,
            handed_out: 0,
        }
    }
}

impl Iterator for MoveOrder {
    type Item = Move;

    fn next(&mut self) -> Option<Move> {
        let slot = self.handed_out;
        if slot >= self.moves.len() {
            return None;
        }

        // the best of what is left, brought to the front of it
        let mut best = slot;
        for index in slot + 1..self.moves.len() {
            if self.scores[index] > self.scores[best] {
                best = index;
            }
        }
        self.moves.swap(slot, best);
        self.scores.swap(slot, best);

        self.handed_out += 1;
        Some(self.moves[slot])
    }
}

// what a move looks worth without playing it, in centipawns
fn move_score(board: &Board, chess_move: &Move) -> i32 {
    let mut score = 0;
    let moved = chess_move.piece.piece_type();

    // MVV-LVA: take the most valuable piece with the least valuable one, so what is
    // taken decides and what takes it only breaks ties
    if let Some(captured) = chess_move.captured {
        score += 10 * piece_value(captured.piece_type()) as i32 - piece_value(moved) as i32;
    }

    // what the pawn turns into, less the pawn it stops being
    if let Some(promotion) = chess_move.promotion {
        score += (piece_value(promotion) - piece_value(PieceType::Pawn)) as i32;
    }

    // stepping in front of a pawn hands the piece over for a pawn, whatever else the
    // move does - pawns are left out, since standing up to one another is what they do
    if moved != PieceType::Pawn
        && attacked_by_pawn(board, chess_move.to, chess_move.piece.color().opponent())
    {
        score -= piece_value(moved) as i32;
    }

    score
}

// whether a pawn of `color` covers this square - read off the board as it stands, so
// the piece that is about to move is still on its old square
fn attacked_by_pawn(board: &Board, square: u8, color: Color) -> bool {
    // back down the direction the pawn moves in: the two squares it captures from
    let rank_step = -color.pawn_direction();

    [(-1, rank_step), (1, rank_step)].iter().any(|&step| {
        offset(square, step)
            .and_then(|from| board.piece_at(from))
            .is_some_and(|piece| piece.is(PieceType::Pawn) && piece.color() == color)
    })
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

// MVV-LVA: the victim weighs eight times the attacker, so what is taken decides the
// order and what takes it only breaks ties
fn capture_score(chess_move: &Move) -> i32 {
    let victim = chess_move
        .captured
        .map_or(0, |piece| piece_value(piece.piece_type()));
    let attacker = piece_value(chess_move.piece.piece_type());

    (victim * 8 - attacker) as i32
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

    // a table small enough that a test can have one of its own, and small enough that
    // positions land on the same slot - which is what the key check is there for
    // no book: what these tests are about is the searching, and the book would answer
    // the opening positions among them before any of it ran
    fn search() -> Search {
        Search::without_book(1)
    }

    // one search on a table nothing else has touched: what a test means when it does
    // not care what the table keeps from one search to the next
    fn find_best_move(board: &mut Board, depth: u32) -> SearchResult {
        search().find_best_move(board, depth)
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

    // plain negamax, no window, no cutoffs and no table: the answer alpha-beta has to
    // match. the Search it is handed is only there to run quiescence out of
    fn negamax(search: &mut Search, board: &mut Board, depth: u32, ply: u32) -> i32 {
        // the draw rules have to be read exactly as alpha_beta reads them, or the two
        // disagree about scores that have nothing to do with pruning
        if ply > 0 && board.is_repetition_draw(ply) {
            return 0;
        }

        if depth == 0 {
            // full window, so delta pruning can never fire
            return search.quiescence(board, -INFINITY, INFINITY, ply);
        }

        let moves = board.legal_moves();
        if moves.is_empty() {
            return terminal_score(board, ply);
        }

        if board.is_fifty_move_draw() {
            return 0;
        }

        let mut best = -INFINITY;
        for chess_move in moves {
            board.make_move(&chess_move);
            best = best.max(-negamax(search, board, depth - 1, ply + 1));
            board.undo_move();
        }

        best
    }

    // find_best_move's root loop, without a window and without cutoffs
    fn negamax_best(search: &mut Search, board: &mut Board, depth: u32) -> (Option<Move>, i32) {
        let moves = board.legal_moves();
        if moves.is_empty() {
            return (None, terminal_score(board, 0));
        }

        let mut best_move = None;
        let mut best_score = -INFINITY;
        for chess_move in moves {
            board.make_move(&chess_move);
            let score = -negamax(search, board, depth - 1, 1);
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
            let (_, reference) = negamax_best(&mut search(), &mut board, depth);

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
                negamax(&mut search(), &mut board, depth, 0),
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

    // the three things the ordering is built out of, on one board: the queen capture
    // comes first, and the rook stepping in front of a pawn comes last
    #[test]
    fn the_best_capture_leads_and_a_pawn_covered_square_trails() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        board.add_piece(Piece::new(PieceType::King, Color::Black), 63); // h8
        board.add_piece(Piece::new(PieceType::Rook, Color::White), 0); // a1
        board.add_piece(Piece::new(PieceType::Queen, Color::Black), 56); // a8
        board.add_piece(Piece::new(PieceType::Pawn, Color::Black), 41); // b6

        let moves = board.legal_moves();
        let ordered: Vec<Move> = MoveOrder::new(&board, moves, None).collect();

        let first = ordered.first().expect("white has moves");
        assert_eq!((first.from, first.to), (0, 56), "Rxa8 was not searched first");

        let last = ordered.last().expect("white has moves");
        assert_eq!((last.from, last.to), (0, 32), "Ra5, where b6 takes it, was not last");
    }

    // the ordering hands out the moves the search would otherwise have walked itself,
    // so losing or repeating one loses or repeats a whole subtree
    #[test]
    fn the_ordering_hands_out_every_move_once_best_first() {
        let board = start_position();
        let moves = board.legal_moves();

        let ordered: Vec<Move> = MoveOrder::new(&board, moves.clone(), None).collect();

        assert_eq!(ordered.len(), moves.len(), "the move list changed length");
        for chess_move in &moves {
            assert_eq!(
                ordered.iter().filter(|&handed| handed == chess_move).count(),
                1,
                "{chess_move:?} was dropped or handed out twice"
            );
        }

        let scores: Vec<i32> = ordered
            .iter()
            .map(|chess_move| move_score(&board, chess_move))
            .collect();
        assert!(
            scores.windows(2).all(|pair| pair[0] >= pair[1]),
            "the moves did not come out best first: {scores:?}"
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

    // the two rook mate, as a position rather than as a search
    fn mate_in_one() -> Board {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        board.add_piece(Piece::new(PieceType::King, Color::Black), 63); // h8
        board.add_piece(Piece::new(PieceType::Rook, Color::White), 48); // a7
        board.add_piece(Piece::new(PieceType::Rook, Color::White), 1); // b1
        board
    }

    // the table is there to save work, not to change answers: the same position
    // searched again on a table that already holds it comes out the same way
    #[test]
    fn a_second_search_of_a_position_answers_as_the_first_did() {
        for mut board in [start_position(), mate_in_one()] {
            let mut engine = search();

            let first = engine.find_best_move(&mut board, 3);
            let second = engine.find_best_move(&mut board, 3);

            assert_eq!(second.score, first.score, "the score moved on the second search");
            assert_eq!(
                second.best_move, first.best_move,
                "the move moved on the second search"
            );
        }
    }

    // and it does save the work: the second search finds most of what it needs filed
    #[test]
    fn a_second_search_of_a_position_is_the_cheaper_one() {
        let mut board = start_position();
        let mut engine = search();

        let first = engine.find_best_move(&mut board, 3);
        let second = engine.find_best_move(&mut board, 3);

        assert!(
            second.positions_searched < first.positions_searched,
            "the second search cost {} against the first {}",
            second.positions_searched,
            first.positions_searched
        );
        assert!(second.table_cutoffs > 0, "the table answered nothing");
    }

    // a mate is stored counted from the position it was found at, so reading it back
    // somewhere else has to count it from the root again
    #[test]
    fn a_mate_keeps_its_distance_across_searches() {
        let mut board = mate_in_one();
        let mut engine = search();

        engine.find_best_move(&mut board, 3);
        let again = engine.find_best_move(&mut board, 3);

        assert_eq!(again.score, MATE - 1, "the mate came back as {}", again.score);
    }

    // whatever the guesswork thinks of it, the move the table kept is tried first
    #[test]
    fn the_stored_move_is_handed_out_first() {
        let board = start_position();
        let moves = board.legal_moves();

        // Nb1c3: a quiet move, which the ordering has no reason to put ahead of the
        // pawn moves that come before it in the list
        let table_move = *moves
            .iter()
            .find(|chess_move| (chess_move.from, chess_move.to) == (1, 18))
            .expect("Nb1c3 is legal");

        let ordered: Vec<Move> = MoveOrder::new(&board, moves, Some(table_move)).collect();

        assert_eq!(ordered[0], table_move, "the stored move was not searched first");
    }

    // the opening is looked up, not searched: no position is walked at all
    #[test]
    fn an_opening_is_played_out_of_the_book() {
        let mut board = start_position();
        let mut engine = Search::new(1);

        let result = engine.find_best_move(&mut board, 4);

        assert!(result.from_book, "the start position was searched, not looked up");
        assert_eq!(result.positions_searched, 0, "a book move cost a search anyway");

        // which of the openings it draws is the book's business, and it draws a new
        // one every game - what matters here is that it is a move that can be played
        let opening = result.best_move.expect("the book has an opening");
        assert!(
            board.legal_moves().contains(&opening),
            "the book opened with {}, which is not legal",
            opening.coordinates()
        );
    }

    // and where the book has nothing, the search takes over
    #[test]
    fn a_position_the_book_does_not_hold_is_searched() {
        let mut board = queen_against_pawns();
        let mut engine = Search::new(1);

        let result = engine.find_best_move(&mut board, 3);

        assert!(!result.from_book, "an endgame came out of an opening book");
        assert!(result.positions_searched > 1, "nothing was searched");
    }
}
