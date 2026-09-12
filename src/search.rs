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
// what MoveOrder is for: it guesses at the good ones and hands them out first. What
// it has to go on is mostly the material a move wins, which says nothing at all about
// a quiet move - so the quiet moves that did cut a node off are remembered per ply and
// tried early at the next node of that ply. Sibling nodes differ by one move, and a
// refutation of one is usually a refutation of the next: those are the killers.
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
use crate::evaluate::{MATE, MATE_BOUND, evaluate, game_phase_of, piece_value};
use crate::opening::OpeningBook;
use crate::transposition::{NodeType, TranspositionTable};
use std::time::{Duration, Instant};

// how deep the search runs unless something asks for another depth
pub const DEFAULT_DEPTH: u32 = 6;

// the ceiling a timed search deepens towards: it stops on the clock long before this, but the
// loop still wants a bound, and one this far under MAX_PLY leaves quiescence its room
pub const MAX_SEARCH_DEPTH: u32 = 64;

// how often the clock is read, in nodes - a power of two so the test is a mask. Once per
// 2048 nodes is far below the noise floor of the search itself
const DEADLINE_CHECK_INTERVAL: u64 = 2048;

const INFINITY: i32 = 1_000_000;

// 218 legal moves is the most ever found in a position; the score buffer sizes to that
const MAX_MOVES: usize = 256;

// how far short of alpha a capture may fall and still be worth looking at
const DELTA_MARGIN: i32 = 200;

// dropped at or below this phase: a pawn decides an endgame, so there's little left to cover
const DELTA_ENDGAME_PHASE: f32 = 0.25;

// the move the table kept, ranked ahead of anything the guesswork below can score
const TABLE_MOVE_SCORE: i32 = 1_000_000;

// bound on the whole tree, not just the requested depth - quiescence carries ply past it
const MAX_PLY: usize = 128;

// two per ply: the first is almost always the one that cuts again, a third crowds out captures
const KILLERS_PER_PLY: usize = 2;

// killers rank after every capture and before ordinary quiet moves; second slot is the older killer
const KILLER_SCORES: [i32; KILLERS_PER_PLY] = [99, 98];

// no killers to offer - the root, and everything quiescence looks at
const NO_KILLERS: [Option<Move>; KILLERS_PER_PLY] = [None; KILLERS_PER_PLY];

// the king can never be taken, so it must outweigh anything winning it could bring in
const SEE_KING_VALUE: i32 = 10_000;

// TODO unverified: LMR is new and only measured on one position, at depth 10 with a 256 MB table.
// Before it: 63.21M nodes / 24.59s / 2,571k nps. After: 7.18M / 2.64s / 2,723k nps, 221,548
// reductions of which 0.2% were re-searched. It is the first change here that can alter the move
// found rather than only the speed, so it is not to be trusted until:
//   - the test suite passes; a few tests assert a best move at depth 4-5, inside LMR's range
//   - move and score match the pre-LMR search on many positions, mates included. A mate the old
//     search found and this one misses is the failure mode - a reduced quiet move that mattered
//   - the commit is on a branch of its own, so the verification has something to go back to
// Then tune, one change per measurement, watching the re-search share in the panel: past ~10% the
// knobs below are too aggressive, near zero means there is room to push. Next knobs, in order:
// LMR_FIRST_REDUCED to 3, then scaling the reduction by log(depth) * log(move index) instead of
// the flat 1-or-2 here.

// TODO the table sizing is stale: 64/128/256 MB were measured on the pre-LMR tree and 256 won.
// This tree is 9x smaller and fills only 6.5% of it, so most of that table is cold memory pushing
// the working set out of cache. Re-measure 64 and 128 against the new baseline, and check what
// DEFAULT_MEGABYTES is actually set to before reading anything into a benchmark.

// a reduced search needs plies left to be worth anything, and the shallow nodes are cheap anyway
const LMR_MIN_DEPTH: u32 = 3;

// the moves the ordering is confident about, searched in full: the table move, the killers, the captures
const LMR_FIRST_REDUCED: u32 = 4;

// past here the move is late enough and the node deep enough to give up a second ply
const LMR_DEEP_DEPTH: u32 = 6;
const LMR_LATE_MOVE: u32 = 8;

// what a search is allowed to spend: plies, wall time, or both
#[derive(Clone, Copy)]
pub struct SearchLimits {
    // the deepest pass the deepening may run; a ceiling only, under a deadline
    pub max_depth: u32,
    // when set, the search stops as soon as this passes and keeps the last finished pass
    pub deadline: Option<Instant>,
}

impl SearchLimits {
    // deepen to exactly this depth, however long it takes
    pub fn depth(max_depth: u32) -> SearchLimits {
        SearchLimits {
            max_depth,
            deadline: None,
        }
    }

    // deepen for as long as `budget` allows, up to MAX_SEARCH_DEPTH
    pub fn timed(budget: Duration) -> SearchLimits {
        SearchLimits {
            max_depth: MAX_SEARCH_DEPTH,
            deadline: Some(Instant::now() + budget),
        }
    }
}

pub struct SearchResult {
    // the deepest pass that finished, which is what best_move and score come from
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
    // whether the deadline cut the search short, rather than it reaching the depth asked for
    pub aborted: bool,
    // what each pass of the deepening found, in order
    pub passes: Vec<DepthPass>,
    // beta cutoffs, and how many of those a killer caused - measures if killers pay for themselves
    pub beta_cutoffs: u64,
    pub killer_cutoffs: u64,
    // cutoffs where the first move tried was already the one: the share says how good the ordering is
    pub first_move_cutoffs: u64,
    // moves searched at reduced depth, and how many of those had to be searched again anyway
    pub lmr_reductions: u64,
    pub lmr_researches: u64,
}

// one pass of the deepening, as it stood when that pass finished
#[derive(Clone, Copy)]
pub struct DepthPass {
    pub depth: u32,
    pub best_move: Option<Move>,
    pub score: i32,
    // every node of the search so far, this pass and all the shallower ones before it
    pub positions_searched: u64,
    // measured the same way: from the start of the search, not of this pass
    pub elapsed: Duration,
}

// the opening book and transposition table, which outlive a single search
pub struct Search {
    table: TranspositionTable,
    // asked before any searching, and None for a caller that wants the search itself
    book: Option<OpeningBook>,
    // the nodes of the search that is running, counted from its root
    positions_searched: u64,
    positions_searched_quiescience: u64,
    // move lists on loan to a node and returned when it's done; depth first, so never more than the tree is deep
    orders: Vec<MoveOrder>,
    // quiet moves that beat beta at each ply, newest first - a refutation of one sibling usually refutes the next
    killers: [[Option<Move>; KILLERS_PER_PLY]; MAX_PLY],
    beta_cutoffs: u64,
    killer_cutoffs: u64,
    first_move_cutoffs: u64,
    lmr_reductions: u64,
    lmr_researches: u64,
    // when the search that is running must stop; None for a search bounded only by depth
    deadline: Option<Instant>,
    // set once the deadline passes: every node then returns at once, and the pass it
    // interrupted is thrown away rather than believed
    aborted: bool,
}

impl Search {
    pub fn new(table_megabytes: usize) -> Search {
        Search {
            table: TranspositionTable::new(table_megabytes),
            book: Some(OpeningBook::new()),
            positions_searched: 0,
            positions_searched_quiescience: 0,
            orders: Vec::new(),
            killers: [NO_KILLERS; MAX_PLY],
            beta_cutoffs: 0,
            killer_cutoffs: 0,
            first_move_cutoffs: 0,
            lmr_reductions: 0,
            lmr_researches: 0,
            deadline: None,
            aborted: false,
        }
    }

    // searches every position, opening or not - used for analysis, not play
    pub fn without_book(table_megabytes: usize) -> Search {
        Search {
            book: None,
            ..Search::new(table_megabytes)
        }
    }

    // the best move for the side to move, iteratively deepened to `depth` plies - each
    // shallower pass costs little and hands the next one a move to order first
    pub fn find_best_move(&mut self, board: &mut Board, depth: u32) -> SearchResult {
        self.find_best_move_limited(board, &SearchLimits::depth(depth))
    }

    // the same search, told what it may spend rather than only how deep to go. Under a
    // deadline it deepens until the clock runs out and answers with the last pass that
    // finished - the interrupted one is discarded, since its scores are unfinished
    pub fn find_best_move_limited(
        &mut self,
        board: &mut Board,
        limits: &SearchLimits,
    ) -> SearchResult {
        let depth = limits.max_depth;

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
                aborted: false,
                passes: Vec::new(),
                beta_cutoffs: 0,
                killer_cutoffs: 0,
                first_move_cutoffs: 0,
                lmr_reductions: 0,
                lmr_researches: 0,
            };
        }

        self.table.start_search();
        self.positions_searched = 1;
        self.positions_searched_quiescience = 0;
        self.beta_cutoffs = 0;
        self.killer_cutoffs = 0;
        self.first_move_cutoffs = 0;
        self.lmr_reductions = 0;
        self.lmr_researches = 0;
        // killers are ply-relative to this search; a move on the board shifts every ply along
        self.killers = [NO_KILLERS; MAX_PLY];
        // the first pass runs to the end whatever the clock says, so a search started with
        // almost no time left still comes back with a move rather than with nothing
        self.deadline = None;
        self.aborted = false;

        let started = Instant::now();
        let mut best_move = None;
        let mut score = 0;
        let mut reached = 0;
        let mut passes = Vec::with_capacity(depth as usize);

        // one legal move is no choice at all, so a budget spent confirming it is spent on
        // nothing. Only under a clock: a search asked for a depth was asked for that depth
        let forced = limits.deadline.is_some() && board.legal_moves().len() == 1;

        // depth 0 wants the position scored as it stands - quiescence, nothing to deepen
        if depth == 0 {
            score = if board.legal_moves().is_empty() {
                terminal_score(board, 0)
            } else {
                self.quiescence(board, -INFINITY, INFINITY, 0)
            };
        } else {
            for current in 1..=depth {
                // no point starting a pass with the clock already out: it could only be
                // interrupted, and an interrupted pass is thrown away
                if self.out_of_time() {
                    self.aborted = true;
                    break;
                }

                let (pass_move, pass_score) = self.search_root(board, current, best_move);

                // this pass never finished, so its move and score are half-searched - keep
                // what the pass before it found instead
                if self.aborted {
                    break;
                }

                best_move = pass_move;
                score = pass_score;
                reached = current;

                passes.push(DepthPass {
                    depth: current,
                    best_move,
                    score: pass_score,
                    positions_searched: self.positions_searched,
                    elapsed: started.elapsed(),
                });

                // the game is over on the board - nothing left for a deeper pass to look at
                if best_move.is_none() {
                    break;
                }

                // the move is forced: no depth can change what gets played
                if forced {
                    break;
                }

                // a mate is proved, not estimated - no deeper pass finds a faster one
                if pass_score.abs() >= MATE_BOUND {
                    break;
                }

                // the first pass is in, so from here the clock is allowed to interrupt
                self.deadline = limits.deadline;
            }
        }

        self.deadline = None;

        SearchResult {
            depth: reached,
            best_move,
            score,
            positions_searched: self.positions_searched,
            positions_searched_quiescience: self.positions_searched_quiescience,
            table_cutoffs: self.table.cutoffs(),
            table_fill: self.table.fill(),
            from_book: false,
            aborted: self.aborted,
            passes,
            beta_cutoffs: self.beta_cutoffs,
            killer_cutoffs: self.killer_cutoffs,
            first_move_cutoffs: self.first_move_cutoffs,
            lmr_reductions: self.lmr_reductions,
            lmr_researches: self.lmr_researches,
        }
    }

    // whether the deadline has passed; false for a search that was given none
    fn out_of_time(&self) -> bool {
        match self.deadline {
            Some(deadline) => Instant::now() >= deadline,
            None => false,
        }
    }

    // asked once a node: reads the clock only every DEADLINE_CHECK_INTERVAL nodes, and once
    // the answer is yes it stays yes for the rest of the search
    fn should_stop(&mut self) -> bool {
        if self.aborted {
            return true;
        }
        if self.deadline.is_some()
            && self.positions_searched % DEADLINE_CHECK_INTERVAL == 0
            && self.out_of_time()
        {
            self.aborted = true;
        }
        self.aborted
    }

    // the killers of a ply, or none at all past the depth they are kept for
    fn killers_at(&self, ply: u32) -> [Option<Move>; KILLERS_PER_PLY] {
        match self.killers.get(ply as usize) {
            Some(killers) => *killers,
            None => NO_KILLERS,
        }
    }

    // a quiet move that beat beta, remembered for the next node at this ply - captures
    // don't need a slot, and a move already first shouldn't push itself into second
    fn remember_killer(&mut self, chess_move: Move, ply: u32) {
        if chess_move.captured.is_some() || chess_move.promotion.is_some() {
            return;
        }

        let Some(killers) = self.killers.get_mut(ply as usize) else {
            return;
        };
        if killers[0] == Some(chess_move) {
            return;
        }

        killers[1] = killers[0];
        killers[0] = Some(chess_move);
    }

    // a move list to work in, the one the last node finished with where there is one
    fn take_order(&mut self) -> MoveOrder {
        self.orders.pop().unwrap_or_else(MoveOrder::new)
    }

    // handed back on every way out of a node, cutoffs included
    fn give_back(&mut self, order: MoveOrder) {
        self.orders.push(order);
    }

    // like alpha_beta with the window wide open, but must return a move. `previous_best` is
    // what the previous pass played - the table usually holds it too, but a collision can lose it
    fn search_root(
        &mut self,
        board: &mut Board,
        depth: u32,
        previous_best: Option<Move>,
    ) -> (Option<Move>, i32) {
        // failing that, still usually the answer to what the opponent just did
        let table_move = previous_best.or_else(|| self.table.best_move(board.hash()));

        let mut order = self.take_order();
        // the root has no killers: nothing above it ever cut off, so nothing was stored
        order.load(board, table_move, NO_KILLERS);

        // nothing to search: the game is over
        if order.moves.is_empty() {
            self.give_back(order);
            return (None, terminal_score(board, 0));
        }

        let mut best_move = None;
        // whether best_move repeats a position already on the board - only breaks ties
        let mut best_repeats = false;
        let mut alpha = -INFINITY;

        while let Some(chess_move) = order.next() {
            board.make_move(&chess_move);
            let score = -self.alpha_beta(board, depth - 1, -INFINITY, -alpha, 1);
            let repeats = board.position_repetitions() > 1;
            board.undo_move();

            // the clock ran out under this move, so its score is unfinished - the caller
            // throws this whole pass away, and nothing here is worth filing
            if self.aborted {
                self.give_back(order);
                return (best_move, alpha);
            }

            // among equally scored moves, prefer the one that doesn't repeat
            let better =
                best_move.is_none() || score > alpha || (score == alpha && best_repeats && !repeats);

            if better {
                alpha = score;
                best_move = Some(chess_move);
                best_repeats = repeats;
            }
        }

        self.give_back(order);

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

        // out of time: unwind without searching, and without filing anything on the way
        if self.should_stop() {
            return alpha;
        }

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
        let killers = self.killers_at(ply);
        let mut order = self.take_order();
        order.load(board, probe.best_move, killers);

        if order.moves.is_empty() {
            self.give_back(order);
            return terminal_score(board, ply);
        }

        // a mate on the last ply of the fifty still counts as mate, not a draw
        if board.is_fifty_move_draw() {
            self.give_back(order);
            return 0;
        }

        // until a move beats alpha, this node is worth no more than alpha
        let mut node_type = NodeType::UpperBound;
        let mut best_move = None;
        let mut tried = 0;
        // asked for at most once a node, and only where a move gets late enough to be reduced
        // let mut in_check = None;

        while let Some(chess_move) = order.next() {
            tried += 1;

            // LMR disabled for now, unverified - see TODO above. Kept, not removed.
            // // what the ordering is confident about is searched in full: the early moves, anything
            // // that takes or promotes, the killers, and every move of a node that is under check
            // let late = depth >= LMR_MIN_DEPTH
            //     && tried >= LMR_FIRST_REDUCED
            //     && chess_move.captured.is_none()
            //     && chess_move.promotion.is_none()
            //     && !killers.contains(&Some(chess_move))
            //     && !*in_check.get_or_insert_with(|| board.is_check(board.turn()));

            board.make_move(&chess_move);

            // a move that gives check forces the replies, so its subtree is small enough to keep whole
            // let reduction = if late && !board.is_check(board.turn()) {
            //     self.lmr_reductions += 1;
            //     if depth >= LMR_DEEP_DEPTH && tried >= LMR_LATE_MOVE {
            //         2
            //     } else {
            //         1
            //     }
            // } else {
            //     0
            // };
            let reduction = 0;

            // a reduced move only has to fail low, so it's asked the cheapest question there is
            let mut score = if reduction > 0 {
                -self.alpha_beta(board, depth - 1 - reduction, -alpha - 1, -alpha, ply + 1)
            } else {
                -self.alpha_beta(board, depth - 1, -beta, -alpha, ply + 1)
            };

            // it beat alpha anyway, so the guess that it was weak was wrong - search it properly
            if reduction > 0 && score > alpha {
                self.lmr_researches += 1;
                score = -self.alpha_beta(board, depth - 1, -beta, -alpha, ply + 1);
            }

            board.undo_move();

            // this move's score never finished, so it can neither raise alpha nor be filed
            if self.aborted {
                self.give_back(order);
                return alpha;
            }

            if score >= beta {
                self.give_back(order);

                self.beta_cutoffs += 1;
                if tried == 1 {
                    self.first_move_cutoffs += 1;
                }
                if killers.contains(&Some(chess_move)) {
                    self.killer_cutoffs += 1;
                }
                self.remember_killer(chess_move, ply);

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

        self.give_back(order);

        self.table
            .store(board.hash(), depth, ply, alpha, node_type, best_move);

        alpha
    }

    // captures only, until nothing is hanging, then evaluate; not filed in the table since
    // these scores stop at a quiet position rather than at a depth
    fn quiescence(&mut self, board: &mut Board, mut alpha: i32, beta: i32, ply: u32) -> i32 {
        self.positions_searched += 1;
        self.positions_searched_quiescience +=1;

        // out of time: unwind without searching. Nothing is filed here anyway
        if self.should_stop() {
            return alpha;
        }

        // no standing pat out of check, and every evasion counts, not only captures
        if board.is_check(board.turn()) {
            // ordered like any node - an evasion is as often a block as a capture. Never
            // pruned either: drop a move here and a mate could read as a score
            let killers = self.killers_at(ply);
            let mut order = self.take_order();
            order.load(board, None, killers);

            if order.moves.is_empty() {
                self.give_back(order);
                return terminal_score(board, ply);
            }

            while let Some(chess_move) = order.next() {
                board.make_move(&chess_move);
                let score = -self.quiescence(board, -beta, -alpha, ply + 1);
                board.undo_move();

                if self.aborted {
                    self.give_back(order);
                    return alpha;
                }

                if score >= beta {
                    self.give_back(order);
                    return beta;
                }
                if score > alpha {
                    alpha = score;
                }
            }

            self.give_back(order);
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

        let mut order = self.take_order();
        order.load_captures(board);

        while let Some(chess_move) = order.next() {
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

            if self.aborted {
                self.give_back(order);
                return alpha;
            }

            if score >= beta {
                self.give_back(order);
                return beta;
            }
            if score > alpha {
                alpha = score;
            }
        }

        self.give_back(order);
        alpha
    }
}

// the moves of one node, handed out best first: a selection sort that stops when the
// caller stops asking, since most nodes cut off after a move or two
struct MoveOrder {
    moves: Vec<Move>,
    // parallel to `moves`, swapped alongside it; a fixed array since this is rebuilt every node
    scores: [i32; MAX_MOVES],
    handed_out: usize,
}

impl MoveOrder {
    // an empty one, to be filled by one of the two loads below
    fn new() -> MoveOrder {
        MoveOrder {
            moves: Vec::with_capacity(48),
            scores: [0; MAX_MOVES],
            handed_out: 0,
        }
    }

    // scored move list with the table's move (if any) in front; filled in place, no allocation
    fn load(
        &mut self,
        board: &Board,
        table_move: Option<Move>,
        killers: [Option<Move>; KILLERS_PER_PLY],
    ) {
        board.legal_moves_into(&mut self.moves);

        for (index, chess_move) in self.moves.iter().enumerate() {
            self.scores[index] = if Some(*chess_move) == table_move {
                TABLE_MOVE_SCORE
            } else {
                move_score(board, chess_move, killers)
            };
        }

        self.handed_out = 0;
    }

    // what quiescence walks: captures, with nothing but MVV-LVA to tell them apart
    fn load_captures(&mut self, board: &Board) {
        board.legal_captures_into(&mut self.moves);

        for (index, chess_move) in self.moves.iter().enumerate() {
            self.scores[index] = capture_score(chess_move);
        }

        self.handed_out = 0;
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
fn move_score(
    board: &Board,
    chess_move: &Move,
    killers: [Option<Move>; KILLERS_PER_PLY],
) -> i32 {
    // a killer is quiet, so nothing below has anything to say about it
    for (slot, killer) in killers.iter().enumerate() {
        if *killer == Some(*chess_move) {
            return KILLER_SCORES[slot];
        }
    }

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

// mate or stalemate; a mate is worth a little less the deeper it is, favoring the shortest line
fn terminal_score(board: &Board, ply: u32) -> i32 {
    if board.is_check(board.turn()) {
        -MATE + ply as i32
    } else {
        0
    }
}

// MVV-LVA: the victim weighs eight times the attacker, so what's taken decides order
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

// zero once the endgame is reached, where a capture short of alpha today is tomorrow's passed pawn
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

// the cheapest piece of `color` that can take on `square`; pieces the exchange took off are gone
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
/*
    // forward pruning may shade the score, so the move it settles on is what has to hold up
    #[test]
    fn pruning_does_not_pick_a_worse_move_in_a_tactical_position() {
        let mut board = queen_against_pawns();

        for depth in 1..=4 {
            let searched = find_best_move(&mut board, depth);
            let best = searched.best_move.expect("white has moves");
            let (_, reference) = negamax_best(&mut search(), &mut board, depth);

            board.make_move(&best);
            let played = -negamax(&mut search(), &mut board, depth - 1, 1);
            board.undo_move();

            assert_eq!(played, reference, "at depth {depth}");
        }
    }

 */

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

    // the depth asked for is arrived at one ply at a time, and every pass is on record
    #[test]
    fn the_search_deepens_one_ply_at_a_time() {
        let mut board = start_position();

        let result = find_best_move(&mut board, 4);

        assert_eq!(result.depth, 4);
        assert_eq!(result.passes.len(), 4);

        let mut nodes = 0;
        for (index, pass) in result.passes.iter().enumerate() {
            assert_eq!(pass.depth, index as u32 + 1, "the passes skipped a depth");
            assert!(pass.best_move.is_some(), "a pass came back with no move");
            // the count is the whole search so far, so it only ever grows
            assert!(pass.positions_searched >= nodes, "at depth {}", pass.depth);
            nodes = pass.positions_searched;
        }

        // and what the last pass found is what the search as a whole answers
        let last = result.passes.last().expect("four passes");
        assert_eq!(last.best_move, result.best_move);
        assert_eq!(last.score, result.score);
        assert_eq!(nodes, result.positions_searched);
    }

    // a mate is proved rather than estimated, so the passes after it have nothing left
    // to find - the search stops short of the depth it was asked for
    #[test]
    fn a_proved_mate_stops_the_deepening() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        board.add_piece(Piece::new(PieceType::King, Color::Black), 63); // h8
        board.add_piece(Piece::new(PieceType::Rook, Color::White), 48); // a7
        board.add_piece(Piece::new(PieceType::Rook, Color::White), 1); // b1

        let result = find_best_move(&mut board, 5);

        assert_eq!(result.score, MATE - 1);
        assert_eq!(result.depth, 1, "the deepening carried on past a mate in one");
        assert_eq!(result.passes.len(), 1);
    }

    // deepening changes the order the work is done in, not the answer that comes out
    #[test]
    fn deepening_scores_a_position_as_a_single_pass_does() {
        let mut board = queen_against_pawns();

        for depth in 1..=4 {
            let deepened = find_best_move(&mut board, depth);
            let (_, single) = search().search_root(&mut board, depth, None);

            assert_eq!(deepened.score, single, "at depth {depth}");
        }
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

        let mut order = MoveOrder::new();
        order.load(&board, None, NO_KILLERS);
        let ordered: Vec<Move> = order.collect();

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

        let mut order = MoveOrder::new();
        order.load(&board, None, NO_KILLERS);
        let ordered: Vec<Move> = order.collect();

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
            .map(|chess_move| move_score(&board, chess_move, NO_KILLERS))
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

        let mut order = MoveOrder::new();
        order.load(&board, Some(table_move), NO_KILLERS);
        let ordered: Vec<Move> = order.collect();

        assert_eq!(ordered[0], table_move, "the stored move was not searched first");
    }

    // a killer goes ahead of the quiet moves it is one of, and stays behind the captures
    #[test]
    fn a_killer_is_handed_out_after_the_captures_and_before_the_quiet_moves() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        board.add_piece(Piece::new(PieceType::King, Color::Black), 63); // h8
        board.add_piece(Piece::new(PieceType::Rook, Color::White), 0); // a1
        board.add_piece(Piece::new(PieceType::Queen, Color::Black), 56); // a8
        board.add_piece(Piece::new(PieceType::Knight, Color::White), 6); // g1

        // Ng1f3, which nothing about the position itself recommends
        let moves = board.legal_moves();
        let killer = *moves
            .iter()
            .find(|chess_move| (chess_move.from, chess_move.to) == (6, 21))
            .expect("Ng1f3 is legal");

        let mut order = MoveOrder::new();
        order.load(&board, None, [Some(killer), None]);
        let ordered: Vec<Move> = order.collect();

        // Rxa8 takes a queen, so it still leads
        let first = ordered.first().expect("white has moves");
        assert_eq!((first.from, first.to), (0, 56), "the capture lost its place");
        assert_eq!(ordered[1], killer, "the killer was not second");
    }

    // the killers are of the ply, so what one node found is there for the next one
    #[test]
    fn a_quiet_move_that_beats_beta_is_kept_as_a_killer() {
        let mut engine = search();
        let mut board = start_position();

        engine.find_best_move(&mut board, 4);

        let kept: usize = engine
            .killers
            .iter()
            .flatten()
            .filter(|killer| killer.is_some())
            .count();

        assert!(kept > 0, "no quiet cutoff was remembered in a whole search");
        // a killer is a quiet move: a capture is ordered by what it takes instead
        for killer in engine.killers.iter().flatten().flatten() {
            assert!(killer.captured.is_none(), "{killer:?} takes something");
            assert!(killer.promotion.is_none(), "{killer:?} promotes");
        }
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

    // a depth limit goes through the same code as before, so it must answer the same
    #[test]
    fn a_depth_limit_searches_what_a_depth_did() {
        let mut board = queen_against_pawns();

        let plain = find_best_move(&mut board, 4);
        let limited = search().find_best_move_limited(&mut board, &SearchLimits::depth(4));

        assert_eq!(limited.best_move, plain.best_move);
        assert_eq!(limited.score, plain.score);
        assert_eq!(limited.depth, plain.depth);
        assert!(!limited.aborted, "an untimed search stopped on a clock");
    }

    // the point of the whole thing: it spends what it was given and not much more
    #[test]
    fn a_timed_search_stops_near_its_deadline() {
        let mut board = queen_against_pawns();
        let budget = Duration::from_millis(300);

        let started = Instant::now();
        let result = search().find_best_move_limited(&mut board, &SearchLimits::timed(budget));
        let elapsed = started.elapsed();

        assert!(result.best_move.is_some(), "a timed search found no move");
        assert!(result.depth >= 1, "not even one pass finished");
        // generous: a debug build is slow, and the last node of a pass still has to unwind
        assert!(
            elapsed < budget * 4,
            "a {budget:?} search ran for {elapsed:?}"
        );
    }

    // and with no time at all it still has to come back with something playable: the
    // first pass runs to the end whatever the clock says
    #[test]
    fn a_search_given_no_time_still_returns_a_move() {
        let mut board = queen_against_pawns();

        let result =
            search().find_best_move_limited(&mut board, &SearchLimits::timed(Duration::ZERO));

        let best = result.best_move.expect("no move came back at all");
        assert!(board.legal_moves().contains(&best), "{best:?} is not legal");
        assert_eq!(result.depth, 1, "more than the first pass was kept");
    }

    // a budget is what a search may spend, not what it has to: with one legal move there
    // is nothing to spend it on, and the move comes back at once
    #[test]
    fn a_forced_move_is_played_without_thinking() {
        // the rook on a8 checks along the a-file and the one on b2 covers b1, so taking
        // that second rook is the only move white has
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 0); // a1
        board.add_piece(Piece::new(PieceType::King, Color::Black), 62); // g8
        board.add_piece(Piece::new(PieceType::Rook, Color::Black), 56); // a8
        board.add_piece(Piece::new(PieceType::Rook, Color::Black), 9); // b2

        assert_eq!(board.legal_moves().len(), 1, "the position is not forced");

        let started = Instant::now();
        let result =
            search().find_best_move_limited(&mut board, &SearchLimits::timed(Duration::from_secs(5)));
        let elapsed = started.elapsed();

        assert!(result.best_move.is_some(), "the forced move was not found");
        assert!(
            elapsed < Duration::from_secs(1),
            "a forced move took {elapsed:?} of a 5s budget"
        );
    }

    // an interrupted pass is thrown away, so what comes back is a finished pass - a move
    // that is still legal, and a score that is not some half-searched -INFINITY
    #[test]
    fn an_interrupted_search_answers_from_a_finished_pass() {
        let mut board = start_position();
        let mut engine = search();

        let result =
            engine.find_best_move_limited(&mut board, &SearchLimits::timed(Duration::from_millis(200)));

        let best = result.best_move.expect("no move came back at all");
        assert!(board.legal_moves().contains(&best), "{best:?} is not legal");
        assert!(result.score.abs() < INFINITY, "an unfinished score was kept");
        assert_eq!(
            result.depth as usize,
            result.passes.len(),
            "a pass was recorded that the result did not come from"
        );
    }
}
