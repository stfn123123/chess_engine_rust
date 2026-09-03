// The board: everything that is only about the position and the moves it allows.
//
// Nothing in here knows about searching, evaluating or drawing - it answers what
// stands where, which moves are legal, and how a move is played and taken back.
//
// board      - the position, playing a move and taking it back
// movegen    - which moves a position allows, and the attack scan
// piece      - the pieces and the two sides
// square     - square numbering and the geometry the generators walk
// castling   - castling rights and the squares castling involves
// chess_move - a move, and what is needed to undo it
// zobrist    - the keys the position hash is built from

pub mod board;
pub mod castling;
pub mod chess_move;
pub mod movegen;
pub mod piece;
pub mod square;
pub mod zobrist;

// the one type the rest of the program works with, so callers write `board::Board`
// instead of `board::board::Board`
pub use self::board::Board;
