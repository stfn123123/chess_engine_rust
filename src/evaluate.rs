use crate::board::Board;
use crate::board::board::insufficient_minors;
use crate::board::piece::{Color, Piece, PieceType};
use crate::board::square::file_of;

type Psqt = [i16; 64];
type PsqtSet = [Psqt; 4];

struct W(i16, i16); // (middlegame, endgame)

impl W {
    fn at(self, phase: i32) -> i32 {
        interpolate(self.0, self.1, phase)
    }
}

const KING_BASE: i16 = 0;
const QUEEN_BASE: i16 = 900;
const ROOK_BASE: i16 = 500;
const KNIGHT_BASE: i16 = 300;
const BISHOP_BASE: i16 = 300;
const PAWN_BASE: i16 = 100;

pub const MATE: i32 = 100_000;
pub const MATE_BOUND: i32 = MATE - 1_000;

const BISHOP_PAIR: W = W(10, 40);

const ISOLATED_PAWN: i32 = -40;
const PASSED_PAWN: i32 = 50;

const QUEEN: Psqt = [
    -20,-10,-10, -5, -5,-10,-10,-20,
    -10,  0,  0,  0,  0,  0,  0,-10,
    -10,  0,  5,  5,  5,  5,  0,-10,
    -5,  0,  5,  5,  5,  5,  0, -5,
    0,  0,  5,  5,  5,  5,  0, -5,
    -10,  5,  5,  5,  5,  5,  0,-10,
    -10,  0,  5,  0,  0,  0,  0,-10,
    -20,-10,-10, -5, -5,-10,-10,-20
];

const ROOK: Psqt = [
    0,  0,  0,  0,  0,  0,  0,  0,
    5, 10, 10, 10, 10, 10, 10,  5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    0,  0,  0,  5,  5,  0,  0,  0
];

const KNIGHT: Psqt = [
    -50,-40,-30,-30,-30,-30,-40,-50,
    -40,-20,  0,  0,  0,  0,-20,-40,
    -30,  0, 10, 15, 15, 10,  0,-30,
    -30,  5, 15, 20, 20, 15,  5,-30,
    -30,  0, 15, 20, 20, 15,  0,-30,
    -30,  5, 10, 15, 15, 10,  5,-30,
    -40,-20,  0,  5,  5,  0,-20,-40,
    -50,-40,-30,-30,-30,-30,-40,-50,
];

const BISHOP: Psqt = [
    -20,-10,-10,-10,-10,-10,-10,-20,
    -10,  0,  0,  0,  0,  0,  0,-10,
    -10,  0,  5, 10, 10,  5,  0,-10,
    -10,  5,  5, 10, 10,  5,  5,-10,
    -10,  0, 10, 10, 10, 10,  0,-10,
    -10, 10, 10, 10, 10, 10, 10,-10,
    -10,  5,  0,  0,  0,  0,  5,-10,
    -20,-10,-10,-10,-10,-10,-10,-20,
];

const PAWN: Psqt = [
    0,  0,  0,  0,  0,  0,  0,  0,
    50, 50, 50, 50, 50, 50, 50, 50,
    10, 10, 20, 30, 30, 20, 10, 10,
    5,  5, 10, 25, 25, 10,  5,  5,
    0,  0,  0, 20, 20,  0,  0,  0,
    5, -5,-10,  0,  0,-10, -5,  5,
    5, 10, 10,-20,-20, 10, 10,  5,
    0,  0,  0,  0,  0,  0,  0,  0
];

const PAWN_AFTER_CASTLE_KING: Psqt = [
    0,  0,  0,  0,  0,  0,  0,  0,
    50, 50, 50, 50, 50, 50, 50, 50,
    10, 10, 20, 30, 30, 20, 10, 10,
    5,  5, 10, 25, 25,  5,  0,  0,
    5,  5,  5, 20, 20, -5, -5, -5,
    5,  5,  5,  0,  0,-10, -5, 5,
    0,  0,  0,-20,-20, 15, 15, 10,
    0,  0,  0,  0,  0,  0,  0,  0
];

const PAWN_AFTER_CASTLE_QUEEN: Psqt = [
     0,  0,  0,  0,  0,  0,  0,  0,
     50, 50, 50, 50, 50, 50, 50, 50,
     10, 10, 20, 30, 30, 20, 10, 10,
     0,  0, 5, 25, 25, 10,  5,  5,
    -5, -5, -5, 20, 20,  5,  5,  5,
     5, -5,-10,  0,  0,  5,  5,  5,
     10, 15, 15,-20,-20,  0,  0,  0,
      0,  0,  0,  0,  0,  0,  0,  0
];

const PAWN_LATE: Psqt = [
      0,  0,  0,  0,  0,  0,  0,  0,
     50, 50, 50, 50, 50, 50, 50, 50,
     20, 30, 30, 30, 30, 30, 30, 20,
     10, 10, 10, 25, 25, 10, 10, 10,
      5,  5,  5, 20, 20,  5,  5,  5,
     -5, -5,-10,  0,  0,-10, -5, -5,
    -10,-10,-10,-20,-20,-10,-10,-10,
      0,  0,  0,  0,  0,  0,  0,  0
];

const KING_MID: Psqt = [
    -30,-40,-40,-50,-50,-40,-40,-30,
    -30,-40,-40,-50,-50,-40,-40,-30,
    -30,-40,-40,-50,-50,-40,-40,-30,
    -30,-40,-40,-50,-50,-40,-40,-30,
    -20,-30,-30,-40,-40,-30,-30,-20,
    -10,-20,-20,-20,-20,-20,-20,-10,
    20, 20,  0,  0,  0,  0, 20, 20,
    20, 30, 10,  0,  0, 10, 30, 20
];

const KING_END: Psqt = [
    -50,-40,-30,-20,-20,-30,-40,-50,
    -30,-20,-10,  0,  0,-10,-20,-30,
    -30,-10, 20, 30, 30, 20,-10,-30,
    -30,-10, 30, 40, 40, 30,-10,-30,
    -30,-10, 30, 40, 40, 30,-10,-30,
    -30,-10, 20, 30, 30, 20,-10,-30,
    -30,-30,  0,  0,  0,  0,-30,-30,
    -50,-30,-30,-30,-30,-30,-30,-50
];

// every square on one file, a1 upwards
const fn file_mask(file: usize) -> u64 {
    0x0101_0101_0101_0101 << file
}

// every square on one rank, a-file to h-file
const fn rank_mask(rank: usize) -> u64 {
    0xff << (rank * 8)
}

// the two files either side of one, which is where a pawn's neighbours would stand
const ADJACENT_FILES: [u64; 8] = {
    let mut masks = [0; 8];
    let mut file = 0;

    while file < 8 {
        if file > 0 {
            masks[file] |= file_mask(file - 1);
        }
        if file < 7 {
            masks[file] |= file_mask(file + 1);
        }
        file += 1;
    }

    masks
};

// every square an enemy pawn could stop a pawn on this square from: the three files
// it walks past, on the ranks still ahead of it. Indexed by color, then square
const PASSED_MASK: [[u64; 64]; 2] = {
    let mut masks = [[0; 64]; 2];
    let mut square = 0;

    while square < 64 {
        let file = square % 8;
        let rank = square / 8;
        let files = file_mask(file) | ADJACENT_FILES[file];

        let mut white = 0;
        let mut ahead = rank + 1;
        while ahead < 8 {
            white |= rank_mask(ahead);
            ahead += 1;
        }

        let mut black = 0;
        let mut behind = 0;
        while behind < rank {
            black |= rank_mask(behind);
            behind += 1;
        }

        masks[0][square] = files & white;
        masks[1][square] = files & black;
        square += 1;
    }

    masks
};

// the base value is baked in at compile time, so a piece costs a single table read
const fn with_base(mut table: Psqt, base: i16) -> Psqt {
    let mut square = 0;
    while square < 64 {
        table[square] += base;
        square += 1;
    }
    table
}

const TABLES: PsqtSet = [
    with_base(KNIGHT, KNIGHT_BASE),
    with_base(BISHOP, BISHOP_BASE),
    with_base(ROOK, ROOK_BASE),
    with_base(QUEEN, QUEEN_BASE),
];

// TABLES starts at the knight, BASE_VALUES at the king
const PSQT_OFFSET: usize = PieceType::Knight.board_index();

const BASE_VALUES: [i16; 6] = [
    KING_BASE,
    PAWN_BASE,
    KNIGHT_BASE,
    BISHOP_BASE,
    ROOK_BASE,
    QUEEN_BASE,
];

const TOTAL_PHASE: i32 = 2 * (2 * PieceType::Knight.phase_weight()
    + 2 * PieceType::Bishop.phase_weight()
    + 2 * PieceType::Rook.phase_weight()
    + PieceType::Queen.phase_weight());

// the types that can still force a mate on their own
const MATING_TYPES: [PieceType; 3] = [PieceType::Pawn, PieceType::Rook, PieceType::Queen];

// positive: side to move stands better
pub fn evaluate(board: &Board) -> i32 {
    let bishops = counts_of(board, PieceType::Bishop);
    let knights = counts_of(board, PieceType::Knight);

    // cheap enough to ask before anything is scored
    if !can_mate(board) && insufficient_minors(bishops, knights) {
        return 0;
    }

    let kings = [
        board.king_square(Color::White),
        board.king_square(Color::Black),
    ];
    let phase = board.phase().clamp(0, TOTAL_PHASE);

    // the knights, bishops, rooks and queens are already summed up by the board
    let mut score = board.psqt();
    score += pawn_score(board, kings, phase);
    score += pawn_structure(board);
    score += king_score(kings, phase);
    score += bishop_pair_score(bishops, phase);

    signed(score, board.turn())
}

// what one piece contributes to the running total the board keeps - kings and pawns
// are left out, since both are only scored once the phase is known
pub(crate) fn incremental_score(piece: Piece, square: u8) -> i32 {
    let index = piece.piece_type().board_index();
    if index < PSQT_OFFSET {
        return 0;
    }

    let color = piece.color();
    let value = TABLES[index - PSQT_OFFSET][table_index(square, color)];

    signed(value as i32, color)
}

pub fn piece_value(piece_type: PieceType) -> i16 {
    BASE_VALUES[piece_type.board_index()]
}

pub fn game_phase_of(board: &Board) -> f32 {
    board.phase().clamp(0, TOTAL_PHASE) as f32 / TOTAL_PHASE as f32
}

fn color_index(color: Color) -> usize {
    match color {
        Color::White => 0,
        Color::Black => 1,
    }
}

// white counts up, black counts down
fn signed(value: i32, color: Color) -> i32 {
    match color {
        Color::White => value,
        Color::Black => -value,
    }
}

fn counts_of(board: &Board, piece_type: PieceType) -> [usize; 2] {
    Color::BOTH.map(|color| board.piece_board(piece_type, color).count_ones() as usize)
}

fn can_mate(board: &Board) -> bool {
    let mut mating = 0;

    for color in Color::BOTH {
        for piece_type in MATING_TYPES {
            mating |= board.piece_board(piece_type, color);
        }
    }

    mating != 0
}

// the tables are written from black's side, so white's squares are mirrored by rank
fn table_index(square: u8, color: Color) -> usize {
    match color {
        Color::White => (square ^ 56) as usize,
        Color::Black => square as usize,
    }
}

fn interpolate(middlegame: i16, endgame: i16, phase: i32) -> i32 {
    (middlegame as i32 * phase + endgame as i32 * (TOTAL_PHASE - phase)) / TOTAL_PHASE
}

fn king_score(kings: [Option<u8>; 2], phase: i32) -> i32 {
    let mut score = 0;

    for color in Color::BOTH {
        let Some(square) = kings[color_index(color)] else {
            continue;
        };

        let index = table_index(square, color);
        score += signed(interpolate(KING_MID[index], KING_END[index], phase), color);
    }

    score
}

fn bishop_pair_score(bishops: [usize; 2], phase: i32) -> i32 {
    let bonus = BISHOP_PAIR.at(phase);
    let mut score = 0;

    if bishops[color_index(Color::White)] >= 2 {
        score += bonus;
    }
    if bishops[color_index(Color::Black)] >= 2 {
        score -= bonus;
    }

    score
}
fn pawn_score(board: &Board, kings: [Option<u8>; 2], phase: i32) -> i32 {
    let mut score = 0;

    for color in Color::BOTH {
        let middlegame = pawn_table(kings[color_index(color)]);
        let mut pawns = board.piece_board(PieceType::Pawn, color);
        let mut side = 0;

        while pawns != 0 {
            let square = pawns.trailing_zeros() as u8;
            pawns &= pawns - 1;

            let index = table_index(square, color);
            side += PAWN_BASE as i32 + interpolate(middlegame[index], PAWN_LATE[index], phase);
        }

        score += signed(side, color);
    }

    score
}

// a king on either outer three files has castled to that wing
fn pawn_table(king: Option<u8>) -> &'static Psqt {
    let Some(square) = king else {
        return &PAWN;
    };

    match file_of(square) {
        0..=2 => &PAWN_AFTER_CASTLE_QUEEN,
        5..=7 => &PAWN_AFTER_CASTLE_KING,
        _ => &PAWN,
    }
}

// reward passed pawns, punish isolated ones
fn pawn_structure(board: &Board) -> i32 {
    let mut score = 0;

    for color in Color::BOTH {
        let friendly = board.piece_board(PieceType::Pawn, color);
        let enemy = board.piece_board(PieceType::Pawn, color.opponent());
        let mut pawns = friendly;
        let mut side = 0;

        while pawns != 0 {
            let square = pawns.trailing_zeros() as u8;
            pawns &= pawns - 1;

            // the pawn stands on its own file, so it never matches its own mask
            if friendly & ADJACENT_FILES[file_of(square) as usize] == 0 {
                side += ISOLATED_PAWN;
            }
            if enemy & PASSED_MASK[color_index(color)][square as usize] == 0 {
                side += PASSED_PAWN;
            }
        }

        score += signed(side, color);
    }

    score
}

fn king_safety() -> i32 {
    return 0
}

fn rook_open_file() -> i32 {
    return 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::piece::Piece;

    fn start_position() -> Board {
        let mut board = Board::new();
        board.set_start_position();
        board
    }

    #[test]
    fn the_start_position_is_equal() {
        assert_eq!(evaluate(&start_position()), 0);
    }

    #[test]
    fn the_phase_runs_from_a_full_board_down_to_bare_kings() {
        assert_eq!(game_phase_of(&start_position()), 1.0);

        let mut bare = Board::new();
        bare.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        bare.add_piece(Piece::new(PieceType::King, Color::Black), 60); // e8

        assert_eq!(game_phase_of(&bare), 0.0);
    }

    #[test]
    fn a_pawn_endgame_is_all_the_way_into_the_endgame() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        board.add_piece(Piece::new(PieceType::King, Color::Black), 60); // e8

        for file in 0..8 {
            board.add_piece(Piece::new(PieceType::Pawn, Color::White), 8 + file);
            board.add_piece(Piece::new(PieceType::Pawn, Color::Black), 48 + file);
        }

        assert_eq!(game_phase_of(&board), 0.0);
    }

    #[test]
    fn the_pawn_psqts_are_roughly_balanced() {
        let tables: [(&str, Psqt); 4] = [
            ("PAWN", PAWN),
            ("PAWN_AFTER_CASTLE_KING", PAWN_AFTER_CASTLE_KING),
            ("PAWN_AFTER_CASTLE_QUEEN", PAWN_AFTER_CASTLE_QUEEN),
            ("PAWN_LATE", PAWN_LATE),
        ];

        let mut sums = Vec::new();

        for (name, table) in tables {
            let sum: i32 = table.iter().map(|&v| v as i32).sum();
            println!("{name} (sum = {sum}):");
            sums.push(sum);
        }

        let min = *sums.iter().min().unwrap();
        let max = *sums.iter().max().unwrap();

        assert!(max - min <= 10, "pawn psqts differ too much: {sums:?}");
    }

    #[test]
    fn every_pawn_still_has_a_neighbour_at_the_start() {
        assert_eq!(pawn_structure(&start_position()), 0);
    }

    #[test]
    fn a_pawn_with_both_neighbouring_files_empty_is_isolated() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::Pawn, Color::White), 8); // a2
        board.add_piece(Piece::new(PieceType::Pawn, Color::White), 10); // c2
        board.add_piece(Piece::new(PieceType::Pawn, Color::Black), 48); // a7
        board.add_piece(Piece::new(PieceType::Pawn, Color::Black), 49); // b7
        board.add_piece(Piece::new(PieceType::Pawn, Color::Black), 50); // c7

        // the black chain holds up both white pawns, so only the isolation is left
        assert_eq!(pawn_structure(&board), 2 * ISOLATED_PAWN);
    }

    #[test]
    fn a_pawn_on_the_next_file_is_neighbour_enough() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::Pawn, Color::White), 12); // e2
        board.add_piece(Piece::new(PieceType::Pawn, Color::White), 13); // f2

        assert_eq!(pawn_structure(&board), 2 * PASSED_PAWN);
    }

    #[test]
    fn a_passer_is_stopped_from_the_next_file_too() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::Pawn, Color::White), 28); // e4
        board.add_piece(Piece::new(PieceType::Pawn, Color::White), 29); // f4
        board.add_piece(Piece::new(PieceType::Pawn, Color::Black), 51); // d7

        // d7 holds up e4 from the side but never reaches f4, and is isolated itself
        assert_eq!(pawn_structure(&board), PASSED_PAWN - ISOLATED_PAWN);
    }

    #[test]
    fn the_queens_carry_a_third_of_the_phase() {
        let queens = 2 * PieceType::Queen.phase_weight();

        assert_eq!(queens * 3, TOTAL_PHASE);
        assert_eq!(PieceType::Pawn.phase_weight(), 0);
        assert_eq!(PieceType::King.phase_weight(), 0);
    }

    #[test]
    fn the_bishop_pair_grows_towards_the_endgame() {
        assert_eq!(bishop_pair_score([2, 0], TOTAL_PHASE), 10);
        assert_eq!(bishop_pair_score([2, 0], 0), 40);

        assert_eq!(bishop_pair_score([1, 0], 0), 0, "one bishop is not a pair");
        assert_eq!(bishop_pair_score([2, 2], 0), 0, "both sides have it");
    }

    #[test]
    fn the_king_turns_around_towards_the_endgame() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 28); // e4
        board.add_piece(Piece::new(PieceType::King, Color::Black), 62); // g8

        let kings = [
            board.king_square(Color::White),
            board.king_square(Color::Black),
        ];

        assert!(king_score(kings, TOTAL_PHASE) < 0, "centre is bad early");
        assert!(king_score(kings, 0) > 0, "centre is good late");
    }

    #[test]
    fn the_score_is_for_the_side_to_move() {
        let mut board = start_position();
        board.make_move_from_squares(12, 28, None); // e2e4

        assert_eq!(board.turn(), Color::Black);

        let score = evaluate(&board);
        assert!(score < 0, "black to move scored {score}");
    }

    #[test]
    fn a_symmetric_position_is_equal_again() {
        let mut board = start_position();
        board.make_move_from_squares(12, 28, None); // e2e4
        board.make_move_from_squares(52, 36, None); // e7e5

        assert_eq!(evaluate(&board), 0);
    }

    #[test]
    fn material_counts() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 4);
        board.add_piece(Piece::new(PieceType::King, Color::Black), 60);
        board.add_piece(Piece::new(PieceType::Queen, Color::White), 3);

        let score = evaluate(&board);
        assert!(
            (QUEEN_BASE as i32 - 50..=QUEEN_BASE as i32 + 50).contains(&score),
            "a lone queen scored {score}"
        );
    }

    #[test]
    fn mirrored_positions_score_the_same() {
        let mut white_side = Board::new();
        white_side.add_piece(Piece::new(PieceType::King, Color::White), 4);
        white_side.add_piece(Piece::new(PieceType::King, Color::Black), 60);
        white_side.add_piece(Piece::new(PieceType::Knight, Color::White), 18); // c3
        white_side.add_piece(Piece::new(PieceType::Pawn, Color::White), 28); // e4

        let mut black_side = Board::new();
        black_side.add_piece(Piece::new(PieceType::King, Color::White), 4);
        black_side.add_piece(Piece::new(PieceType::King, Color::Black), 60);
        black_side.add_piece(Piece::new(PieceType::Knight, Color::Black), 42); // c6
        black_side.add_piece(Piece::new(PieceType::Pawn, Color::Black), 36); // e5

        assert_eq!(evaluate(&white_side), -evaluate(&black_side));
    }

    #[test]
    fn two_knights_against_a_bare_king_is_a_draw() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        board.add_piece(Piece::new(PieceType::King, Color::Black), 60); // e8
        board.add_piece(Piece::new(PieceType::Knight, Color::White), 1); // b1
        board.add_piece(Piece::new(PieceType::Knight, Color::White), 6); // g1

        assert_eq!(evaluate(&board), 0);
    }

    #[test]
    fn a_knight_up_against_a_knight_still_counts() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        board.add_piece(Piece::new(PieceType::King, Color::Black), 60); // e8
        board.add_piece(Piece::new(PieceType::Knight, Color::White), 1); // b1
        board.add_piece(Piece::new(PieceType::Knight, Color::White), 6); // g1
        board.add_piece(Piece::new(PieceType::Knight, Color::Black), 57); // b8

        let score = evaluate(&board);
        assert!(score > 200, "a knight up scored {score}");
    }

    #[test]
    fn a_draw_by_material_is_worth_nothing() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        board.add_piece(Piece::new(PieceType::King, Color::Black), 60); // e8
        board.add_piece(Piece::new(PieceType::Bishop, Color::White), 2); // c1

        assert_eq!(evaluate(&board), 0);
    }
}
