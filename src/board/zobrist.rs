// Zobrist keys.
//
// One random number per (piece, square), plus one for black to move, one per
// castling side and one per en passant file. The hash of a position is all of
// them xored together, which is why a move only has to xor the few keys that
// actually changed - and why undoing a move is the very same xor again.
//
// The piece table is indexed piece first: a move keeps one piece and changes its
// square, so both of its keys sit in the same 512 byte row, often the same cache
// line. Square first would put them at least a row apart on every single move.

use crate::board::castling::{CASTLE_FLAGS, CastlingRights};
use crate::board::piece::Piece;

pub struct Zobrist {
    pieces: [[u64; 64]; Piece::ZOBRIST_INDEX_COUNT],
    side_to_move: u64,
    castling: [u64; 4],
    en_passant_file: [u64; 8],
}

impl Zobrist {
    pub fn piece(&self, square: u8, piece: Piece) -> u64 {
        self.pieces[piece.zobrist_index()][square as usize]
    }

    // mixed in exactly while black is to move
    pub fn side_to_move(&self) -> u64 {
        self.side_to_move
    }

    pub fn en_passant_file(&self, file: u8) -> u64 {
        self.en_passant_file[file as usize]
    }

    // the keys of all castling sides that are still open
    pub fn castling(&self, castling_rights: CastlingRights) -> u64 {
        let mut hash = 0;
        for (index, (color, side)) in CASTLE_FLAGS.into_iter().enumerate() {
            if castling_rights.get(color, side) {
                hash ^= self.castling[index];
            }
        }
        hash
    }
}

// the keys have to be the same on every run, so they are built at compile time
pub static ZOBRIST: Zobrist = build_zobrist();

const ZOBRIST_SEED: u64 = 0x9E37_79B9_7F4A_7C15;

// xorshift64*, good enough for table keys and simple enough to run in a const fn
const fn next_state(state: u64) -> u64 {
    let mut state = state;
    state ^= state >> 12;
    state ^= state << 25;
    state ^= state >> 27;
    state
}

const fn random_from(state: u64) -> u64 {
    state.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

const fn build_zobrist() -> Zobrist {
    let mut pieces = [[0u64; 64]; Piece::ZOBRIST_INDEX_COUNT];
    let mut castling = [0u64; 4];
    let mut en_passant_file = [0u64; 8];
    let mut state = ZOBRIST_SEED;

    // the three rows no packed byte lands on are filled too - a random key there turns
    // a stray lookup into a failed hash assert instead of a silent xor by zero
    // const fn has no for loops, hence the while loops
    let mut piece = 0;
    while piece < Piece::ZOBRIST_INDEX_COUNT {
        let mut square = 0;
        while square < 64 {
            state = next_state(state);
            pieces[piece][square] = random_from(state);
            square += 1;
        }
        piece += 1;
    }

    let mut index = 0;
    while index < 4 {
        state = next_state(state);
        castling[index] = random_from(state);
        index += 1;
    }

    let mut file = 0;
    while file < 8 {
        state = next_state(state);
        en_passant_file[file] = random_from(state);
        file += 1;
    }

    state = next_state(state);
    let side_to_move = random_from(state);

    Zobrist {
        pieces,
        side_to_move,
        castling,
        en_passant_file,
    }
}
