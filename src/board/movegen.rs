// Move generation and the attack scan.
//
// The generator hands out legal moves straight away instead of playing every
// pseudo-legal move only to ask whether it left the king in check. Two scans around
// the king are enough for that: `possible_king_attackers` collects every enemy piece
// that lines up with the king with the pieces in between ignored, and `evaluate_pins`
// sorts those into the ones that really give check and the ones that only pin a piece
// to the king. Out of that comes, for every piece, a mask of the squares it is still
// allowed to move to, and the moves are filtered against that mask as they are made.
//
// Two moves cannot be described by such a mask, because they take a piece off a square
// that is neither the square they start nor the square they end on: a king step (the
// king must not walk backwards along the ray that checks it) and an en passant capture
// (the captured pawn stands beside the destination, and two pawns leaving the same rank
// at once can open a line no pin scan ever saw). Both ask the attack scan directly,
// through an `Overlay` that describes the board as the move would leave it.

use crate::board::Board;
use crate::board::attacks;
use crate::board::castling::{
    CastleSide, king_castle_square, king_start_square, rook_castle_square, rook_start_square,
};
use crate::board::chess_move::Move;
use crate::board::piece::{Color, Piece, PieceType};
use crate::board::square::{
    DIAGONAL_STEPS, KING_STEPS, KNIGHT_STEPS, STRAIGHT_STEPS, bit, direction_between,
    en_passant_captured_square, offset, rank_of, ray, squares_between,
};

// the king is generated on its own, ahead of the others, so the scan over the pieces
// leaves it out
const NON_KING_TYPES: [PieceType; 5] = [
    PieceType::Pawn,
    PieceType::Knight,
    PieceType::Bishop,
    PieceType::Rook,
    PieceType::Queen,
];

impl Board {
    // every legal move of one side, castling included
    pub(crate) fn legal_moves_for(&self, color: Color) -> Vec<Move> {
        // a position holds around forty moves, and far fewer captures
        let mut moves = Vec::with_capacity(48);
        self.generate_into(&mut moves, color, false);
        moves
    }

    // only the moves that take something - what quiescence walks
    pub(crate) fn legal_captures_for(&self, color: Color) -> Vec<Move> {
        let mut moves = Vec::with_capacity(16);
        self.generate_into(&mut moves, color, true);
        moves
    }

    // appends onto whatever the caller hands over, so a search that walks millions of
    // nodes can pass the same list round instead of allocating one per node
    pub(crate) fn generate_into(&self, moves: &mut Vec<Move>, color: Color, captures_only: bool) {
        // a hand built board without a king has no king to keep safe
        let Some(king_square) = self.king_square(color) else {
            // hand built boards only, so the list this makes is not worth saving
            let mut pseudo = self.pseudo_legal_moves(color);
            if captures_only {
                pseudo.retain(|candidate| candidate.captured.is_some());
            }
            moves.append(&mut pseudo);
            return;
        };

        let king = Piece::new(PieceType::King, color);
        self.king_moves(moves, king, king_square, captures_only);

        let safety = self.evaluate_pins(color.opponent());

        // against two checkers only the king can help: no single move takes two pieces
        // off the board, and blocking one line leaves the other one open
        if safety.checkers.count >= 2 {
            return;
        }

        // the squares a move has to end on to answer the check - the checking piece
        // itself, or any square between it and the king. Not in check: anywhere
        let answers_check = match safety.checkers.first {
            Some(checker) => bit(checker) | ray_between(king_square, checker),
            None => u64::MAX,
        };

        // one turn per piece standing rather than one per square, and the type is the
        // loop's own instead of something to read off the board
        for piece_type in NON_KING_TYPES {
            let piece = Piece::new(piece_type, color);
            let mut pieces = self.piece_board(piece_type, color);

            while pieces != 0 {
                let from = pieces.trailing_zeros() as u8;
                // takes the lowest bit back off, which is the square just read
                pieces &= pieces - 1;

                // a pinned piece is not stuck: it may still move along the line it is
                // pinned on, up to and including the piece that pins it
                let allowed = match safety.pin_ray(from) {
                    Some(pin_ray) => answers_check & pin_ray,
                    None => answers_check,
                };

                // this piece writes onto the end of the list and the moves it may not
                // play are taken off again, which saves a second list to sort them out in
                let start = moves.len();
                self.moves_for_piece(moves, piece, from);

                let mut kept = start;
                for index in start..moves.len() {
                    let candidate = moves[index];

                    // before the legality scan, which is the expensive half
                    if captures_only && candidate.captured.is_none() {
                        continue;
                    }

                    let legal = if candidate.en_passant {
                        // the pawn it takes stands beside the square it ends on, so
                        // neither mask can judge this move - the attack scan has to
                        self.en_passant_is_legal(&candidate, king_square)
                    } else {
                        allowed & bit(candidate.to) != 0
                    };

                    if legal {
                        moves[kept] = candidate;
                        kept += 1;
                    }
                }
                moves.truncate(kept);
            }
        }

        // castling out of check is never allowed; castle_moves itself refuses to castle
        // through or into an attacked square. A castle never takes anything
        if !captures_only && safety.checkers.count == 0 {
            self.castle_moves(moves, color);
        }
    }

    // every move of one side that follows the movement rules, whether or not it leaves
    // the own king in check - only used for boards that hold no king at all
    fn pseudo_legal_moves(&self, color: Color) -> Vec<Move> {
        let mut moves = Vec::with_capacity(48);

        for piece_type in PieceType::ALL {
            for from in self.squares_with(piece_type, color) {
                let piece = Piece::new(piece_type, color);
                self.moves_for_piece(&mut moves, piece, from);
            }
        }
        self.castle_moves(&mut moves, color);

        moves
    }

    // pseudo-legal moves of a single piece, castling aside; appends rather than allocates
    fn moves_for_piece(&self, moves: &mut Vec<Move>, piece: Piece, from: u8) {
        match piece.piece_type() {
            PieceType::Bishop | PieceType::Rook | PieceType::Queen => {
                self.sliding_moves(moves, piece, from)
            }
            PieceType::Knight => self.stepping_moves(moves, piece, from, &KNIGHT_STEPS),
            PieceType::King => self.stepping_moves(moves, piece, from, &KING_STEPS),
            PieceType::Pawn => self.pawn_moves(moves, piece, from),
        }
    }

    // every square the slider reaches, read out of the attack tables and with its own
    // side's pieces taken back off - bishops, rooks and queens
    fn sliding_moves(&self, moves: &mut Vec<Move>, piece: Piece, from: u8) {
        let occupied = self.occupied();
        let reached = match piece.piece_type() {
            PieceType::Bishop => attacks::bishop_attacks(from, occupied),
            PieceType::Rook => attacks::rook_attacks(from, occupied),
            _ => attacks::queen_attacks(from, occupied),
        };

        // what the table leaves in is the first piece on each ray, which is only a move
        // when it belongs to the other side
        let mut targets = reached & !self.occupied_by(piece.color());
        while targets != 0 {
            let to = targets.trailing_zeros() as u8;
            targets &= targets - 1;

            moves.push(Move::normal(from, to, piece, self.piece_at(to)));
        }
    }

    // pieces of `color` lining up with the opposing king, ignoring what stands between -
    // whether each really checks or only pins is sorted out later, in evaluate_pins
    fn possible_king_attackers(&self, color: Color) -> AttackerList {
        let mut attackers = AttackerList::new();
        let Some(king_square) = self.king_square(color.opponent()) else {
            return attackers;
        };

        // a knight either attacks the king or it does not - nothing can come between a
        // jump, so a knight is never an x-ray attacker and never pins anything
        for &step in &KNIGHT_STEPS {
            let Some(square) = offset(king_square, step) else {
                continue;
            };
            if self.is_piece(square, PieceType::Knight, color, Overlay::NONE) {
                attackers.push(Attacker::stepping(square));
            }
        }

        // an attacking pawn stands one rank behind the king square, seen from its own
        // direction of travel, on a neighbouring file - a pawn push is not an attack
        let pawn_step = -color.pawn_direction();
        for file_step in [-1, 1] {
            let Some(square) = offset(king_square, (file_step, pawn_step)) else {
                continue;
            };
            if self.is_piece(square, PieceType::Pawn, color, Overlay::NONE) {
                attackers.push(Attacker::stepping(square));
            }
        }

        // steps over the king's own pieces, which might be pinned; the first enemy piece
        // on the line ends the walk either way
        for (steps, slider) in [
            (&DIAGONAL_STEPS, PieceType::Bishop),
            (&STRAIGHT_STEPS, PieceType::Rook),
        ] {
            for &step in steps {
                for square in ray(king_square, step) {
                    match self.piece_at(square) {
                        None => continue,
                        Some(piece) if piece.color() != color => continue,
                        Some(piece) => {
                            if piece.is(slider) || piece.is(PieceType::Queen) {
                                attackers.push(Attacker::sliding(square, step));
                            }
                            break;
                        }
                    }
                }
            }
        }

        attackers
    }

    // sorts attackers by pieces in between: none = check, one = pinned, two+ = blocked
    fn evaluate_pins(&self, color: Color) -> KingSafety {
        let mut safety = KingSafety::new();
        let Some(king_square) = self.king_square(color.opponent()) else {
            return safety;
        };

        let attackers = self.possible_king_attackers(color);
        for attacker in attackers.iter() {
            let Some(direction) = attacker.direction else {
                // a knight or a pawn attacks from a square nothing can be blocked on
                safety.checkers.push(attacker.square);
                continue;
            };

            // whatever stands in the way belongs to the king's side: the scan above
            // stopped at the first piece of the attacking color
            let mut blocker = None;
            let mut blocked = false;
            for square in ray(king_square, direction) {
                if square == attacker.square {
                    break;
                }
                if self.piece_at(square).is_some() {
                    if blocker.is_some() {
                        blocked = true;
                        break;
                    }
                    blocker = Some(square);
                }
            }

            if blocked {
                continue;
            }

            // king up to and including the attacker: where check is blocked or the pin held
            let line = bit(attacker.square) | ray_between(king_square, attacker.square);

            match blocker {
                None => safety.checkers.push(attacker.square),
                Some(pinned) => safety.add_pin(pinned, line),
            }
        }

        safety
    }

    // applies each step once instead of sliding - knights and kings
    fn stepping_moves(&self, moves: &mut Vec<Move>, piece: Piece, from: u8, steps: &[(i8, i8)]) {
        for &step in steps {
            let Some(to) = offset(from, step) else {
                continue;
            };

            match self.piece_at(to) {
                None => moves.push(Move::normal(from, to, piece, None)),
                Some(occupant) if occupant.color() != piece.color() => {
                    moves.push(Move::normal(from, to, piece, Some(occupant)));
                }
                Some(_) => {}
            }
        }
    }

    // king steps onto squares no enemy piece covers; scanned with the king lifted off
    // the board, or it would block the very ray it is stepping out of
    fn king_moves(&self, moves: &mut Vec<Move>, king: Piece, from: u8, captures_only: bool) {
        let enemy = king.color().opponent();
        let overlay = Overlay::vacating(from);

        for &step in &KING_STEPS {
            let Some(to) = offset(from, step) else {
                continue;
            };

            let occupant = self.piece_at(to);
            if matches!(occupant, Some(piece) if piece.color() == king.color()) {
                continue;
            }
            // before the attack scan, the expensive part of a king step
            if captures_only && occupant.is_none() {
                continue;
            }
            if self.is_attacked_over(to, enemy, overlay) {
                continue;
            }

            moves.push(Move::normal(from, to, king, occupant));
        }
    }

    // two pawns leave the same rank at once, which no pin scan sees - scanned directly
    fn en_passant_is_legal(&self, candidate: &Move, king_square: u8) -> bool {
        let color = candidate.piece.color();
        let overlay = Overlay {
            vacated: bit(candidate.from) | bit(en_passant_captured_square(candidate.to, color)),
            filled: Some((candidate.to, candidate.piece)),
        };

        !self.is_attacked_over(king_square, color.opponent(), overlay)
    }

    // one or two squares forward onto empty squares, diagonal captures, en passant,
    // and every promotion choice on the last rank
    fn pawn_moves(&self, moves: &mut Vec<Move>, piece: Piece, from: u8) {
        let color = piece.color();
        let direction = color.pawn_direction();

        // a pawn can only push onto an empty square, and only push twice from its start
        // rank and only when the square it steps over is empty as well
        let one_forward = offset(from, (0, direction)).filter(|&to| self.piece_at(to).is_none());
        if let Some(one_forward) = one_forward {
            push_pawn_move(moves, from, one_forward, piece, None);

            if rank_of(from) == color.pawn_start_rank() {
                let two_forward =
                    offset(from, (0, direction * 2)).filter(|&to| self.piece_at(to).is_none());
                if let Some(two_forward) = two_forward {
                    moves.push(Move::normal(from, two_forward, piece, None));
                }
            }
        }

        for file_step in [-1, 1] {
            let Some(to) = offset(from, (file_step, direction)) else {
                continue;
            };

            match self.piece_at(to) {
                Some(occupant) if occupant.color() != color => {
                    push_pawn_move(moves, from, to, piece, Some(occupant));
                }
                Some(_) => {}
                // the rank check makes sure only the side to move can take en passant
                None if self.en_passant_target() == Some(to)
                    && rank_of(to) == color.en_passant_rank() =>
                {
                    let captured_square = en_passant_captured_square(to, color);
                    if let Some(captured_pawn) = self.piece_at(captured_square) {
                        moves.push(Move::en_passant_capture(from, to, piece, captured_pawn));
                    }
                }
                None => {}
            }
        }
    }

    // the castling moves of one side
    fn castle_moves(&self, moves: &mut Vec<Move>, color: Color) {
        let king_from = king_start_square(color);

        let king = match self.piece_at(king_from) {
            Some(piece) if piece.is(PieceType::King) && piece.color() == color => piece,
            _ => return,
        };

        for side in CastleSide::BOTH {
            if !self.castling_rights().get(color, side) {
                continue;
            }

            let rook_from = rook_start_square(color, side);
            let rook_stands_there = match self.piece_at(rook_from) {
                Some(piece) => piece.is(PieceType::Rook) && piece.color() == color,
                None => false,
            };
            if !rook_stands_there {
                continue;
            }

            // every square between king and rook has to be empty
            if squares_between(king_from, rook_from).any(|square| self.piece_at(square).is_some()) {
                continue;
            }

            // the king may not stand on, cross, or land on an attacked square
            // (the crossed square is exactly where the rook ends up)
            let king_to = king_castle_square(color, side);
            let rook_to = rook_castle_square(color, side);
            if [king_from, rook_to, king_to]
                .iter()
                .any(|&square| self.is_attacked(square, color.opponent()))
            {
                continue;
            }

            moves.push(Move::castling(king, king_from, king_to, side));
        }
    }

    // is the given square attacked - looks outward from the square rather than
    // generating every move of that side, so it allocates nothing
    pub(crate) fn is_attacked(&self, square: u8, color: Color) -> bool {
        self.is_attacked_over(square, color, Overlay::NONE)
    }

    // the same question, asked of the board as an overlay leaves it
    fn is_attacked_over(&self, square: u8, color: Color, overlay: Overlay) -> bool {
        // an attacking pawn stands one rank behind this square, seen from its own
        // direction of travel, on a neighbouring file - a pawn push is not an attack
        let pawn_step = -color.pawn_direction();
        for file_step in [-1, 1] {
            if self.has_piece_at(
                offset(square, (file_step, pawn_step)),
                PieceType::Pawn,
                color,
                overlay,
            ) {
                return true;
            }
        }

        for &step in &KNIGHT_STEPS {
            if self.has_piece_at(offset(square, step), PieceType::Knight, color, overlay) {
                return true;
            }
        }

        for &step in &KING_STEPS {
            if self.has_piece_at(offset(square, step), PieceType::King, color, overlay) {
                return true;
            }
        }

        // in each direction only the first piece can attack, everything behind it is blocked
        self.attacked_by_slider(square, &DIAGONAL_STEPS, PieceType::Bishop, color, overlay)
            || self.attacked_by_slider(square, &STRAIGHT_STEPS, PieceType::Rook, color, overlay)
    }

    // is there a piece of that type and color on that square
    fn is_piece(&self, square: u8, piece_type: PieceType, color: Color, overlay: Overlay) -> bool {
        match self.piece_at_over(square, overlay) {
            Some(piece) => piece.is(piece_type) && piece.color() == color,
            None => false,
        }
    }

    // the same for a square that may lie off the board, where None counts as no piece,
    // so callers don't have to check bounds
    fn has_piece_at(
        &self,
        square: Option<u8>,
        piece_type: PieceType,
        color: Color,
        overlay: Overlay,
    ) -> bool {
        matches!(square, Some(square) if self.is_piece(square, piece_type, color, overlay))
    }

    // walks each direction until it runs into a piece - true when that piece is a queen
    // or the given slider (bishop for the diagonals, rook for the straight lines)
    fn attacked_by_slider(
        &self,
        square: u8,
        steps: &[(i8, i8)],
        slider: PieceType,
        color: Color,
        overlay: Overlay,
    ) -> bool {
        steps.iter().any(|&step| {
            for target in ray(square, step) {
                if let Some(piece) = self.piece_at_over(target, overlay) {
                    return piece.color() == color
                        && (piece.is(slider) || piece.is(PieceType::Queen));
                }
            }
            false
        })
    }

    // the piece standing on a square once the overlay is taken into account
    fn piece_at_over(&self, square: u8, overlay: Overlay) -> Option<Piece> {
        if let Some((filled_square, piece)) = overlay.filled {
            if filled_square == square {
                return Some(piece);
            }
        }
        if overlay.vacated & bit(square) != 0 {
            return None;
        }

        self.piece_at(square)
    }
}

// the board as a move would leave it, for the two moves no mask can judge: a king
// step and an en passant capture
#[derive(Clone, Copy)]
struct Overlay {
    vacated: u64,
    filled: Option<(u8, Piece)>,
}

impl Overlay {
    // the board as it stands
    const NONE: Overlay = Overlay {
        vacated: 0,
        filled: None,
    };

    fn vacating(square: u8) -> Overlay {
        Overlay {
            vacated: bit(square),
            filled: None,
        }
    }
}

// a piece lining up with the king, whatever stands in between
#[derive(Clone, Copy)]
struct Attacker {
    square: u8,
    // the step leading from the king towards this piece, None for a knight or a pawn:
    // those attack from a fixed square and nothing can be put in their way
    direction: Option<(i8, i8)>,
}

impl Attacker {
    fn stepping(square: u8) -> Attacker {
        Attacker {
            square,
            direction: None,
        }
    }

    fn sliding(square: u8, direction: (i8, i8)) -> Attacker {
        Attacker {
            square,
            direction: Some(direction),
        }
    }
}

// how many pieces can line up with one king at once: one per line, plus the eight
// knight squares and the two squares a pawn can attack from
const MAX_ATTACKERS: usize = 18;

// a list of a fixed size, so that the scan around the king allocates nothing
struct AttackerList {
    items: [Attacker; MAX_ATTACKERS],
    len: usize,
}

impl AttackerList {
    fn new() -> AttackerList {
        AttackerList {
            items: [Attacker::stepping(0); MAX_ATTACKERS],
            len: 0,
        }
    }

    fn push(&mut self, attacker: Attacker) {
        debug_assert!(
            self.len < MAX_ATTACKERS,
            "more attackers than there are lines to the king"
        );
        self.items[self.len] = attacker;
        self.len += 1;
    }

    fn iter(&self) -> impl Iterator<Item = &Attacker> {
        self.items[..self.len].iter()
    }
}

// the pieces that really give check right now
#[derive(Clone, Copy)]
struct Checkers {
    count: usize,
    // only the first one is ever needed: a single check has to be answered on its own
    // line, and against two checkers nothing but a king move helps anyway
    first: Option<u8>,
}

impl Checkers {
    fn push(&mut self, square: u8) {
        self.count += 1;
        self.first.get_or_insert(square);
    }
}

// a piece that may not leave the line between the king and the piece behind it
#[derive(Clone, Copy)]
struct Pin {
    square: u8,
    // the line from the king up to and including the pinning piece: every square the
    // pinned piece may still move to, the pinner itself included - it can be taken
    ray: u64,
}

// only one piece per line can be pinned, and there are eight lines
const MAX_PINS: usize = 8;

// what the scan around the king found: who checks it, and who is pinned to it
struct KingSafety {
    checkers: Checkers,
    pins: [Pin; MAX_PINS],
    pin_count: usize,
}

impl KingSafety {
    fn new() -> KingSafety {
        KingSafety {
            checkers: Checkers {
                count: 0,
                first: None,
            },
            pins: [Pin { square: 0, ray: 0 }; MAX_PINS],
            pin_count: 0,
        }
    }

    fn add_pin(&mut self, square: u8, ray: u64) {
        debug_assert!(
            self.pin_count < MAX_PINS,
            "more pins than there are lines to the king"
        );
        self.pins[self.pin_count] = Pin { square, ray };
        self.pin_count += 1;
    }

    // the squares a piece may still move to because it is pinned, None when it is free
    fn pin_ray(&self, square: u8) -> Option<u64> {
        self.pins[..self.pin_count]
            .iter()
            .find(|pin| pin.square == square)
            .map(|pin| pin.ray)
    }
}

// the squares strictly between two squares that share a rank, file or diagonal - empty
// when the two do not line up at all, as with a checking knight
fn ray_between(from: u8, to: u8) -> u64 {
    let Some(step) = direction_between(from, to) else {
        return 0;
    };

    let mut squares = 0;
    for square in ray(from, step) {
        if square == to {
            break;
        }
        squares |= bit(square);
    }

    squares
}

// adds a pawn move, split into one move per promotion choice when it ends on the last
// rank - so every promotion is its own move in the move list
fn push_pawn_move(moves: &mut Vec<Move>, from: u8, to: u8, piece: Piece, captured: Option<Piece>) {
    if rank_of(to) != piece.color().promotion_rank() {
        moves.push(Move::normal(from, to, piece, captured));
        return;
    }

    for promote_to in PieceType::PROMOTION_CHOICES {
        moves.push(Move::promoting(from, to, piece, captured, promote_to));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // an otherwise empty board holding the given pieces, white to move
    fn board_with(pieces: &[(PieceType, Color, u8)]) -> Board {
        let mut board = Board::new();
        for &(piece_type, color, square) in pieces {
            board.add_piece(Piece::new(piece_type, color), square);
        }
        board
    }

    // white king e1, white rook e2, black rook e8: the white rook is pinned, but a pin
    // is not a ban - it may still move along the line, up to the rook that pins it
    #[test]
    fn a_pinned_rook_still_moves_along_the_pin() {
        let board = board_with(&[
            (PieceType::King, Color::White, 4),
            (PieceType::Rook, Color::White, 12),
            (PieceType::Rook, Color::Black, 60),
        ]);

        let mut targets: Vec<u8> = board
            .legal_moves()
            .into_iter()
            .filter(|candidate| candidate.from == 12)
            .map(|candidate| candidate.to)
            .collect();
        targets.sort();

        // e3 up to e8, the black rook included, and nothing off the e file
        assert_eq!(targets, vec![20, 28, 36, 44, 52, 60]);
    }

    // white king e1, black rook e8 and black knight d3: taking one checker still leaves
    // the other one, so only the king may move (to f1 or d2)
    #[test]
    fn a_double_check_leaves_only_king_moves() {
        let board = board_with(&[
            (PieceType::King, Color::White, 4),
            (PieceType::Rook, Color::White, 3),
            (PieceType::Rook, Color::Black, 60),
            (PieceType::Knight, Color::Black, 19),
        ]);

        let moves = board.legal_moves();

        assert!(moves.iter().all(|candidate| candidate.from == 4));
        assert_eq!(moves.len(), 2);
    }

    // white king e4, black rook e8: the king may not run down the file it is checked
    // on - it does not block that line while it is standing on it
    #[test]
    fn the_king_cannot_step_backwards_out_of_a_check() {
        let board = board_with(&[
            (PieceType::King, Color::White, 28),
            (PieceType::Rook, Color::Black, 60),
        ]);

        let targets: Vec<u8> = board
            .legal_moves()
            .into_iter()
            .map(|candidate| candidate.to)
            .collect();

        assert!(!targets.contains(&20), "e3 is still on the rook's line");
        assert!(!targets.contains(&36), "e5 is still on the rook's line");
        assert_eq!(targets.len(), 6);
    }

    #[test]
    fn castling_out_of_check_is_not_allowed() {
        let board = board_with(&[
            (PieceType::King, Color::White, 4),
            (PieceType::Rook, Color::White, 7),
            (PieceType::Rook, Color::Black, 60),
        ]);

        let moves = board.legal_moves();

        assert!(moves.iter().all(|candidate| candidate.castle.is_none()));
        assert_eq!(moves.len(), 4);
    }

    // with `with_rook`, a rook on h5 waits on the rank both pawns leave at once
    fn en_passant_position(with_rook: bool) -> Board {
        let mut pieces = vec![
            (PieceType::King, Color::White, 32),
            (PieceType::Pawn, Color::White, 33),
            (PieceType::Pawn, Color::White, 14),
            (PieceType::King, Color::Black, 63),
            (PieceType::Pawn, Color::Black, 50),
        ];
        if with_rook {
            pieces.push((PieceType::Rook, Color::Black, 39));
        }

        let mut board = board_with(&pieces);
        board.make_move_from_squares(14, 22, None); // g2g3, a waiting move
        board.make_move_from_squares(50, 34, None); // c7c5, opening the en passant square
        board
    }

    #[test]
    fn en_passant_that_uncovers_the_king_is_refused() {
        let moves = en_passant_position(true).legal_moves();

        assert!(!moves.is_empty());
        assert!(moves.iter().all(|candidate| !candidate.en_passant));
    }

    #[test]
    fn en_passant_is_allowed_when_no_rook_waits_on_the_rank() {
        let moves = en_passant_position(false).legal_moves();
        let capture = moves.iter().find(|candidate| candidate.en_passant);

        assert_eq!(capture.map(|candidate| candidate.to), Some(42));
    }
}
