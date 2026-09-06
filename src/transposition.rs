// The transposition table.
//
// A position turns up over and over in one search: the moves that lead to it can be
// played in any order, so every ordering of them arrives at the same place, and the
// search walks each of those orderings. The table remembers what searching a position
// found, filed under its zobrist key, so the next time the position shows up the work
// is already done.
//
// A score on its own would not be enough to file. Alpha-beta stops a node as soon as
// the node is proven to lie outside the window it was asked about, so most scores are
// not what the position is worth but only a bound on it. The node type is what says
// which of the two a score is, and a bound may only be reused where it still settles
// the question - a floor answers a node whose beta it clears, a ceiling one whose
// alpha it fails to reach, and neither answers anything in between.
//
// The table is a fixed array of slots and the key picks the slot, so two positions
// that land on the same one cannot both be kept. The full key is stored alongside the
// entry, which is what tells a hit apart from a position that merely landed here too.

use crate::board::chess_move::Move;
use crate::evaluate::MATE;

// a score at least this far up is a mate rather than a count of material
const MATE_BOUND: i32 = MATE - 1_000;

// what a stored score says about the position
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeType {
    // every move was searched and one of them came out inside the window: the score is
    // what the position is worth
    Exact,
    // a move beat beta and the rest were never looked at, so the position is worth at
    // least this - it could be worth more
    LowerBound,
    // no move reached alpha, so the position is worth at most this
    UpperBound,
}

// one stored position
#[derive(Clone, Copy)]
struct Entry {
    // the whole key, not just the bits that picked the slot - the rest is what tells
    // this position apart from another one filed in the same place
    key: u64,
    // the move that was best here, or the one that caused the cutoff; kept even where
    // the score cannot be used, since it is still the first move worth trying
    best_move: Option<Move>,
    score: i32,
    // how deep the search that produced the score was: a shallower one proves less
    depth: u8,
    node_type: NodeType,
    // which search wrote this, so entries left over from an earlier one can be given up
    generation: u8,
}

// what the table has to say about a position
pub struct Probe {
    // the move to try first, whenever the table has seen this position at all
    pub best_move: Option<Move>,
    // the score, when what is stored settles the node for the window that is asking
    pub cutoff: Option<i32>,
}

pub struct TranspositionTable {
    slots: Vec<Option<Entry>>,
    // slot count minus one: the count is a power of two, so this masks a key down to
    // an index without a division
    mask: usize,
    generation: u8,
    // how many slots have ever been written, for the fill figure
    filled: usize,
    // how many probes of the search that is running answered a node outright
    cutoffs: u64,
}

impl TranspositionTable {
    // how much memory the table takes unless something asks for another size
    pub const DEFAULT_MEGABYTES: usize = 64;

    pub fn new(megabytes: usize) -> TranspositionTable {
        let slot_size = std::mem::size_of::<Option<Entry>>();
        let wanted = (megabytes.max(1) * 1024 * 1024 / slot_size).max(1);
        // rounded down to a power of two, so a key masks straight into an index
        let slots = 1usize << (usize::BITS - 1 - wanted.leading_zeros());

        TranspositionTable {
            slots: vec![None; slots],
            mask: slots - 1,
            generation: 0,
            filled: 0,
            cutoffs: 0,
        }
    }

    // a new search is starting: what is in the table stays, since the positions are
    // still the same positions, but it is now a search old
    pub fn start_search(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.cutoffs = 0;
    }

    // the move stored for this position, whatever depth it was found at - what the
    // root wants, which needs a move to play and cannot be answered by a score
    pub fn best_move(&self, hash: u64) -> Option<Move> {
        self.entry(hash).and_then(|entry| entry.best_move)
    }

    // what the table can do for a node searching `depth` plies with this window
    pub fn probe(&mut self, hash: u64, depth: u32, ply: u32, alpha: i32, beta: i32) -> Probe {
        let Some(entry) = self.entry(hash) else {
            return Probe {
                best_move: None,
                cutoff: None,
            };
        };

        let best_move = entry.best_move;

        // a shallower search looked at less than this node has been asked to, so its
        // score answers nothing here - its move is still the best guess going
        if u32::from(entry.depth) < depth {
            return Probe {
                best_move,
                cutoff: None,
            };
        }

        let score = from_table(entry.score, ply);
        let settles = match entry.node_type {
            NodeType::Exact => true,
            NodeType::LowerBound => score >= beta,
            NodeType::UpperBound => score <= alpha,
        };

        if !settles {
            return Probe {
                best_move,
                cutoff: None,
            };
        }

        self.cutoffs += 1;
        Probe {
            best_move,
            cutoff: Some(score),
        }
    }

    pub fn store(
        &mut self,
        hash: u64,
        depth: u32,
        ply: u32,
        score: i32,
        node_type: NodeType,
        best_move: Option<Move>,
    ) {
        let slot = hash as usize & self.mask;

        match self.slots[slot] {
            None => self.filled += 1,
            // an entry from an earlier search is given up freely; within one search the
            // deeper of the two stays, since it cost the most and settles the most
            Some(existing) => {
                if existing.generation == self.generation && u32::from(existing.depth) > depth {
                    return;
                }
            }
        }

        self.slots[slot] = Some(Entry {
            key: hash,
            best_move,
            score: to_table(score, ply),
            depth: depth.min(u32::from(u8::MAX)) as u8,
            node_type,
            generation: self.generation,
        });
    }

    // how many nodes of the search that is running the table answered by itself
    pub fn cutoffs(&self) -> u64 {
        self.cutoffs
    }

    // how much of the table has been written, 0.0 to 1.0
    pub fn fill(&self) -> f32 {
        self.filled as f32 / self.slots.len() as f32
    }

    // the entry filed here, once the full key says it is this position and not another
    // one that happened to land on the same slot
    fn entry(&self, hash: u64) -> Option<Entry> {
        self.slots[hash as usize & self.mask].filter(|entry| entry.key == hash)
    }
}

// A mate score counts from the node it was found at - "mate in three from here" - and
// the whole point of the table is that the entry gets read at some other node. So it
// is stored counted from the position and read back counted from the root, which is
// the frame the rest of the search works in.
fn to_table(score: i32, ply: u32) -> i32 {
    if score >= MATE_BOUND {
        score + ply as i32
    } else if score <= -MATE_BOUND {
        score - ply as i32
    } else {
        score
    }
}

fn from_table(score: i32, ply: u32) -> i32 {
    if score >= MATE_BOUND {
        score - ply as i32
    } else if score <= -MATE_BOUND {
        score + ply as i32
    } else {
        score
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::piece::{Color, Piece, PieceType};

    // a table small enough to make one per test
    fn table() -> TranspositionTable {
        TranspositionTable::new(1)
    }

    fn a_move() -> Move {
        Move::normal(4, 12, Piece::new(PieceType::King, Color::White), None)
    }

    // the whole window, for probes that are only asking what is stored
    const WIDE: i32 = 1_000_000;

    #[test]
    fn an_exact_score_comes_back_as_it_went_in() {
        let mut table = table();
        table.store(0x1234, 4, 0, 55, NodeType::Exact, Some(a_move()));

        let probe = table.probe(0x1234, 4, 0, -WIDE, WIDE);

        assert_eq!(probe.cutoff, Some(55));
        assert_eq!(probe.best_move, Some(a_move()));
    }

    // two positions can land on the same slot, and the key is what tells them apart -
    // 1 << 40 is above any mask a table this size could have, so both pick slot 0
    #[test]
    fn a_position_does_not_answer_for_another_one_in_its_slot() {
        let mut table = table();
        table.store(0, 4, 0, 55, NodeType::Exact, Some(a_move()));

        let probe = table.probe(1 << 40, 4, 0, -WIDE, WIDE);

        assert_eq!(probe.cutoff, None);
        assert_eq!(probe.best_move, None);
    }

    // a shallower search proves less than this node needs, but it still had an opinion
    // about which move to try
    #[test]
    fn a_shallower_entry_gives_up_its_move_and_not_its_score() {
        let mut table = table();
        table.store(0x1234, 2, 0, 55, NodeType::Exact, Some(a_move()));

        let probe = table.probe(0x1234, 5, 0, -WIDE, WIDE);

        assert_eq!(probe.cutoff, None);
        assert_eq!(probe.best_move, Some(a_move()));
    }

    // a floor answers a node it lifts over beta, and says nothing about one it does not
    #[test]
    fn a_floor_only_answers_the_windows_it_clears() {
        let mut table = table();
        table.store(0x1234, 4, 0, 55, NodeType::LowerBound, None);

        assert_eq!(table.probe(0x1234, 4, 0, 0, 50).cutoff, Some(55));
        assert_eq!(table.probe(0x1234, 4, 0, 0, 90).cutoff, None);
    }

    // and a ceiling answers a node it keeps below alpha
    #[test]
    fn a_ceiling_only_answers_the_windows_it_stays_under() {
        let mut table = table();
        table.store(0x1234, 4, 0, 55, NodeType::UpperBound, None);

        assert_eq!(table.probe(0x1234, 4, 0, 60, 100).cutoff, Some(55));
        assert_eq!(table.probe(0x1234, 4, 0, 20, 100).cutoff, None);
    }

    // stored three plies down as a mate in two from there, read one ply down: still
    // the same mate, now four plies away
    #[test]
    fn a_mate_is_read_back_from_the_ply_that_asks() {
        let mut table = table();
        table.store(0x1234, 4, 3, MATE - 5, NodeType::Exact, None);

        assert_eq!(table.probe(0x1234, 4, 1, -WIDE, WIDE).cutoff, Some(MATE - 3));
        assert_eq!(table.probe(0x1234, 4, 3, -WIDE, WIDE).cutoff, Some(MATE - 5));
    }

    // the deeper search of the two cost more and proves more, so it is the one kept
    #[test]
    fn a_shallower_search_does_not_push_out_a_deeper_one() {
        let mut table = table();
        table.store(0x1234, 6, 0, 55, NodeType::Exact, None);
        table.store(0x1234, 2, 0, 12, NodeType::Exact, None);

        assert_eq!(table.probe(0x1234, 6, 0, -WIDE, WIDE).cutoff, Some(55));
    }

    // but only within one search: once the next one starts, the slot is free again
    #[test]
    fn the_next_search_may_write_over_what_this_one_stored() {
        let mut table = table();
        table.store(0x1234, 6, 0, 55, NodeType::Exact, None);

        table.start_search();
        table.store(0x1234, 2, 0, 12, NodeType::Exact, None);

        assert_eq!(table.probe(0x1234, 2, 0, -WIDE, WIDE).cutoff, Some(12));
    }

    // the counters the panel reads: a slot written once is a slot written, and only
    // the probes that answered a node count as cutoffs
    #[test]
    fn the_counters_follow_what_the_table_did() {
        let mut table = table();
        assert_eq!(table.fill(), 0.0);

        table.store(0x1234, 4, 0, 55, NodeType::Exact, None);
        table.probe(0x1234, 4, 0, -WIDE, WIDE);
        table.probe(0x5678, 4, 0, -WIDE, WIDE);

        assert!(table.fill() > 0.0);
        assert_eq!(table.cutoffs(), 1);

        table.start_search();
        assert_eq!(table.cutoffs(), 0);
    }
}
