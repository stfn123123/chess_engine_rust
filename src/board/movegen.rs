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
use crate::board::castling::{
    CastleSide, king_castle_square, king_start_square, rook_castle_square, rook_start_square,
};
use crate::board::chess_move::Move;
use crate::board::piece::{Color, Piece, PieceType};
use crate::board::square::{
    DIAGONAL_STEPS, KING_STEPS, KNIGHT_STEPS, STRAIGHT_STEPS, direction_between,
    en_passant_captured_square, offset, rank_of, ray, squares_between,
};

impl Board {
    // every legal move of one side, castling included
    pub(crate) fn legal_moves_for(&self, color: Color) -> Vec<Move> {
        // a position without a king only comes from a hand built board; there is no
        // king to keep safe, so every pseudo-legal move is as legal as it gets
        let Some(king_square) = self.king_square(color) else {
            return self.pseudo_legal_moves(color);
        };

        let mut moves = Vec::new();
        let king = Piece::new(PieceType::King, color);
        self.king_moves(&mut moves, king, king_square);

        let safety = self.evaluate_pins(color.opponent());

        // against two checkers only the king can help: no single move takes two pieces
        // off the board, and blocking one line leaves the other one open
        if safety.checkers.count >= 2 {
            return moves;
        }

        // the squares a move has to end on to answer the check - the checking piece
        // itself, or any square between it and the king. Not in check: anywhere
        let answers_check = match safety.checkers.first {
            Some(checker) => bit(checker) | ray_between(king_square, checker),
            None => u64::MAX,
        };

        // walking the squares once and dispatching on what stands there beats asking
        // for the squares of all six piece types one after the other
        for from in 0..64u8 {
            let Some(piece) = self.piece_at(from) else {
                continue;
            };
            if piece.color() != color || piece.is(PieceType::King) {
                continue;
            }

            // a pinned piece is not stuck: it may still move along the line it is
            // pinned on, up to and including the piece that pins it
            let allowed = match safety.pin_ray(from) {
                Some(pin_ray) => answers_check & pin_ray,
                None => answers_check,
            };

            for candidate in self.moves_for_piece(piece, from) {
                let legal = if candidate.en_passant {
                    // the pawn it takes stands beside the square it ends on, so neither
                    // mask can judge this move - the attack scan has to
                    self.en_passant_is_legal(&candidate, king_square)
                } else {
                    allowed & bit(candidate.to) != 0
                };

                if legal {
                    moves.push(candidate);
                }
            }
        }

        // castling out of check is never allowed; castle_moves itself refuses to castle
        // through or into an attacked square
        if safety.checkers.count == 0 {
            moves.extend(self.castle_moves(color));
        }

        moves
    }

    // every move of one side that follows the movement rules, whether or not it leaves
    // the own king in check - only used for boards that hold no king at all
    fn pseudo_legal_moves(&self, color: Color) -> Vec<Move> {
        let mut moves = Vec::new();

        for piece_type in PieceType::ALL {
            for from in self.squares_with(piece_type, color) {
                let piece = Piece::new(piece_type, color);
                moves.extend(self.moves_for_piece(piece, from));
            }
        }
        moves.extend(self.castle_moves(color));

        moves
    }

    // the pseudo-legal moves of a single piece, castling aside
    fn moves_for_piece(&self, piece: Piece, from: u8) -> Vec<Move> {
        match piece.piece_type() {
            PieceType::Bishop => self.sliding_moves(piece, from, &DIAGONAL_STEPS),
            PieceType::Rook => self.sliding_moves(piece, from, &STRAIGHT_STEPS),
            PieceType::Queen => {
                let mut moves = self.sliding_moves(piece, from, &DIAGONAL_STEPS);
                moves.extend(self.sliding_moves(piece, from, &STRAIGHT_STEPS));
                moves
            }
            PieceType::Knight => self.stepping_moves(piece, from, &KNIGHT_STEPS),
            PieceType::King => self.stepping_moves(piece, from, &KING_STEPS),
            PieceType::Pawn => self.pawn_moves(piece, from),
        }
    }

    // walks in each direction until the edge of the board, a friendly piece, or a
    // capture is hit - bishops, rooks and queens
    fn sliding_moves(&self, piece: Piece, from: u8, steps: &[(i8, i8)]) -> Vec<Move> {
        let mut moves = Vec::new();

        for &step in steps {
            for to in ray(from, step) {
                match self.piece_at(to) {
                    None => moves.push(Move::normal(from, to, piece, None)),
                    Some(occupant) => {
                        if occupant.color() != piece.color() {
                            moves.push(Move::normal(from, to, piece, Some(occupant)));
                        }
                        // blocked, the rest of this direction is out of reach
                        break;
                    }
                }
            }
        }

        moves
    }

    // every piece of `color` that lines up with the opposing king, with whatever stands
    // in between ignored: "who would give check if the board were empty in between".
    // color = White means every white piece pointing at the black king.
    // Whether such a piece really checks the king or only pins a piece to it is decided
    // in evaluate_pins, which counts what stands in the way
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

        // along each line the pieces of the king's own side are stepped over, since
        // those are exactly the pieces that might turn out to be pinned. The first
        // piece of the attacking color ends the walk either way: anything behind it is
        // blocked by a piece that can never be pinned to this king
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

    // sorts the possible attackers by how many pieces stand between them and the king:
    // - no piece:    the attack goes through, the king is in check
    // - one piece:   that piece is pinned to the king
    // - two or more: the line is blocked for good, the attacker does nothing
    // Both answers fall out of the same walk, so both are returned: the pieces that
    // really check the king, and the pinned ones with the line each is pinned on
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

            // the line from the king up to and including the attacker: where a check
            // can be blocked or the checking piece taken, and the only stretch a pinned
            // piece may move along without opening the line
            let line = bit(attacker.square) | ray_between(king_square, attacker.square);

            match blocker {
                None => safety.checkers.push(attacker.square),
                Some(pinned) => safety.add_pin(pinned, line),
            }
        }

        safety
    }

    // applies each step once instead of sliding - knights and kings
    fn stepping_moves(&self, piece: Piece, from: u8, steps: &[(i8, i8)]) -> Vec<Move> {
        let mut moves = Vec::new();

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

        moves
    }

    // the king steps onto every square around it that no enemy piece covers
    // no mask can be worked out for the king beforehand: it is the piece the whole scan
    // is about, and every step changes what the enemy reaches. The scan runs with the
    // king lifted off the board, otherwise it would block the very ray it is trying to
    // step out of and stepping straight backwards would look safe
    fn king_moves(&self, moves: &mut Vec<Move>, king: Piece, from: u8) {
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
            if self.is_attacked_over(to, enemy, overlay) {
                continue;
            }

            moves.push(Move::normal(from, to, king, occupant));
        }
    }

    // an en passant capture takes a pawn off a square it does not end on, and two pawns
    // leave the same rank at once - a rook waiting on that rank can be uncovered that
    // way, which no pin scan ever saw. So the position the capture leads to is scanned
    // directly; en passant is rare enough for that to cost nothing
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
    fn pawn_moves(&self, piece: Piece, from: u8) -> Vec<Move> {
        let color = piece.color();
        let direction = color.pawn_direction();
        let mut moves = Vec::new();

        // a pawn can only push onto an empty square, and only push twice from its start
        // rank and only when the square it steps over is empty as well
        let one_forward = offset(from, (0, direction)).filter(|&to| self.piece_at(to).is_none());
        if let Some(one_forward) = one_forward {
            push_pawn_move(&mut moves, from, one_forward, piece, None);

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
                    push_pawn_move(&mut moves, from, to, piece, Some(occupant));
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

        moves
    }

    // the castling moves of one side
    fn castle_moves(&self, color: Color) -> Vec<Move> {
        let mut moves = Vec::new();
        let king_from = king_start_square(color);

        let king = match self.piece_at(king_from) {
            Some(piece) if piece.is(PieceType::King) && piece.color() == color => piece,
            _ => return moves,
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

        moves
    }

    // is the given square attacked by any piece of the given color
    // this looks outward from the square ("what could reach me from here") instead of
    // generating every move of that side, so it allocates nothing and stops at the
    // first attacker it finds
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

// the board as a move would leave it: the squares in `vacated` are treated as empty and
// `filled` as holding a piece. Only the attack scan uses it, and only for the two moves
// no mask can judge - a king step and an en passant capture
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

// a square as a single bit, so that a set of squares fits into one number
fn bit(square: u8) -> u64 {
    1 << square
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

    // white king a5, white pawn b5, black pawn arriving on c5, and with `with_rook` a
    // black rook on h5: capturing en passant takes both pawns off the fifth rank at
    // once, which is the one line no pin scan can see coming
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
