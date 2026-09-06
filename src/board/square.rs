// Square numbering and the geometry every generator shares.
//
// A square is a plain index 0..64 with `index = rank * 8 + file`, so a1 = 0,
// h1 = 7, a8 = 56, h8 = 63.
//
// Everything that steps around the board goes through `offset` or `ray`, which
// return None / stop at the edge. That keeps the "is this still on the board"
// check in one place instead of in every loop.

use crate::board::piece::Color;

pub fn file_of(square: u8) -> u8 {
    square % 8
}

pub fn rank_of(square: u8) -> u8 {
    square / 8
}

// a square as it is written down, e.g. 4 -> "e1"
pub fn square_name(square: u8) -> String {
    let file = (b'a' + file_of(square)) as char;
    let rank = (b'1' + rank_of(square)) as char;
    format!("{file}{rank}")
}

// the square a name like "e1" stands for, or None when it is not one
pub fn square_from_name(text: &str) -> Option<u8> {
    let bytes = text.as_bytes();
    let [file, rank] = bytes else { return None };

    if !(b'a'..=b'h').contains(file) || !(b'1'..=b'8').contains(rank) {
        return None;
    }

    Some((rank - b'1') * 8 + (file - b'a'))
}

// the square at (file, rank), or None when that is off the board
pub fn square_at(file: i8, rank: i8) -> Option<u8> {
    if (0..8).contains(&file) && (0..8).contains(&rank) {
        Some((rank * 8 + file) as u8)
    } else {
        None
    }
}

// one step away from `square`, or None when that step leaves the board
pub fn offset(square: u8, (file_step, rank_step): (i8, i8)) -> Option<u8> {
    square_at(
        file_of(square) as i8 + file_step,
        rank_of(square) as i8 + rank_step,
    )
}

// every square from `from` in the given direction, up to the edge of the board
// (`from` itself is not part of it)
pub fn ray(from: u8, step: (i8, i8)) -> impl Iterator<Item = u8> {
    let mut current = Some(from);
    std::iter::from_fn(move || {
        let next = offset(current?, step);
        current = next;
        next
    })
}

// the single step that leads from one square towards another, when the two share a
// rank, a file or a diagonal - None when they do not line up at all (a knight's jump)
// or when they are the same square
pub fn direction_between(from: u8, to: u8) -> Option<(i8, i8)> {
    let file_step = file_of(to) as i8 - file_of(from) as i8;
    let rank_step = rank_of(to) as i8 - rank_of(from) as i8;

    let lines_up = file_step == 0 || rank_step == 0 || file_step.abs() == rank_step.abs();
    if !lines_up || (file_step == 0 && rank_step == 0) {
        return None;
    }

    Some((file_step.signum(), rank_step.signum()))
}

// the squares strictly between two positions on the same rank
pub fn squares_between(a: u8, b: u8) -> impl Iterator<Item = u8> {
    let (low, high) = if a < b { (a, b) } else { (b, a) };
    (low + 1)..high
}

// where the pawn being captured en passant stands - next to the capturing pawn,
// one rank behind the square it is captured on
pub fn en_passant_captured_square(to: u8, capturing_color: Color) -> u8 {
    match capturing_color {
        Color::White => to - 8,
        Color::Black => to + 8,
    }
}

// (file_step, rank_step) tables, shared by the move generators and the attack scan
pub const KING_STEPS: [(i8, i8); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

pub const KNIGHT_STEPS: [(i8, i8); 8] = [
    (1, 2),
    (2, 1),
    (2, -1),
    (1, -2),
    (-1, -2),
    (-2, -1),
    (-2, 1),
    (-1, 2),
];

pub const DIAGONAL_STEPS: [(i8, i8); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];

pub const STRAIGHT_STEPS: [(i8, i8); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
