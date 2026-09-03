// Castling rights and the squares castling involves.

use crate::board::piece::Color;

// king side = short castle (towards the h file), queen side = long castle (towards the a file)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CastleSide {
    King,
    Queen,
}

impl CastleSide {
    pub const BOTH: [CastleSide; 2] = [CastleSide::King, CastleSide::Queen];
}

// the four castling flags in a fixed order - also the order of the zobrist castling keys
pub const CASTLE_FLAGS: [(Color, CastleSide); 4] = [
    (Color::White, CastleSide::King),
    (Color::White, CastleSide::Queen),
    (Color::Black, CastleSide::King),
    (Color::Black, CastleSide::Queen),
];

// one flag per castling side, cleared for good as soon as the king moves or the
// matching rook moves away from / is captured on its start square
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CastlingRights {
    white_king_side: bool,
    white_queen_side: bool,
    black_king_side: bool,
    black_queen_side: bool,
}

impl CastlingRights {
    pub const ALL: CastlingRights = CastlingRights {
        white_king_side: true,
        white_queen_side: true,
        black_king_side: true,
        black_queen_side: true,
    };

    #[allow(dead_code)]
    pub const NONE: CastlingRights = CastlingRights {
        white_king_side: false,
        white_queen_side: false,
        black_king_side: false,
        black_queen_side: false,
    };

    pub fn get(self, color: Color, side: CastleSide) -> bool {
        match (color, side) {
            (Color::White, CastleSide::King) => self.white_king_side,
            (Color::White, CastleSide::Queen) => self.white_queen_side,
            (Color::Black, CastleSide::King) => self.black_king_side,
            (Color::Black, CastleSide::Queen) => self.black_queen_side,
        }
    }

    pub fn clear(&mut self, color: Color, side: CastleSide) {
        let flag = match (color, side) {
            (Color::White, CastleSide::King) => &mut self.white_king_side,
            (Color::White, CastleSide::Queen) => &mut self.white_queen_side,
            (Color::Black, CastleSide::King) => &mut self.black_king_side,
            (Color::Black, CastleSide::Queen) => &mut self.black_queen_side,
        };
        *flag = false;
    }

    // the king moved, so both of that side's castles are gone
    pub fn clear_color(&mut self, color: Color) {
        for side in CastleSide::BOTH {
            self.clear(color, side);
        }
    }

    // a rook that leaves its start square, or gets captured on it, ends that castling side
    // called with both the from and the to square of every move, which covers both cases
    pub fn clear_on_square(&mut self, square: u8) {
        for (color, side) in CASTLE_FLAGS {
            if rook_start_square(color, side) == square {
                self.clear(color, side);
            }
        }
    }
}

// e1 / e8
pub fn king_start_square(color: Color) -> u8 {
    match color {
        Color::White => 4,
        Color::Black => 60,
    }
}

// a1 / h1 / a8 / h8
pub fn rook_start_square(color: Color, side: CastleSide) -> u8 {
    match (color, side) {
        (Color::White, CastleSide::King) => 7,
        (Color::White, CastleSide::Queen) => 0,
        (Color::Black, CastleSide::King) => 63,
        (Color::Black, CastleSide::Queen) => 56,
    }
}

// where the king ends up after castling (g1 / c1 / g8 / c8)
pub fn king_castle_square(color: Color, side: CastleSide) -> u8 {
    match (color, side) {
        (Color::White, CastleSide::King) => 6,
        (Color::White, CastleSide::Queen) => 2,
        (Color::Black, CastleSide::King) => 62,
        (Color::Black, CastleSide::Queen) => 58,
    }
}

// where the rook ends up after castling (f1 / d1 / f8 / d8)
// this is also the square the king crosses
pub fn rook_castle_square(color: Color, side: CastleSide) -> u8 {
    match (color, side) {
        (Color::White, CastleSide::King) => 5,
        (Color::White, CastleSide::Queen) => 3,
        (Color::Black, CastleSide::King) => 61,
        (Color::Black, CastleSide::Queen) => 59,
    }
}
