// The two sides and the pieces they own.
//
// A Piece is packed into a single byte: the low three bits are the piece type,
// the next two are the color. That keeps a piece Copy-cheap and lets the byte be
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

// the discriminants are the color bits stored inside a Piece
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Color {
    White = 8,
    Black = 16,
}

impl Color {
    pub const BOTH: [Color; 2] = [Color::White, Color::Black];

    pub fn opponent(self) -> Color {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
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

const TYPE_MASK: u8 = 0b00111;
const COLOR_MASK: u8 = 0b11000;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Piece(u8);

impl Piece {
    // the biggest packed value is Queen | Black = 6 | 16 = 22, so a table indexed by
    // the raw byte needs 23 rows - a few unused ones in exchange for no mapping step
    pub const ZOBRIST_INDEX_COUNT: usize = 23;

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
