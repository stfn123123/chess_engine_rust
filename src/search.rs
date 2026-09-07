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
use crate::board::piece::{Color, Piece, PieceType};
use crate::board::square::{
    DIAGONAL_STEPS, KING_STEPS, KNIGHT_STEPS, STRAIGHT_STEPS, en_passant_captured_square, offset,
    ray,
};
use crate::evaluate::{MATE, evaluate, game_phase_of, piece_value};
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

// at or below this phase the margin is dropped: a pawn decides an endgame, and there is
// little piece-square swing left to cover once the pieces are off
const DELTA_ENDGAME_PHASE: f32 = 0.25;

// the move the table kept, put ahead of anything the guesswork below can score
const TABLE_MOVE_SCORE: i32 = 1_000_000;

// what the king is worth to an exchange: it can never be taken, so it has to outweigh
// anything winning it could bring in
const SEE_KING_VALUE: i32 = 10_000;

pub struct SearchResult {
    pub depth: u32,
    pub best_move: Option<Move>,
    pub score: i32,
    pub positions_searched: u64,
    pub positions_searched_quiescience: u64,
    // how many nodes the table answered without searching them
    pub table_cutoffs: u64,
    // how much of the table has been written, 0.0 to 1.0
    pub table_fill: f32,
    // whether the move was read out of the opening book instead of searched for
    pub from_book: bool,
}

// the opening book and transposition table, which outlive a single search
pub struct Search {
    table: TranspositionTable,
    // asked before any searching, and None for a caller that wants the search itself
    book: Option<OpeningBook>,
    // the nodes of the search that is running, counted from its root
    positions_searched: u64,
    positions_searched_quiescience: u64,
}

impl Search {
    pub fn new(table_megabytes: usize) -> Search {
        Search {
            table: TranspositionTable::new(table_megabytes),
            book: Some(OpeningBook::new()),
            positions_searched: 0,
            positions_searched_quiescience: 0,
        }
    }

    // searches every position, opening or not - used for analysis, not play
    pub fn without_book(table_megabytes: usize) -> Search {
        Search {
            book: None,
            ..Search::new(table_megabytes)
        }
    }

    // the best move for the side to move, searched `depth` plies deep
    pub fn find_best_move(&mut self, board: &mut Board, depth: u32) -> SearchResult {
        // a position the book holds is answered without searching anything at all
        if let Some(opening) = self.book.as_ref().and_then(|book| book.move_for(board)) {
            return SearchResult {
                depth: 0,
                best_move: Some(opening),
                score: 0,
                positions_searched: 0,
                positions_searched_quiescience: 0,
                table_cutoffs: 0,
                table_fill: self.table.fill(),
                from_book: true,
            };
        }

        self.table.start_search();
        self.positions_searched = 1;
        self.positions_searched_quiescience = 0;

        let (best_move, score) = self.search_root(board, depth);

        SearchResult {
            depth,
            best_move,
            score,
            positions_searched: self.positions_searched,
            positions_searched_quiescience: self.positions_searched_quiescience,
            table_cutoffs: self.table.cutoffs(),
            table_fill: self.table.fill(),
            from_book: false,
        }
    }

    // like alpha_beta with the window wide open, but must return a move
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
        // whether best_move repeats a position already on the board this game -
        // tracked only to break ties, never to prefer a move that scores worse
        let mut best_repeats = false;
        let mut alpha = -INFINITY;

        // usually still the answer to what the opponent just did
        let table_move = self.table.best_move(board.hash());

        for chess_move in MoveOrder::new(board, moves, table_move) {
            board.make_move(&chess_move);
            let score = -self.alpha_beta(board, depth - 1, -INFINITY, -alpha, 1);
            let repeats = board.position_repetitions() > 1;
            board.undo_move();

            // among moves the search scores the same, the one that does not repeat
            // a position already reached this game is the one worth playing
            let better =
                best_move.is_none() || score > alpha || (score == alpha && best_repeats && !repeats);

            if better {
                alpha = score;
                best_move = Some(chess_move);
                best_repeats = repeats;
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

        // a repeated position is a draw regardless of material; only the search sees it
        if board.is_repetition_draw(ply) {
            return 0;
        }

        // the table files positions, not the moves that led to them, so it's probed after
        let probe = self.table.probe(board.hash(), depth, ply, alpha, beta);

        if let Some(score) = probe.cutoff {
            // a stored bound can lie outside the window, so clamp into it
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

        // a mate on the last ply of the fifty still counts as mate, not a draw
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
                // beta is only a floor here, and this is the move to try first next time
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

    // captures only, until nothing is hanging, then evaluate; nothing is filed in the
    // table here, since these scores stop at a quiet position rather than at a depth
    fn quiescence(&mut self, board: &mut Board, mut alpha: i32, beta: i32, ply: u32) -> i32 {
        self.positions_searched += 1;
        self.positions_searched_quiescience +=1;

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

        // nobody is forced to capture, so this is a floor; most nodes cut off on it
        let stand_pat = evaluate(board);
        if stand_pat >= beta {
            return beta;
        }
        if stand_pat > alpha {
            alpha = stand_pat;
        }

        // the same for every capture here, so it is worked out once
        let margin = delta_margin(board);

        for chess_move in MoveOrder::captures(board.legal_captures()) {
            // delta pruning: winning this piece for free still would not reach alpha
            if stand_pat + optimistic_gain(&chess_move) + margin < alpha {
                continue;
            }

            // and a capture the recaptures win back is not worth a subtree of its own
            if see(board, &chess_move) < 0 {
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

// the moves of one node, handed out best first: a selection sort that stops when the
// caller stops asking, since most nodes cut off after a move or two
struct MoveOrder {
    moves: Vec<Move>,
    // scored in the same order as `moves`, and swapped along with them; an array rather
    // than a Vec, since this is built fresh at every node
    scores: [i32; MAX_MOVES],
    handed_out: usize,
}

impl MoveOrder {
    // scored move list, with the table's move for this position - if it has one - in front
    fn new(board: &Board, moves: Vec<Move>, table_move: Option<Move>) -> MoveOrder {
        let mut scores = [0; MAX_MOVES];
        for (index, chess_move) in moves.iter().enumerate() {
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

    // MVV-LVA: what is taken decides, what takes it only breaks ties
    if let Some(captured) = chess_move.captured {
        score += 10 * piece_value(captured.piece_type()) as i32 - piece_value(moved) as i32;
    }

    // what the pawn turns into, less the pawn it stops being
    if let Some(promotion) = chess_move.promotion {
        score += (piece_value(promotion) - piece_value(PieceType::Pawn)) as i32;
    }

    // stepping in front of a pawn hands the piece over for a pawn; pawns are left out
    if moved != PieceType::Pawn
        && attacked_by_pawn(board, chess_move.to, chess_move.piece.color().opponent())
    {
        score -= piece_value(moved) as i32;
    }

    score
}

// whether a pawn of `color` covers this square, read off the board as it stands
fn attacked_by_pawn(board: &Board, square: u8, color: Color) -> bool {
    // back down the direction the pawn moves in: the two squares it captures from
    let rank_step = -color.pawn_direction();

    [(-1, rank_step), (1, rank_step)].iter().any(|&step| {
        offset(square, step)
            .and_then(|from| board.piece_at(from))
            .is_some_and(|piece| piece.is(PieceType::Pawn) && piece.color() == color)
    })
}

// a node with no legal move left: mate, or a draw by stalemate; a mate is worth a
// little less the deeper it is, so the search takes the shortest winning line
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

// what delta pruning allows here: nothing once the endgame is reached, where a capture
// short of alpha today is what a passed pawn is made of tomorrow
fn delta_margin(board: &Board) -> i32 {
    if game_phase_of(board) <= DELTA_ENDGAME_PHASE {
        0
    } else {
        DELTA_MARGIN
    }
}

// what a capture is worth once both sides have taken on the square with their least
// valuable attacker in turn - negative means the recaptures win the material back
fn see(board: &Board, chess_move: &Move) -> i32 {
    let to = chess_move.to;
    let color = chess_move.piece.color();

    let mut gone = bit(chess_move.from);
    // the pawn taken en passant never stood on `to`, and its square opens a rank
    if chess_move.en_passant {
        gone |= bit(en_passant_captured_square(to, color));
    }

    let promotion = chess_move.promotion.map_or(0, |piece_type| {
        see_value(piece_type) - see_value(PieceType::Pawn)
    });

    // gains[n] is what the side taking nth is left with if the exchange stops there
    let mut gains = [0i32; 32];
    gains[0] = chess_move
        .captured
        .map_or(0, |piece| see_value(piece.piece_type()))
        + promotion;

    // what stands on the square now, waiting to be taken back
    let mut on_square = match chess_move.promotion {
        Some(piece_type) => see_value(piece_type),
        None => see_value(chess_move.piece.piece_type()),
    };
    let mut side = color.opponent();
    let mut depth = 0;

    while let Some((square, value)) = least_valuable_attacker(board, to, side, gone) {
        if depth + 1 == gains.len() {
            break;
        }

        // the king may only take where nothing is left to take it back
        let after = gone | bit(square);
        if value == SEE_KING_VALUE
            && least_valuable_attacker(board, to, side.opponent(), after).is_some()
        {
            break;
        }

        depth += 1;
        gains[depth] = on_square - gains[depth - 1];
        gone = after;
        on_square = value;
        side = side.opponent();
    }

    // back down the list: nobody has to take, so each side stops where taking is worse
    while depth > 0 {
        gains[depth - 1] = -i32::max(-gains[depth - 1], gains[depth]);
        depth -= 1;
    }

    gains[0]
}

// the cheapest piece of `color` that can take on `square`, with what the exchange has
// already taken off treated as gone
fn least_valuable_attacker(
    board: &Board,
    square: u8,
    color: Color,
    gone: u64,
) -> Option<(u8, i32)> {
    let mut best = None;

    // a pawn attacks from one rank back along its own direction of travel
    let pawn_step = -color.pawn_direction();
    for file_step in [-1, 1] {
        if let Some(from) = offset(square, (file_step, pawn_step))
            && standing(board, from, gone)
                .is_some_and(|piece| piece.is(PieceType::Pawn) && piece.color() == color)
        {
            // nothing takes more cheaply than a pawn
            return Some((from, see_value(PieceType::Pawn)));
        }
    }

    for (steps, piece_type) in [
        (&KNIGHT_STEPS, PieceType::Knight),
        (&KING_STEPS, PieceType::King),
    ] {
        for &step in steps {
            if let Some(from) = offset(square, step)
                && standing(board, from, gone)
                    .is_some_and(|piece| piece.is(piece_type) && piece.color() == color)
            {
                cheaper(&mut best, from, see_value(piece_type));
            }
        }
    }

    // the first piece a line runs into is the only one that can attack along it
    for (steps, slider) in [
        (&DIAGONAL_STEPS, PieceType::Bishop),
        (&STRAIGHT_STEPS, PieceType::Rook),
    ] {
        for &step in steps {
            for from in ray(square, step) {
                let Some(piece) = standing(board, from, gone) else {
                    continue;
                };
                if piece.color() == color && (piece.is(slider) || piece.is(PieceType::Queen)) {
                    cheaper(&mut best, from, see_value(piece.piece_type()));
                }
                break;
            }
        }
    }

    best
}

fn cheaper(best: &mut Option<(u8, i32)>, square: u8, value: i32) {
    if best.is_none_or(|(_, current)| value < current) {
        *best = Some((square, value));
    }
}

// the piece on a square, once what the exchange has taken off is gone
fn standing(board: &Board, square: u8, gone: u64) -> Option<Piece> {
    if gone & bit(square) == 0 {
        board.piece_at(square)
    } else {
        None
    }
}

// piece values as an exchange counts them, the king included
fn see_value(piece_type: PieceType) -> i32 {
    match piece_type {
        PieceType::King => SEE_KING_VALUE,
        other => piece_value(other) as i32,
    }
}

// a square as a single bit, so a set of squares fits into one number
fn bit(square: u8) -> u64 {
    1 << square
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

    // a small table of its own, and no book, since these tests are about the searching
    fn search() -> Search {
        Search::without_book(1)
    }

    // one search on a table nothing else has touched
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

    // plain negamax, no window, no cutoffs and no table: the answer alpha_beta must match
    fn negamax(search: &mut Search, board: &mut Board, depth: u32, ply: u32) -> i32 {
        // draw rules read exactly as alpha_beta reads them, or scores disagree for free
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

    // at one ply Qxd5 reads as a free pawn; only quiescence finds exd5
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

    // pruning only skips moves that can't change the outcome, so the score must match
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

    // the mate is delivered by the deepest move, so the position it leads to is a leaf
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

        // taking the queen leaves white a rook against a bare king
        assert!(
            result.score > 400,
            "a rook against a bare king scored {}",
            result.score
        );
    }

    // the queen capture comes first, the rook stepping in front of a pawn comes last
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

    // losing or repeating a move here loses or repeats a whole subtree in the search
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

    // the generated capture between two squares, which is what see is asked about
    fn capture_of(board: &Board, from: u8, to: u8) -> Move {
        board
            .legal_captures()
            .into_iter()
            .find(|candidate| candidate.from == from && candidate.to == to)
            .expect("that capture is legal here")
    }

    // the margin covers a piece-square swing that is not there any more once the pieces
    // are off, and in an endgame the pawn it prunes away is the game
    #[test]
    fn the_delta_margin_goes_out_in_the_endgame() {
        assert_eq!(delta_margin(&start_position()), DELTA_MARGIN);

        let mut endgame = Board::new();
        endgame.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        endgame.add_piece(Piece::new(PieceType::King, Color::Black), 60); // e8
        endgame.add_piece(Piece::new(PieceType::Rook, Color::White), 0); // a1

        assert_eq!(delta_margin(&endgame), 0);

        // a queen and a rook are a phase of exactly 0.25, which the line lets through
        let mut boundary = Board::new();
        boundary.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        boundary.add_piece(Piece::new(PieceType::King, Color::Black), 60); // e8
        boundary.add_piece(Piece::new(PieceType::Queen, Color::White), 3); // d1
        boundary.add_piece(Piece::new(PieceType::Rook, Color::Black), 56); // a8

        assert_eq!(game_phase_of(&boundary), DELTA_ENDGAME_PHASE);
        assert_eq!(delta_margin(&boundary), 0);
    }

    // a piece standing for nothing is worth what it is worth
    #[test]
    fn see_wins_an_undefended_piece() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        board.add_piece(Piece::new(PieceType::King, Color::Black), 63); // h8
        board.add_piece(Piece::new(PieceType::Rook, Color::White), 0); // a1
        board.add_piece(Piece::new(PieceType::Queen, Color::Black), 8); // a2

        let capture = capture_of(&board, 0, 8);

        assert_eq!(see(&board, &capture), piece_value(PieceType::Queen) as i32);
    }

    // Qxd5 wins a pawn and loses the queen to exd5, which is what gets pruned
    #[test]
    fn see_refuses_a_capture_the_recapture_wins_back() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        board.add_piece(Piece::new(PieceType::Queen, Color::White), 3); // d1
        board.add_piece(Piece::new(PieceType::King, Color::Black), 63); // h8
        board.add_piece(Piece::new(PieceType::Pawn, Color::Black), 35); // d5
        board.add_piece(Piece::new(PieceType::Pawn, Color::Black), 44); // e6

        let capture = capture_of(&board, 3, 35);

        assert_eq!(
            see(&board, &capture),
            (piece_value(PieceType::Pawn) - piece_value(PieceType::Queen)) as i32
        );
    }

    // pawn for pawn comes out at nothing, and nothing is not below the pruning line
    #[test]
    fn see_counts_an_even_trade_as_nothing() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        board.add_piece(Piece::new(PieceType::King, Color::Black), 63); // h8
        board.add_piece(Piece::new(PieceType::Pawn, Color::White), 28); // e4
        board.add_piece(Piece::new(PieceType::Pawn, Color::Black), 35); // d5
        board.add_piece(Piece::new(PieceType::Pawn, Color::Black), 42); // c6

        let capture = capture_of(&board, 28, 35);

        assert_eq!(see(&board, &capture), 0);
    }

    // the rook on d1 only joins in once the one on d2 has left the file
    #[test]
    fn see_counts_the_slider_a_capture_uncovers() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        board.add_piece(Piece::new(PieceType::King, Color::Black), 63); // h8
        board.add_piece(Piece::new(PieceType::Rook, Color::White), 3); // d1
        board.add_piece(Piece::new(PieceType::Rook, Color::White), 11); // d2
        board.add_piece(Piece::new(PieceType::Pawn, Color::Black), 35); // d5
        board.add_piece(Piece::new(PieceType::Pawn, Color::Black), 44); // e6
        board.add_piece(Piece::new(PieceType::Rook, Color::Black), 59); // d8

        let capture = capture_of(&board, 11, 35);

        // Rxd5 exd5 and white stops: taking back with d1 only loses that rook to Rxd5 too
        assert_eq!(
            see(&board, &capture),
            (piece_value(PieceType::Pawn) - piece_value(PieceType::Rook)) as i32
        );
    }

    // mate in one, found and scored as a mate rather than as material
    #[test]
    fn mate_in_one_is_found() {
        // the two rook mate: Rb1-b8 checks while the rook on a7 covers the seventh
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

    // the table saves work, not changes answers: a re-searched position comes out the same
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

    // a mate is stored counted from where it was found, read back counted from the root
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

        // Nb1c3: a quiet move the ordering has no reason to put ahead of the pawn moves
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

        // which opening it draws is the book's business; it just has to be playable
        let opening = result.best_move.expect("the book has an opening");
        assert!(
            board.legal_moves().contains(&opening),
            "the book opened with {}, which is not legal",
            opening.coordinates()
        );
    }

    // bare kings are insufficient material, so every move scores exactly 0 - the
    // only thing left to decide between them is whether one repeats a position
    // already reached, which the tie-break should steer away from
    #[test]
    fn a_tied_move_that_does_not_repeat_is_preferred() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 0); // a1
        board.add_piece(Piece::new(PieceType::King, Color::Black), 63); // h8

        // shuffle both kings back to the start, so Ka1-a2 is now a move that has
        // already been played from here once before
        board.make_move_from_squares(0, 8, None); // Ka1-a2
        board.make_move_from_squares(63, 55, None); // Kh8-h7
        board.make_move_from_squares(8, 0, None); // Ka2-a1
        board.make_move_from_squares(55, 63, None); // Kh7-h8

        let result = find_best_move(&mut board, 1);
        let best = result.best_move.expect("white has moves");

        assert_ne!(
            (best.from, best.to),
            (0, 8),
            "the search repeated Ka1-a2 over an equally scored fresh move"
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
