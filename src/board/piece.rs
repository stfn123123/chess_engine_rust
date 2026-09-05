// The two sides and the pieces they own.
//
// A Piece is packed into a single byte: the low three bits are the piece type,
// the fourth is the color. That keeps a piece Copy-cheap and lets the byte be
// used directly as an index into the zobrist tables.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PieceType {
    King = 1,
    Pawn = 2,
    Knight = 3,
    Bishop = 4,
    Rook = 5,
    Queen = 6,
}

impl PieceType {
    // every type, in the order the move generator walks them
    pub const ALL: [PieceType; 6] = [
        PieceType::King,
        PieceType::Pawn,
        PieceType::Knight,
        PieceType::Bishop,
        PieceType::Rook,
        PieceType::Queen,
    ];

    // a pawn may become any of these
    pub const PROMOTION_CHOICES: [PieceType; 4] = [
        PieceType::Queen,
        PieceType::Rook,
        PieceType::Bishop,
        PieceType::Knight,
    ];

    // the type bits of a Piece back into a PieceType
    // only ever called on bits that came out of Piece::new, so 1.=6
    fn from_bits(bits: u8) -> PieceType {
        match bits {
            1 => PieceType::King,
            2 => PieceType::Pawn,
            3 => PieceType::Knight,
            4 => PieceType::Bishop,
            5 => PieceType::Rook,
            6 => PieceType::Queen,
            other => unreachable!("{other} is not a piece type"),
        }
    }

    // what this piece counts towards the game phase - kings and pawns count nothing,
    // since a board of nothing but pawns is an endgame
    pub const fn phase_weight(self) -> i32 {
        match self {
            PieceType::King | PieceType::Pawn => 0,
            PieceType::Knight | PieceType::Bishop => 1,
            PieceType::Rook => 2,
            PieceType::Queen => 4,
        }
    }

    // the usual algebraic letter, lowercase
    pub fn letter(self) -> char {
        match self {
            PieceType::King => 'k',
            PieceType::Pawn => 'p',
            PieceType::Knight => 'n',
            PieceType::Bishop => 'b',
            PieceType::Rook => 'r',
            PieceType::Queen => 'q',
        }
    }
}

// the discriminants are the color bit stored inside a Piece - white is 0 so that the
// packed byte stays small enough to index the zobrist table directly
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Color {
    White = 0,
    Black = 8,
}

impl Color {
    pub const BOTH: [Color; 2] = [Color::White, Color::Black];

    pub fn opponent(self) -> Color {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }

    // the two sides as the rows of a table with one entry per color
    pub fn index(self) -> usize {
        match self {
            Color::White => 0,
            Color::Black => 1,
        }
    }

    // the rank step a pawn of this color moves in
    pub fn pawn_direction(self) -> i8 {
        match self {
            Color::White => 1,
            Color::Black => -1,
        }
    }

    // the rank this side's pawns start on, the only one they may push twice from
    pub fn pawn_start_rank(self) -> u8 {
        match self {
            Color::White => 1,
            Color::Black => 6,
        }
    }

    // the rank a pawn of this color promotes on
    pub fn promotion_rank(self) -> u8 {
        match self {
            Color::White => 7,
            Color::Black => 0,
        }
    }

    // the rank a pawn of this color lands on when it captures en passant
    pub fn en_passant_rank(self) -> u8 {
        match self {
            Color::White => 5,
            Color::Black => 2,
        }
    }
}

const TYPE_MASK: u8 = 0b0111;
const COLOR_MASK: u8 = 0b1000;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Piece(u8);

impl Piece {
    // the biggest packed value is Queen | Black = 6 | 8 = 14, so a table indexed by the
    // raw byte needs 15 rows - three unused ones in exchange for no mapping step
    pub const ZOBRIST_INDEX_COUNT: usize = 15;

    pub const fn new(piece_type: PieceType, color: Color) -> Self {
        Piece(piece_type as u8 | color as u8)
    }

    pub fn piece_type(self) -> PieceType {
        PieceType::from_bits(self.0 & TYPE_MASK)
    }

    pub fn color(self) -> Color {
        if self.0 & COLOR_MASK == Color::White as u8 {
            Color::White
        } else {
            Color::Black
        }
    }

    pub fn is(self, piece_type: PieceType) -> bool {
        self.piece_type() == piece_type
    }

    // uppercase for white, lowercase for black, as in FEN
    pub fn symbol(self) -> char {
        let letter = self.piece_type().letter();
        match self.color() {
            Color::White => letter.to_ascii_uppercase(),
            Color::Black => letter,
        }
    }

    // the row this piece occupies in the zobrist piece table
    pub(crate) fn zobrist_index(self) -> usize {
        self.0 as usize
    }
}

// printed as its board symbol, so assert failures read like a position
impl std::fmt::Debug for Piece {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.symbol())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // the packing pays for itself by being the zobrist row index straight off, which
    // only holds while every piece lands on a row of its own inside the table
    #[test]
    fn every_piece_indexes_a_zobrist_row_of_its_own() {
        let mut seen = Vec::new();

        for piece_type in PieceType::ALL {
            for color in Color::BOTH {
                let index = Piece::new(piece_type, color).zobrist_index();

                assert!(
                    index < Piece::ZOBRIST_INDEX_COUNT,
                    "{piece_type:?} {color:?} wants row {index}"
                );
                assert!(
                    !seen.contains(&index),
                    "{piece_type:?} {color:?} shares row {index}"
                );
                seen.push(index);
            }
        }
    }

    // what a mask that is a bit too wide or too narrow breaks first
    #[test]
    fn a_packed_piece_unpacks_again() {
        for piece_type in PieceType::ALL {
            for color in Color::BOTH {
                let piece = Piece::new(piece_type, color);

                assert_eq!(piece.piece_type(), piece_type);
                assert_eq!(piece.color(), color);
            }
        }
    }
}
