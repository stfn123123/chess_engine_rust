// The squares a piece attacks, read off a table instead of walked square by square.
//
// A stepping piece is easy: a knight on e4 attacks the same eight squares whatever
// else stands on the board, so one mask per square is the whole answer.
//
// A slider is the interesting one, because what it attacks depends on what is in the
// way. This is the classical approach: for every direction and every square, the whole
// ray out to the edge is kept in a table. Given the pieces on the board, the ray runs
// as far as the nearest of them -
//
//     ray       . x x x x x x x     the whole ray from the square, out to the edge
//     occupied  . . . o . . o .     what stands on the board
//     blockers  . . . x . . x .     ray & occupied
//     nearest   . . . ^ . . . .     the first bit of that, found with a bitscan
//     behind    . . . . x x x x     RAYS[direction][nearest]
//     attacks   . x x x . . . .     ray ^ behind
//
// so it costs a mask, an and, a bitscan and an xor - four of those for a bishop or a
// rook, eight for a queen. The blocker itself stays in the set: it is the square the
// slider can capture on, and whoever asks takes their own pieces back off with `& !own`.
//
// The tables are built at compile time, the same way the zobrist keys are.

use crate::board::piece::Color;

// the eight directions, as the index of a row in RAYS. The first four run up the board
// (towards h8) and the last four down it, which is what decides how the nearest blocker
// on the ray is found: the lowest bit set going up, the highest going down
const NORTH: usize = 0;
const NORTH_EAST: usize = 1;
const EAST: usize = 2;
const SOUTH_EAST: usize = 3;
const SOUTH: usize = 4;
const SOUTH_WEST: usize = 5;
const WEST: usize = 6;
const NORTH_WEST: usize = 7;

// (file, rank) steps, in the order the direction constants above number them
const STEPS: [(i8, i8); 8] = [
    (0, 1),
    (1, 1),
    (1, 0),
    (1, -1),
    (0, -1),
    (-1, -1),
    (-1, 0),
    (-1, 1),
];

// every square a ray passes through, from a square out to the edge, the square itself
// not among them
static RAYS: [[u64; 64]; 8] = build_rays();

static KNIGHT_ATTACKS: [u64; 64] = build_steps(KNIGHT_STEPS);
static KING_ATTACKS: [u64; 64] = build_steps(KING_STEPS);

// what a pawn of each color takes on, indexed by Color::index
static PAWN_ATTACKS: [[u64; 64]; 2] = build_pawn_attacks();

const KNIGHT_STEPS: [(i8, i8); 8] = [
    (1, 2),
    (2, 1),
    (2, -1),
    (1, -2),
    (-1, -2),
    (-2, -1),
    (-2, 1),
    (-1, 2),
];

const KING_STEPS: [(i8, i8); 8] = STEPS;

#[inline]
pub fn knight_attacks(from: u8) -> u64 {
    KNIGHT_ATTACKS[from as usize]
}

#[inline]
pub fn king_attacks(from: u8) -> u64 {
    KING_ATTACKS[from as usize]
}

// the two squares a pawn of this color captures on, whether or not anything is there
#[inline]
pub fn pawn_attacks(from: u8, color: Color) -> u64 {
    PAWN_ATTACKS[color.index()][from as usize]
}

// the four rays are written out rather than walked over a list of directions, so that
// each one carries its direction as a constant: which end of the blockers to scan is
// then settled where it is compiled instead of looked up on every ray
#[inline]
pub fn bishop_attacks(from: u8, occupied: u64) -> u64 {
    ray_up(from, occupied, NORTH_EAST)
        | ray_up(from, occupied, NORTH_WEST)
        | ray_down(from, occupied, SOUTH_EAST)
        | ray_down(from, occupied, SOUTH_WEST)
}

#[inline]
pub fn rook_attacks(from: u8, occupied: u64) -> u64 {
    ray_up(from, occupied, NORTH)
        | ray_up(from, occupied, EAST)
        | ray_down(from, occupied, SOUTH)
        | ray_down(from, occupied, WEST)
}

#[inline]
pub fn queen_attacks(from: u8, occupied: u64) -> u64 {
    bishop_attacks(from, occupied) | rook_attacks(from, occupied)
}

// a ray towards the high squares, where the nearest piece on it is the lowest bit set
#[inline]
fn ray_up(from: u8, occupied: u64, direction: usize) -> u64 {
    let ray = RAYS[direction][from as usize];
    let blockers = ray & occupied;

    if blockers == 0 {
        return ray;
    }

    // everything past the nearest piece is out of reach; the piece itself stays
    ray ^ RAYS[direction][blockers.trailing_zeros() as usize]
}

// and one towards the low squares, where it is the highest bit set instead
#[inline]
fn ray_down(from: u8, occupied: u64, direction: usize) -> u64 {
    let ray = RAYS[direction][from as usize];
    let blockers = ray & occupied;

    if blockers == 0 {
        return ray;
    }

    ray ^ RAYS[direction][(63 - blockers.leading_zeros()) as usize]
}

// -------------------- the tables --------------------

const fn build_rays() -> [[u64; 64]; 8] {
    let mut rays = [[0u64; 64]; 8];

    let mut direction = 0;
    while direction < 8 {
        let (file_step, rank_step) = STEPS[direction];

        let mut square = 0;
        while square < 64 {
            let mut file = (square % 8) as i8 + file_step;
            let mut rank = (square / 8) as i8 + rank_step;

            // on and on until the ray walks off the board
            while file >= 0 && file < 8 && rank >= 0 && rank < 8 {
                rays[direction][square] |= 1u64 << (rank * 8 + file);
                file += file_step;
                rank += rank_step;
            }

            square += 1;
        }

        direction += 1;
    }

    rays
}

// one mask per square for a piece that takes a single step in each direction
const fn build_steps(steps: [(i8, i8); 8]) -> [u64; 64] {
    let mut attacks = [0u64; 64];

    let mut square = 0;
    while square < 64 {
        let mut index = 0;
        while index < 8 {
            let (file_step, rank_step) = steps[index];
            let file = (square % 8) as i8 + file_step;
            let rank = (square / 8) as i8 + rank_step;

            if file >= 0 && file < 8 && rank >= 0 && rank < 8 {
                attacks[square] |= 1u64 << (rank * 8 + file);
            }

            index += 1;
        }

        square += 1;
    }

    attacks
}

// a pawn takes one square diagonally forward, forward being up the board for white
const fn build_pawn_attacks() -> [[u64; 64]; 2] {
    let mut attacks = [[0u64; 64]; 2];

    let mut color = 0;
    while color < 2 {
        let rank_step = if color == 0 { 1 } else { -1 };

        let mut square = 0;
        while square < 64 {
            let rank = (square / 8) as i8 + rank_step;

            let mut file_step = -1;
            while file_step <= 1 {
                let file = (square % 8) as i8 + file_step;

                if file_step != 0 && file >= 0 && file < 8 && rank >= 0 && rank < 8 {
                    attacks[color][square] |= 1u64 << (rank * 8 + file);
                }

                file_step += 1;
            }

            square += 1;
        }

        color += 1;
    }

    attacks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::square::{
        DIAGONAL_STEPS, KING_STEPS as WALKED_KING_STEPS, KNIGHT_STEPS as WALKED_KNIGHT_STEPS,
        STRAIGHT_STEPS, bit, offset, ray,
    };

    // the same answer the square walkers give, which is what the engine ran on before
    // the tables existed: every square, one direction at a time
    fn walked_slider(from: u8, occupied: u64, steps: &[(i8, i8)]) -> u64 {
        let mut attacks = 0;

        for &step in steps {
            for square in ray(from, step) {
                attacks |= bit(square);
                // the first piece is reached and everything behind it is not
                if occupied & bit(square) != 0 {
                    break;
                }
            }
        }

        attacks
    }

    fn walked_steps(from: u8, steps: &[(i8, i8)]) -> u64 {
        steps
            .iter()
            .filter_map(|&step| offset(from, step))
            .map(bit)
            .fold(0, |attacks, square| attacks | square)
    }

    // a spread of boards to try the sliders against: empty, full, and a scattering in
    // between. Not random, so a failure is the same failure on the next run
    fn occupancies() -> Vec<u64> {
        let mut boards = vec![0, u64::MAX];

        let mut state = 0x1234_5678_9ABC_DEF0u64;
        for _ in 0..64 {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            let random = state.wrapping_mul(0x2545_F491_4F6C_DD1D);

            // one sparse and one dense board out of every draw
            boards.push(random & random.rotate_left(17));
            boards.push(random | random.rotate_left(29));
        }

        boards
    }

    #[test]
    fn a_knight_attacks_what_the_walker_says_it_does() {
        for square in 0..64u8 {
            assert_eq!(
                knight_attacks(square),
                walked_steps(square, &WALKED_KNIGHT_STEPS),
                "from {square}"
            );
        }
    }

    #[test]
    fn a_king_attacks_what_the_walker_says_it_does() {
        for square in 0..64u8 {
            assert_eq!(
                king_attacks(square),
                walked_steps(square, &WALKED_KING_STEPS),
                "from {square}"
            );
        }
    }

    #[test]
    fn a_pawn_attacks_the_two_squares_diagonally_in_front_of_it() {
        for color in Color::BOTH {
            let rank_step = color.pawn_direction();

            for square in 0..64u8 {
                let expected = walked_steps(square, &[(-1, rank_step), (1, rank_step)]);
                assert_eq!(pawn_attacks(square, color), expected, "from {square}");
            }
        }
    }

    #[test]
    fn a_bishop_is_stopped_by_the_first_piece_on_each_diagonal() {
        for occupied in occupancies() {
            for square in 0..64u8 {
                assert_eq!(
                    bishop_attacks(square, occupied),
                    walked_slider(square, occupied, &DIAGONAL_STEPS),
                    "from {square} with {occupied:#018x}"
                );
            }
        }
    }

    #[test]
    fn a_rook_is_stopped_by_the_first_piece_on_each_file_and_rank() {
        for occupied in occupancies() {
            for square in 0..64u8 {
                assert_eq!(
                    rook_attacks(square, occupied),
                    walked_slider(square, occupied, &STRAIGHT_STEPS),
                    "from {square} with {occupied:#018x}"
                );
            }
        }
    }

    // the queen is the two of them together, and nothing else
    #[test]
    fn a_queen_attacks_the_bishop_squares_and_the_rook_squares() {
        for occupied in occupancies() {
            for square in 0..64u8 {
                assert_eq!(
                    queen_attacks(square, occupied),
                    bishop_attacks(square, occupied) | rook_attacks(square, occupied),
                    "from {square} with {occupied:#018x}"
                );
            }
        }
    }

    // a piece standing on the square is what the slider may take, so it is in the set;
    // the squares behind it are not
    #[test]
    fn a_slider_reaches_the_piece_that_blocks_it_and_no_further() {
        // a rook on a1 with a piece on a4: a2, a3 and a4 are attacked, a5 and up are not
        let occupied = bit(24); // a4
        let attacks = rook_attacks(0, occupied);

        assert!(attacks & bit(8) != 0, "a2 is not attacked");
        assert!(attacks & bit(16) != 0, "a3 is not attacked");
        assert!(attacks & bit(24) != 0, "the blocker on a4 is not attacked");
        assert!(attacks & bit(32) == 0, "a5 is attacked through the blocker");
    }

    // an empty board is the one case where every ray runs all the way to the edge, so
    // the counts are the published ones: a queen covers 21 squares from the corner and
    // 27 from the middle, which is the whole range there is
    #[test]
    fn a_queen_on_an_empty_board_covers_the_squares_it_is_known_to() {
        // a1: seven up the file, seven along the rank, seven up the long diagonal
        assert_eq!(queen_attacks(0, 0).count_ones(), 21);

        // d4: fourteen on the rook lines, thirteen on the two diagonals
        assert_eq!(queen_attacks(27, 0).count_ones(), 27);
        assert_eq!(rook_attacks(27, 0).count_ones(), 14);
        assert_eq!(bishop_attacks(27, 0).count_ones(), 13);
    }
}
