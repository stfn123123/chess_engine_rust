// The board: where the pieces stand, whose turn it is, and how a move is played
// and taken back again. Move generation itself lives in `movegen`.

use crate::board::castling::{CastleSide, CastlingRights, rook_castle_square, rook_start_square};
use crate::board::chess_move::{Move, MoveRecord};
use crate::board::piece::{Color, Piece, PieceType};
use crate::board::square::{en_passant_captured_square, file_of, offset, rank_of};
use crate::board::zobrist::ZOBRIST;

// cloned to put a position aside and come back to it later, history and all - the
// GUI stores test positions that way
#[derive(Clone)]
pub struct Board {
    turn: Color,
    squares: [Option<Piece>; 64],
    // where each king stands, indexed by Color::index - kept here because the move
    // generator asks for it several times per position
    king_squares: [Option<u8>; 2],
    // every move played so far, together with what is needed to take it back
    history: Vec<MoveRecord>,
    // the key of every position a move was played in, oldest first - the same order as
    // `history`, kept apart from it so the repetition scan walks nothing but keys
    position_keys: Vec<u64>,
    // plies since the last capture or pawn move: the 50-move counter, and also how far
    // back a repetition could possibly reach
    halfmove_clock: u16,
    castling_rights: CastlingRights,
    // the square a pawn skipped over with its double push, i.e. the square an enemy
    // pawn may capture onto right now - only valid for the immediately following move
    en_passant_target: Option<u8>,
    // the zobrist hash of the current position: pieces, side to move, castling rights
    // and the en passant option, kept up to date move by move instead of recomputed
    hash: u64,
    // the en passant key that is currently mixed into `hash`, 0 when there is none
    // remembered because whether a target is capturable can change with the position
    en_passant_hash: u64,
    // the phase weights of everything standing, added up - kept here because only a
    // capture or a promotion can change it
    phase: i32,
}

// -------------------- setting up --------------------
impl Board {
    pub fn new() -> Board {
        let castling_rights = CastlingRights::ALL;

        Board {
            turn: Color::White,
            squares: [None; 64],
            king_squares: [None; 2],
            history: Vec::new(),
            position_keys: Vec::new(),
            halfmove_clock: 0,
            castling_rights,
            en_passant_target: None,
            // white to move and no en passant target contribute nothing
            hash: ZOBRIST.castling(castling_rights),
            en_passant_hash: 0,
            // an empty board has nothing standing on it
            phase: 0,
        }
    }

    pub fn add_piece(&mut self, piece: Piece, square: u8) {
        self.set_square(square, Some(piece));
    }

    // the usual starting position
    pub fn set_start_position(&mut self) {
        let back_rank = [
            PieceType::Rook,
            PieceType::Knight,
            PieceType::Bishop,
            PieceType::Queen,
            PieceType::King,
            PieceType::Bishop,
            PieceType::Knight,
            PieceType::Rook,
        ];

        for (file, piece_type) in back_rank.into_iter().enumerate() {
            self.add_piece(Piece::new(piece_type, Color::White), file as u8);
            self.add_piece(Piece::new(piece_type, Color::Black), 56 + file as u8);
        }

        for file in 0..8 {
            self.add_piece(Piece::new(PieceType::Pawn, Color::White), 8 + file);
            self.add_piece(Piece::new(PieceType::Pawn, Color::Black), 48 + file);
        }

        self.set_castling_rights(CastlingRights::ALL);
        self.set_en_passant_target(None);
    }
}

// -------------------- reading the position --------------------
impl Board {
    pub fn turn(&self) -> Color {
        self.turn
    }

    // the zobrist hash of the current position
    #[allow(dead_code)]
    pub fn hash(&self) -> u64 {
        self.hash
    }

    pub fn castling_rights(&self) -> CastlingRights {
        self.castling_rights
    }

    pub fn en_passant_target(&self) -> Option<u8> {
        self.en_passant_target
    }

    // the piece standing on a square, if any
    pub fn piece_at(&self, square: u8) -> Option<Piece> {
        self.squares[square as usize]
    }

    // where the king of that side stands - None only on a hand built board
    pub(crate) fn king_square(&self, color: Color) -> Option<u8> {
        self.king_squares[color.index()]
    }

    // the raw count, so a promotion can push it past a full board
    pub fn phase(&self) -> i32 {
        self.phase
    }

    // every square holding a piece of that type and color
    pub fn squares_with(
        &self,
        piece_type: PieceType,
        color: Color,
    ) -> impl Iterator<Item = u8> + '_ {
        self.squares
            .iter()
            .enumerate()
            .filter_map(move |(index, square)| {
                let piece = (*square)?;
                (piece.is(piece_type) && piece.color() == color).then_some(index as u8)
            })
    }

    // every move played so far, oldest first - a position is written down as the moves
    // that lead to it, so replaying them brings it back with its history intact
    pub fn moves_played(&self) -> Vec<Move> {
        self.history.iter().map(|record| record.chess_move).collect()
    }

    #[allow(dead_code)]
    pub fn display(&self) {
        for rank in (0..8).rev() {
            for file in 0..8 {
                let symbol = match self.squares[rank * 8 + file] {
                    Some(piece) => piece.symbol(),
                    None => '.',
                };
                print!("{symbol} ");
            }
            println!();
        }
    }
}

// -------------------- playing moves --------------------
impl Board {
    // plays a move and pushes it onto the history, so undo_move can take it back
    // the move is taken as given: whatever it says about castling, promotion and en
    // passant is what happens, which is what the generator in `movegen` filled in
    pub fn make_move(&mut self, chess_move: &Move) {
        let record = MoveRecord {
            chess_move: *chess_move,
            castling_rights_before: self.castling_rights,
            en_passant_before: self.en_passant_target,
            halfmove_clock_before: self.halfmove_clock,
        };
        // read before anything changes: this is the position as it was
        let hash_before = self.hash;

        let Move {
            from,
            to,
            piece,
            castle,
            promotion,
            en_passant,
            ..
        } = *chess_move;
        let color = piece.color();

        // the captured pawn does not stand on `to`, so it has to be taken off separately
        if en_passant {
            self.set_square(en_passant_captured_square(to, color), None);
        }

        // the pawn only reaches the board as its promoted piece, the Move keeps the pawn
        let arriving = match promotion {
            Some(piece_type) => Piece::new(piece_type, color),
            None => piece,
        };
        self.set_square(to, Some(arriving));
        self.set_square(from, None);

        if let Some(side) = castle {
            let rook_from = rook_start_square(color, side);
            let rook_to = rook_castle_square(color, side);
            let rook = self.piece_at(rook_from);
            self.set_square(rook_to, rook);
            self.set_square(rook_from, None);
        }

        let mut castling_rights = self.castling_rights;
        if piece.is(PieceType::King) {
            castling_rights.clear_color(color);
        }
        // covers both a rook leaving its start square and a rook being captured on it
        castling_rights.clear_on_square(from);
        castling_rights.clear_on_square(to);
        self.set_castling_rights(castling_rights);

        self.history.push(record);
        self.position_keys.push(hash_before);
        // a capture or a pawn move resets it, and with it the window a repetition or
        // the 50-move rule can reach back over
        self.halfmove_clock = if chess_move.is_irreversible() {
            0
        } else {
            self.halfmove_clock + 1
        };
        self.flip_turn();

        // last, because whether the target is capturable depends on the finished
        // position and on the side that is to move now
        // only a double push opens a target, every other move closes it
        let target = if piece.is(PieceType::Pawn) && (to as i8 - from as i8).abs() == 16 {
            Some((from + to) / 2)
        } else {
            None
        };
        self.set_en_passant_target(target);

        debug_assert_eq!(self.hash, self.full_hash(), "incremental hash drifted");
        debug_assert_eq!(
            self.king_squares,
            self.searched_king_squares(),
            "king squares drifted"
        );
        debug_assert_eq!(self.phase, self.counted_phase(), "phase drifted");
        debug_assert_eq!(
            self.position_keys.len(),
            self.history.len(),
            "key stack drifted"
        );
        debug_assert_eq!(
            self.halfmove_clock,
            self.counted_halfmove_clock(),
            "halfmove clock drifted"
        );
    }

    // plays the move going from one square to another, for callers that only have the
    // two squares (the GUI); `promotion` is the piece a promoting pawn turns into
    pub fn make_move_from_squares(&mut self, from: u8, to: u8, promotion: Option<PieceType>) {
        let chess_move = self.describe_move(from, to, promotion);
        self.make_move(&chess_move);
    }

    // undoes the last move (pop from the history, restore the captured piece and the
    // castling rights, move a castled rook back, flip turn back)
    // a promotion needs no extra work: `piece` is still the pawn, so putting it back
    // on `from` removes the promoted piece from the board
    pub fn undo_move(&mut self) -> Option<Move> {
        let record = self.history.pop()?;
        self.position_keys.pop();
        self.halfmove_clock = record.halfmove_clock_before;

        let chess_move = record.chess_move;
        let color = chess_move.piece.color();

        self.set_square(chess_move.from, Some(chess_move.piece));
        if chess_move.en_passant {
            // the captured pawn never stood on `to`
            let captured_square = en_passant_captured_square(chess_move.to, color);
            self.set_square(chess_move.to, None);
            self.set_square(captured_square, chess_move.captured);
        } else {
            self.set_square(chess_move.to, chess_move.captured);
        }

        if let Some(side) = chess_move.castle {
            let rook_from = rook_start_square(color, side);
            let rook_to = rook_castle_square(color, side);
            let rook = self.piece_at(rook_to);
            self.set_square(rook_from, rook);
            self.set_square(rook_to, None);
        }

        self.set_castling_rights(record.castling_rights_before);
        self.flip_turn();
        // same as in make_move: the target is restored last, once the position and
        // the side to move are back to what they were
        self.set_en_passant_target(record.en_passant_before);

        debug_assert_eq!(self.hash, self.full_hash(), "incremental hash drifted");
        debug_assert_eq!(
            self.king_squares,
            self.searched_king_squares(),
            "king squares drifted"
        );
        debug_assert_eq!(self.phase, self.counted_phase(), "phase drifted");
        debug_assert_eq!(
            self.position_keys.len(),
            self.history.len(),
            "key stack drifted"
        );
        debug_assert_eq!(
            self.halfmove_clock,
            self.counted_halfmove_clock(),
            "halfmove clock drifted"
        );

        Some(chess_move)
    }

    // works out what kind of move going from `from` to `to` is:
    // a king moving two files is taken as a castle, a pawn reaching the last rank is
    // taken as a promotion (defaulting to a queen), and a pawn moving diagonally onto
    // an empty square is taken as an en passant capture
    fn describe_move(&self, from: u8, to: u8, promotion: Option<PieceType>) -> Move {
        let piece = self.piece_at(from).expect("no piece on the from square");
        let color = piece.color();

        if piece.is(PieceType::King) && (to as i8 - from as i8).abs() == 2 {
            let side = if to > from {
                CastleSide::King
            } else {
                CastleSide::Queen
            };
            return Move::castling(piece, from, to, side);
        }

        if piece.is(PieceType::Pawn) {
            if file_of(from) != file_of(to) && self.piece_at(to).is_none() {
                let captured = self
                    .piece_at(en_passant_captured_square(to, color))
                    .expect("no pawn to capture en passant");
                return Move::en_passant_capture(from, to, piece, captured);
            }

            if rank_of(to) == color.promotion_rank() {
                let promote_to = promotion.unwrap_or(PieceType::Queen);
                return Move::promoting(from, to, piece, self.piece_at(to), promote_to);
            }
        }

        Move::normal(from, to, piece, self.piece_at(to))
    }
}

// -------------------- how the game ends --------------------
impl Board {
    // every move the side to move may play
    // the generator in `movegen` works these out from the position alone, so nothing
    // has to be played and taken back again here
    pub fn legal_moves(&self) -> Vec<Move> {
        self.legal_moves_for(self.turn)
    }

    // only the moves that take something, for the quiescence search
    pub fn legal_captures(&self) -> Vec<Move> {
        self.legal_captures_for(self.turn)
    }

    // is the king of the given side in check
    pub fn is_check(&self, color: Color) -> bool {
        match self.king_square(color) {
            Some(king_square) => self.is_attacked(king_square, color.opponent()),
            None => false,
        }
    }

    // the side to move is in check and has no legal moves left
    pub fn is_checkmate(&self) -> bool {
        self.is_check(self.turn) && self.legal_moves().is_empty()
    }

    // the side to move is not in check but has no legal moves left
    pub fn is_stalemate(&self) -> bool {
        !self.is_check(self.turn) && self.legal_moves().is_empty()
    }

    // the winning side, only meaningful right after is_checkmate returned true
    pub fn winner(&self) -> Color {
        if self.is_check(Color::White) {
            Color::Black
        } else {
            Color::White
        }
    }

    // the keys of the positions the current one could still repeat: back to the last
    // capture or pawn move, which no earlier position can survive
    // the halfmove clock is that distance already, so nothing has to be walked for it
    fn reversible_keys(&self) -> &[u64] {
        let window = (self.halfmove_clock as usize).min(self.position_keys.len());
        &self.position_keys[self.position_keys.len() - window..]
    }

    // the positions worth comparing against, paired with how many plies back they are
    // only every second one can match: a position repeats with the same side to move,
    // and the side to move is part of the key
    // it starts four plies back because that is the shortest way back to a position -
    // both sides have to move a piece and move it back again
    fn repetition_candidates(&self) -> impl Iterator<Item = (usize, u64)> + '_ {
        self.reversible_keys()
            .iter()
            .rev()
            .enumerate()
            .skip(3)
            .step_by(2)
            .map(|(index, &key)| (index + 1, key))
    }

    // how often the current position has occurred in this game, the current one included
    pub fn position_repetitions(&self) -> usize {
        if self.halfmove_clock < 4 {
            return 1;
        }

        let mut repetitions = 1;
        for (_, key) in self.repetition_candidates() {
            if key == self.hash {
                repetitions += 1;
            }
        }

        repetitions
    }

    // the same position has been on the board three times - the drawing rule itself,
    // which is what the GUI reports a finished game with
    pub fn is_threefold_repetition(&self) -> bool {
        self.position_repetitions() >= 3
    }

    // a draw by repetition as the search should see it, `ply` plies below its root.
    //
    // one repetition inside the tree is already enough: the line got back to a position
    // it had already reached, so either side can simply do it again, and waiting for a
    // third costs four plies of depth on exactly the perpetuals that decide games.
    // before the root the real rule applies, because that is the game being played.
    //
    // note that this is path dependent - the same position is a draw or not depending
    // on how it was reached - so a score that came out of here must never be stored in
    // a transposition table under the position key alone
    pub fn is_repetition_draw(&self, ply: u32) -> bool {
        if self.halfmove_clock < 4 {
            return false;
        }

        let mut seen = 0;

        for (distance, key) in self.repetition_candidates() {
            if key != self.hash {
                continue;
            }

            // reached within the search tree rather than in the game before it
            if distance as u32 <= ply {
                return true;
            }

            // the current position is the first, so two more make three
            seen += 1;
            if seen >= 2 {
                return true;
            }
        }

        false
    }

    // fifty moves by each side with no capture and no pawn move
    pub fn is_fifty_move_draw(&self) -> bool {
        self.halfmove_clock >= 100
    }

    // neither side has enough material left to mate with: K vs K, KB vs K, KN vs K,
    // KB vs KB, KB vs KN, KN vs KN, or a bare king against king and two knights
    pub fn insufficient_material(&self) -> bool {
        // a single pawn, rook or queen is always enough material to mate with
        for color in Color::BOTH {
            for piece_type in [PieceType::Pawn, PieceType::Rook, PieceType::Queen] {
                if self.squares_with(piece_type, color).next().is_some() {
                    return false;
                }
            }
        }

        let (white_bishops, white_knights) = self.minor_piece_count(Color::White);
        let (black_bishops, black_knights) = self.minor_piece_count(Color::Black);

        insufficient_minors(
            [white_bishops, black_bishops],
            [white_knights, black_knights],
        )
    }

    // (bishops, knights) of one side
    fn minor_piece_count(&self, color: Color) -> (usize, usize) {
        (
            self.squares_with(PieceType::Bishop, color).count(),
            self.squares_with(PieceType::Knight, color).count(),
        )
    }
}

// with no pawns, rooks or queens left, whether anyone can still mate comes down to
// the minor pieces alone: one minor each at most cannot do it, and neither can two
// knights against a bare king
// both counts are [white, black]; kept out of `Board` so that a caller which has
// counted the pieces for its own reasons can ask without counting them again
pub fn insufficient_minors(bishops: [usize; 2], knights: [usize; 2]) -> bool {
    let white_minors = bishops[0] + knights[0];
    let black_minors = bishops[1] + knights[1];

    if white_minors <= 1 && black_minors <= 1 {
        return true;
    }

    (white_minors == 0 && bishops[1] == 0 && knights[1] == 2)
        || (black_minors == 0 && bishops[0] == 0 && knights[0] == 2)
}

// -------------------- keeping the hash in sync --------------------
impl Board {
    // every write to a square goes through here, so the hash, the king squares and the
    // phase stay in sync - xor is its own inverse, so taking off and putting on are one
    fn set_square(&mut self, square: u8, piece: Option<Piece>) {
        if let Some(previous) = self.squares[square as usize] {
            self.hash ^= ZOBRIST.piece(square, previous);
            // no guard needed here: this counts what is standing, not where it stands
            self.phase -= previous.piece_type().phase_weight();

            // only clear when this is still the recorded square: a move writes the king
            // onto `to` before clearing `from`, and that write already moved the record
            if previous.is(PieceType::King)
                && self.king_squares[previous.color().index()] == Some(square)
            {
                self.king_squares[previous.color().index()] = None;
            }
        }
        if let Some(piece) = piece {
            self.hash ^= ZOBRIST.piece(square, piece);
            self.phase += piece.piece_type().phase_weight();

            if piece.is(PieceType::King) {
                self.king_squares[piece.color().index()] = Some(square);
            }
        }

        self.squares[square as usize] = piece;
    }

    fn flip_turn(&mut self) {
        self.turn = self.turn.opponent();
        self.hash ^= ZOBRIST.side_to_move();
    }

    // swaps the castling keys of the old rights for those of the new ones
    fn set_castling_rights(&mut self, castling_rights: CastlingRights) {
        self.hash ^= ZOBRIST.castling(self.castling_rights) ^ ZOBRIST.castling(castling_rights);
        self.castling_rights = castling_rights;
    }

    // only the file of the target is hashed, and only while the side to move can really
    // capture onto it - two positions that differ in an unusable target are the same
    // position, so they have to get the same hash
    // call this only once the position and the side to move are final
    fn set_en_passant_target(&mut self, target: Option<u8>) {
        self.en_passant_target = target;

        let en_passant_hash = match self.capturable_en_passant_target() {
            Some(square) => ZOBRIST.en_passant_file(file_of(square)),
            None => 0,
        };

        self.hash ^= self.en_passant_hash ^ en_passant_hash;
        self.en_passant_hash = en_passant_hash;
    }

    // the en passant target, but only while the side to move can really take it
    fn capturable_en_passant_target(&self) -> Option<u8> {
        let target = self.en_passant_target?;
        let captured_square = en_passant_captured_square(target, self.turn);

        let can_capture = [-1, 1].into_iter().any(|file_step| {
            match offset(captured_square, (file_step, 0)).and_then(|square| self.piece_at(square)) {
                Some(piece) => piece.is(PieceType::Pawn) && piece.color() == self.turn,
                None => false,
            }
        });

        if can_capture { Some(target) } else { None }
    }

    // the king squares searched for from scratch, for the debug_asserts to check
    fn searched_king_squares(&self) -> [Option<u8>; 2] {
        Color::BOTH.map(|color| self.squares_with(PieceType::King, color).next())
    }

    // the halfmove clock counted from scratch, for the debug_asserts to check against
    fn counted_halfmove_clock(&self) -> u16 {
        self.history
            .iter()
            .rev()
            .take_while(|record| !record.chess_move.is_irreversible())
            .count() as u16
    }

    // the phase counted from scratch, for the debug_asserts to check against
    fn counted_phase(&self) -> i32 {
        self.squares
            .iter()
            .flatten()
            .map(|piece| piece.piece_type().phase_weight())
            .sum()
    }

    // the hash of the current position computed from scratch
    // the incrementally updated hash has to match this at all times, which is what the
    // debug_assert in make_move/undo_move checks
    fn full_hash(&self) -> u64 {
        let mut hash = ZOBRIST.castling(self.castling_rights);

        for (square, occupant) in self.squares.iter().enumerate() {
            if let Some(piece) = occupant {
                hash ^= ZOBRIST.piece(square as u8, *piece);
            }
        }

        if self.turn == Color::Black {
            hash ^= ZOBRIST.side_to_move();
        }

        if let Some(target) = self.capturable_en_passant_target() {
            hash ^= ZOBRIST.en_passant_file(file_of(target));
        }

        hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ng1-f3 Ng8-f6 Nf3-g1 Nf6-g8: four plies that put the position back exactly as it
    // was, which is the shortest repetition there is
    fn shuffle_knights(board: &mut Board) {
        board.make_move_from_squares(6, 21, None);
        board.make_move_from_squares(62, 45, None);
        board.make_move_from_squares(21, 6, None);
        board.make_move_from_squares(45, 62, None);
    }

    fn start_position() -> Board {
        let mut board = Board::new();
        board.set_start_position();
        board
    }

    #[test]
    fn the_start_position_has_twenty_legal_moves() {
        assert_eq!(start_position().legal_moves().len(), 20);
    }

    // playing a move and taking it back has to leave every square, the side to move
    // and the hash exactly as they were - the whole search depends on it
    #[test]
    fn undo_restores_the_position() {
        let mut board = start_position();
        let squares_before = board.squares;
        let hash_before = board.hash();

        for chess_move in board.legal_moves() {
            board.make_move(&chess_move);
            board.undo_move();

            assert_eq!(board.squares, squares_before, "after {chess_move:?}");
            assert_eq!(board.turn(), Color::White, "after {chess_move:?}");
            assert_eq!(board.hash(), hash_before, "after {chess_move:?}");
        }
    }

    // set_square sees the king twice per move, and only the first write counts
    #[test]
    fn the_king_square_follows_the_king() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        board.add_piece(Piece::new(PieceType::Rook, Color::White), 7); // h1
        board.add_piece(Piece::new(PieceType::King, Color::Black), 60); // e8

        assert_eq!(board.king_square(Color::White), Some(4));

        board.make_move_from_squares(4, 6, None); // e1g1, castling short
        assert_eq!(board.king_square(Color::White), Some(6));
        assert_eq!(board.king_square(Color::Black), Some(60));

        board.undo_move();
        assert_eq!(board.king_square(Color::White), Some(4));
    }

    // the capture writes the taker onto the very square the king stood on
    #[test]
    fn a_captured_king_leaves_the_king_square_empty() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        board.add_piece(Piece::new(PieceType::King, Color::Black), 12); // e2

        board.make_move_from_squares(4, 12, None);

        assert_eq!(board.king_square(Color::White), Some(12));
        assert_eq!(board.king_square(Color::Black), None);

        board.undo_move();
        assert_eq!(board.king_square(Color::Black), Some(12));
    }

    // bxa8=Q removes a rook and adds a queen in one move
    #[test]
    fn the_phase_follows_a_capture_and_a_promotion() {
        let mut board = Board::new();
        board.add_piece(Piece::new(PieceType::King, Color::White), 4); // e1
        board.add_piece(Piece::new(PieceType::King, Color::Black), 60); // e8
        board.add_piece(Piece::new(PieceType::Pawn, Color::White), 49); // b7
        board.add_piece(Piece::new(PieceType::Rook, Color::Black), 56); // a8

        // kings and pawns weigh nothing, so the rook is the whole of it
        assert_eq!(board.phase(), 2);

        board.make_move_from_squares(49, 56, Some(PieceType::Queen)); // bxa8=Q
        assert_eq!(board.phase(), 4);

        board.undo_move();
        assert_eq!(board.phase(), 2);
    }

    // the scan steps two plies at a time and starts four back, so the one thing it must
    // not do is step over the repetition it is looking for
    #[test]
    fn a_knight_shuffle_repeats_the_position() {
        let mut board = start_position();
        assert_eq!(board.position_repetitions(), 1);

        shuffle_knights(&mut board);
        assert_eq!(board.position_repetitions(), 2, "after one shuffle");

        shuffle_knights(&mut board);
        assert_eq!(board.position_repetitions(), 3, "after two shuffles");
        assert!(board.is_threefold_repetition());
    }

    // a pawn move shuts the window, so the shuffle after it repeats only the position
    // the pawns left behind - the two visits to the start position are out of reach and
    // the scan must not count them or run back that far looking
    #[test]
    fn a_pawn_move_closes_the_repetition_window() {
        let mut board = start_position();
        shuffle_knights(&mut board);
        assert_eq!(board.position_repetitions(), 2);

        board.make_move_from_squares(12, 28, None); // e2e4
        board.make_move_from_squares(52, 36, None); // e7e5, so white is to move again
        assert_eq!(board.halfmove_clock, 0);

        shuffle_knights(&mut board);
        assert_eq!(board.halfmove_clock, 4);
        assert_eq!(
            board.reversible_keys().len(),
            4,
            "the scan reached past the pawn moves"
        );
        assert_eq!(board.position_repetitions(), 2, "the pawns are still out");
    }

    // the clock counts reversible plies and a capture puts it back to nothing
    #[test]
    fn the_halfmove_clock_counts_and_resets() {
        let mut board = start_position();
        assert_eq!(board.halfmove_clock, 0);

        board.make_move_from_squares(6, 21, None); // Ng1f3
        assert_eq!(board.halfmove_clock, 1);
        board.make_move_from_squares(57, 42, None); // Nb8c6
        assert_eq!(board.halfmove_clock, 2);

        board.make_move_from_squares(12, 28, None); // e2e4, a pawn move
        assert_eq!(board.halfmove_clock, 0);
    }

    // undo has to put the clock and the key stack back, or every repetition the search
    // looks at afterwards is read off the wrong window
    #[test]
    fn undo_restores_the_repetition_state() {
        let mut board = start_position();
        shuffle_knights(&mut board);

        let clock_before = board.halfmove_clock;
        let keys_before = board.position_keys.clone();

        board.make_move_from_squares(6, 21, None); // Ng1f3
        board.undo_move();

        assert_eq!(board.halfmove_clock, clock_before);
        assert_eq!(board.position_keys, keys_before);
    }

    // the difference between the two rules: inside the search tree one repetition is a
    // draw, before the root it takes three
    #[test]
    fn the_search_draws_on_a_repetition_the_rules_would_not() {
        let mut board = start_position();
        shuffle_knights(&mut board);

        // the search played all four plies, so it can see it got back here itself
        assert!(board.is_repetition_draw(4));

        // only three of them are inside the tree: the repetition reaches past the root,
        // where twofold is just a position that has occurred before
        assert!(!board.is_repetition_draw(3));
        assert!(!board.is_threefold_repetition());

        // a third occurrence is a draw wherever the root is
        shuffle_knights(&mut board);
        assert!(board.is_repetition_draw(0));
        assert!(board.is_threefold_repetition());
    }

    // a hundred reversible plies with nothing captured and no pawn moved
    #[test]
    fn fifty_moves_by_each_side_is_a_draw() {
        let mut board = start_position();
        assert!(!board.is_fifty_move_draw());

        for _ in 0..25 {
            shuffle_knights(&mut board);
        }

        assert_eq!(board.halfmove_clock, 100);
        assert!(board.is_fifty_move_draw());
    }

    // the same position reached by two different move orders has to hash the same
    #[test]
    fn transpositions_hash_the_same() {
        let mut one = start_position();
        one.make_move_from_squares(12, 28, None); // e2e4
        one.make_move_from_squares(52, 36, None); // e7e5
        one.make_move_from_squares(6, 21, None); // g1f3
        one.make_move_from_squares(57, 42, None); // b8c6

        let mut other = start_position();
        other.make_move_from_squares(6, 21, None); // g1f3
        other.make_move_from_squares(57, 42, None); // b8c6
        other.make_move_from_squares(12, 28, None); // e2e4
        other.make_move_from_squares(52, 36, None); // e7e5

        assert_eq!(one.hash(), other.hash());
    }
}
