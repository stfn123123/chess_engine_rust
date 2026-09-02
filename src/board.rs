// Goal: create a valid and full representation of a chess board with the final goal of creating a chess engine

// missing feature:
// 50-move-rule


#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PieceType {
    None = 0,
    King = 1,
    Pawn = 2,
    Knight = 3,
    Bishop = 4,
    Rook = 5,
    Queen = 6,
}

// the discriminants are the color bits stored inside a Piece
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Color {
    White = 8,
    Black = 16,
}

impl Color {
    pub fn opponent(self) -> Color {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }
}

// king side = short castle (towards the h file), queen side = long castle (towards the a file)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CastleSide {
    King,
    Queen,
}

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

    fn clear(&mut self, color: Color, side: CastleSide) {
        let flag = match (color, side) {
            (Color::White, CastleSide::King) => &mut self.white_king_side,
            (Color::White, CastleSide::Queen) => &mut self.white_queen_side,
            (Color::Black, CastleSide::King) => &mut self.black_king_side,
            (Color::Black, CastleSide::Queen) => &mut self.black_queen_side,
        };
        *flag = false;
    }

    fn clear_color(&mut self, color: Color) {
        self.clear(color, CastleSide::King);
        self.clear(color, CastleSide::Queen);
    }
}

const TYPE_MASK: u8 = 0b00111;
const COLOR_MASK: u8 = 0b11000;


#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Piece(u8);

impl Piece {
    pub fn new(piece_type: PieceType, color: Color) -> Self {
        Piece(piece_type as u8 | color as u8)
    }

    pub fn piece_type(self) -> u8 {
        self.0 & TYPE_MASK
    }

    pub fn color(self) -> Color {
        if self.0 & COLOR_MASK == Color::White as u8 {
            Color::White
        } else {
            Color::Black
        }
    }

    pub fn symbol(self) -> char {
        let letter = match self.piece_type() {
            1 => 'k',
            2 => 'p',
            3 => 'n',
            4 => 'b',
            5 => 'r',
            6 => 'q',
            _ => '?',
        };
        match self.color() {
            Color::White => letter.to_ascii_uppercase(),
            Color::Black => letter,
        }
    }
}

pub struct Move {
    pub from: u8,
    pub to: u8,
    pub piece: Piece,
    pub captured: Option<Piece>,
    pub castle: Option<CastleSide>,
    // the piece type the pawn turned into, set only on promotion moves
    // `piece` stays the pawn, so undo_move can put the pawn back
    pub promotion: Option<PieceType>,
    // true when this is an en passant capture: then `captured` does not stand on `to`
    // but on the square next to `from`
    pub en_passant: bool,
    pub castling_rights_before: CastlingRights,
    // the en passant target that was in effect before the move, for undo_move
    pub en_passant_before: Option<u8>,
}

impl Move {
    // helper: an ordinary (non castling, non promoting) move
    fn normal(from: u8, to: u8, piece: Piece, captured: Option<Piece>) -> Move {
        Move {
            from,
            to,
            piece,
            captured,
            castle: None,
            promotion: None,
            en_passant: false,
            castling_rights_before: CastlingRights::NONE,
            en_passant_before: None,
        }
    }

    // helper: a pawn capturing a pawn that just passed by
    fn en_passant(from: u8, to: u8, piece: Piece, captured: Piece) -> Move {
        Move {
            en_passant: true,
            ..Move::normal(from, to, piece, Some(captured))
        }
    }

    // helper: a pawn move that ends on the last rank
    fn promoting(
        from: u8,
        to: u8,
        piece: Piece,
        captured: Option<Piece>,
        promote_to: PieceType,
    ) -> Move {
        Move {
            promotion: Some(promote_to),
            ..Move::normal(from, to, piece, captured)
        }
    }
}

pub struct Board {
    turn: Color,
    square: [Option<Piece>; 64],
    moves: Vec<Move>,
    // the zobrist hash of the position before each move on the move stack,
    // so index i belongs to moves[i]
    position_history: Vec<u64>,
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
}

// -------------------- public API --------------------
impl Board {
    pub fn new() -> Board {
        let castling_rights = CastlingRights::ALL;

        Board {
            turn: Color::White,
            square: [None; 64],
            moves: Vec::new(),
            position_history: Vec::new(),
            castling_rights,
            en_passant_target: None,
            // white to move and no en passant target contribute nothing
            hash: castling_hash(castling_rights),
            en_passant_hash: 0,
        }
    }

    pub fn turn(&self) -> Color {
        self.turn
    }

    // the zobrist hash of the current position
    pub fn hash(&self) -> u64 {
        self.hash
    }

    pub fn castling_rights(&self) -> CastlingRights {
        self.castling_rights
    }

    pub fn en_passant_target(&self) -> Option<u8> {
        self.en_passant_target
    }

    pub fn add_piece(&mut self, piece: Piece, position: u8) {
        self.set_square(position, Some(piece));
    }

    pub fn create_basic_layout(&mut self) {
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

    #[allow(dead_code)]
    pub fn display(&self) {
        for rank in (0..8).rev() {
            for file in 0..8 {
                let symbol = match self.square[rank * 8 + file] {
                    Some(piece) => piece.symbol(),
                    None => '.',
                };
                print!("{symbol} ");
            }
            println!();
        }
    }

    // returns the piece on a certain position
    pub fn get_piece(&self, position: u8) -> Option<Piece> {
        self.square[position as usize]
    }

    // adds a move to the stack (piece type or from required, rest optional?)
    // a king moving two files is taken as a castle: the rook is moved along with it
    // a pawn reaching the last rank is replaced by `promotion`, defaulting to a queen
    // a pawn moving diagonally onto an empty square is taken as an en passant capture
    pub fn create_move(&mut self, from: u8, to: u8, promotion: Option<PieceType>) {
        // has to happen before anything is changed: this records the position as it was
        self.position_history.push(self.hash);

        let piece = self.square[from as usize].expect("no piece on the from square");
        let is_king = piece.piece_type() == PieceType::King as u8;
        let is_pawn = piece.piece_type() == PieceType::Pawn as u8;

        let en_passant = is_pawn && from % 8 != to % 8 && self.square[to as usize].is_none();
        let captured = if en_passant {
            let captured_square = en_passant_captured_square(to, piece.color());
            let captured_pawn = self.square[captured_square as usize];
            self.set_square(captured_square, None);
            captured_pawn
        } else {
            self.square[to as usize]
        };

        let castle = if is_king && (to as i8 - from as i8).abs() == 2 {
            Some(if to > from {
                CastleSide::King
            } else {
                CastleSide::Queen
            })
        } else {
            None
        };

        let promotion = if is_pawn && to / 8 == last_rank(piece.color()) {
            Some(promotion.unwrap_or(PieceType::Queen))
        } else {
            None
        };

        // the pawn only reaches the board as its promoted piece, the Move keeps the pawn
        let arriving = match promotion {
            Some(piece_type) => Piece::new(piece_type, piece.color()),
            None => piece,
        };
        self.set_square(to, Some(arriving));
        self.set_square(from, None);

        if let Some(side) = castle {
            let rook_from = rook_start_square(piece.color(), side);
            let rook_to = rook_castle_square(piece.color(), side);
            let rook = self.square[rook_from as usize];
            self.set_square(rook_to, rook);
            self.set_square(rook_from, None);
        }

        let castling_rights_before = self.castling_rights;
        let mut castling_rights = self.castling_rights;
        if is_king {
            castling_rights.clear_color(piece.color());
        }
        // covers both a rook leaving its start square and a rook being captured on it
        clear_castling_rights_on(&mut castling_rights, from);
        clear_castling_rights_on(&mut castling_rights, to);
        self.set_castling_rights(castling_rights);

        let en_passant_before = self.en_passant_target;

        self.moves.push(Move {
            from,
            to,
            piece,
            captured,
            castle,
            promotion,
            en_passant,
            castling_rights_before,
            en_passant_before,
        });
        self.flip_turn();

        // last, because whether the target is capturable depends on the finished
        // position and on the side that is to move now
        // only a double push opens a target, every other move closes it
        let target = if is_pawn && (to as i8 - from as i8).abs() == 16 {
            Some((from + to) / 2)
        } else {
            None
        };
        self.set_en_passant_target(target);

        debug_assert_eq!(self.hash, self.full_hash(), "incremental hash drifted");
    }

    // undoes the last move (pop from the stack, restore the captured piece and the
    // castling rights, move a castled rook back, flip turn back)
    // a promotion needs no extra work: `piece` is still the pawn, so putting it back
    // on `from` removes the promoted piece from the board
    pub fn undo_move(&mut self) -> Option<Move> {
        let last_move = self.moves.pop()?;
        // moves and position_history are pushed together and have to stay the same length
        self.position_history.pop();

        self.set_square(last_move.from, Some(last_move.piece));
        if last_move.en_passant {
            // the captured pawn never stood on `to`
            let captured_square = en_passant_captured_square(last_move.to, last_move.piece.color());
            self.set_square(last_move.to, None);
            self.set_square(captured_square, last_move.captured);
        } else {
            self.set_square(last_move.to, last_move.captured);
        }

        if let Some(side) = last_move.castle {
            let rook_from = rook_start_square(last_move.piece.color(), side);
            let rook_to = rook_castle_square(last_move.piece.color(), side);
            let rook = self.square[rook_to as usize];
            self.set_square(rook_from, rook);
            self.set_square(rook_to, None);
        }

        self.set_castling_rights(last_move.castling_rights_before);
        self.flip_turn();
        // same as in create_move: the target is restored last, once the position and
        // the side to move are back to what they were
        self.set_en_passant_target(last_move.en_passant_before);

        debug_assert_eq!(self.hash, self.full_hash(), "incremental hash drifted");

        Some(last_move)
    }

    // filters the pseudo-legal moves of a side down to moves that don't leave/put that side's
    // own king in check (covers the king walking into check, pins, blocking checks, etc.)
    pub fn get_legal_moves(&mut self, color: Color) -> Vec<Move> {
        let candidate_moves = self.pseudo_moves_for_color(color);

        candidate_moves
            .into_iter()
            .filter(|candidate| {
                self.create_move(candidate.from, candidate.to, candidate.promotion);
                let leaves_king_in_check = self.is_check(color);
                self.undo_move();
                !leaves_king_in_check
            })
            .collect()
    }

    // checks if the king of the given side is in check
    pub fn is_check(&self, color: Color) -> bool {
        match self.positions_of(PieceType::King, color).first() {
            Some(&king_position) => self.is_attacked(king_position, color.opponent()),
            None => false,
        }
    }

    // check if either side has won (side to move is in check and has no legal moves left)
    pub fn is_checkmate(&mut self) -> bool {
        self.is_check(self.turn) && self.get_legal_moves(self.turn).is_empty()
    }

    // side to move is not in check but has no legal moves left
    pub fn is_stalemate(&mut self) -> bool {
        !self.is_check(self.turn) && self.get_legal_moves(self.turn).is_empty()
    }

    // returns the winning side if is_checkmate is true
    // method only makes sense to call if is_checkmate was called before
    pub fn get_winner(&self) -> Color {
        if self.is_check(Color::White) {
            Color::Black
        } else {
            Color::White
        }
    }

    // how often the current position has occurred in this game, the current one included
    pub fn position_repetitions(&self) -> usize {
        let current = self.hash;
        let mut repetitions = 1;

        // walking backwards only until the last capture or pawn move is enough:
        // no position from before such a move can ever come back
        for index in (0..self.position_history.len()).rev() {
            if self.position_history[index] == current {
                repetitions += 1;
            }
            if is_irreversible(&self.moves[index]) {
                break;
            }
        }

        repetitions
    }

    // the same position has been on the board three times
    pub fn is_threefold_repetition(&self) -> bool {
        self.position_repetitions() >= 3
    }

    // if both sides have: only king, only king and bishop, only king and knight, only king and knight and king and bishop or
    // if one side has only king and the other side has king and 2 knights
    pub fn insufficient_material(&self) -> bool {
        // a single pawn, rook or queen is always enough material to mate with
        for color in [Color::White, Color::Black] {
            for piece_type in [PieceType::Pawn, PieceType::Rook, PieceType::Queen] {
                if !self.positions_of(piece_type, color).is_empty() {
                    return false;
                }
            }
        }

        let (white_bishops, white_knights) = self.minor_piece_count(Color::White);
        let (black_bishops, black_knights) = self.minor_piece_count(Color::Black);
        let white_minors = white_bishops + white_knights;
        let black_minors = black_bishops + black_knights;

        // K vs K, KB vs K, KN vs K, KB vs KB, KB vs KN, KN vs KN
        if white_minors <= 1 && black_minors <= 1 {
            return true;
        }

        // bare king against king and two knights
        (white_minors == 0 && black_bishops == 0 && black_knights == 2)
            || (black_minors == 0 && white_bishops == 0 && white_knights == 2)
    }
}

// -------------------- private helpers --------------------
impl Board {
    // helper: every write to a square goes through here, so that the hash stays in sync
    // with the board - xor is its own inverse, so taking a piece off a square is the
    // same operation as putting it there
    fn set_square(&mut self, position: u8, piece: Option<Piece>) {
        if let Some(previous) = self.square[position as usize] {
            self.hash ^= ZOBRIST.pieces[position as usize][previous.0 as usize];
        }
        if let Some(piece) = piece {
            self.hash ^= ZOBRIST.pieces[position as usize][piece.0 as usize];
        }

        self.square[position as usize] = piece;
    }

    // helper: the side to move key is mixed in exactly while black is to move
    fn flip_turn(&mut self) {
        self.turn = self.turn.opponent();
        self.hash ^= ZOBRIST.side_to_move;
    }

    // helper: swaps the castling keys of the old rights for those of the new ones
    fn set_castling_rights(&mut self, castling_rights: CastlingRights) {
        self.hash ^= castling_hash(self.castling_rights) ^ castling_hash(castling_rights);
        self.castling_rights = castling_rights;
    }

    // helper: only the file of the target is hashed, and only while the side to move
    // can really capture onto it - two positions that differ in an unusable target are
    // the same position, so they have to get the same hash
    // call this only once the position and the side to move are final
    fn set_en_passant_target(&mut self, target: Option<u8>) {
        self.en_passant_target = target;

        let en_passant_hash = match self.capturable_en_passant_target() {
            Some(square) => ZOBRIST.en_passant_file[(square % 8) as usize],
            None => 0,
        };

        self.hash ^= self.en_passant_hash ^ en_passant_hash;
        self.en_passant_hash = en_passant_hash;
    }

    // helper: the hash of the current position computed from scratch
    // the incrementally updated hash has to match this at all times, which is what the
    // debug_assert in create_move/undo_move checks
    fn full_hash(&self) -> u64 {
        let mut hash = castling_hash(self.castling_rights);

        for (position, square) in self.square.iter().enumerate() {
            if let Some(piece) = square {
                hash ^= ZOBRIST.pieces[position][piece.0 as usize];
            }
        }

        if self.turn == Color::Black {
            hash ^= ZOBRIST.side_to_move;
        }

        if let Some(target) = self.capturable_en_passant_target() {
            hash ^= ZOBRIST.en_passant_file[(target % 8) as usize];
        }

        hash
    }

    // helper: the en passant target, but only while the side to move can really take it
    // two positions that differ only in an unusable target are the same position
    fn capturable_en_passant_target(&self) -> Option<u8> {
        let target = self.en_passant_target?;
        let captured_square = en_passant_captured_square(target, self.turn);
        let file = (captured_square % 8) as i8;

        let can_capture = [-1, 1].into_iter().any(|file_offset: i8| {
            if !(0..8).contains(&(file + file_offset)) {
                return false;
            }

            let neighbour = (captured_square as i8 + file_offset) as u8;
            match self.get_piece(neighbour) {
                Some(piece) => {
                    piece.piece_type() == PieceType::Pawn as u8 && piece.color() == self.turn
                }
                None => false,
            }
        });

        if can_capture { Some(target) } else { None }
    }

    // helper: (bishops, knights) of one side
    fn minor_piece_count(&self, color: Color) -> (usize, usize) {
        (
            self.positions_of(PieceType::Bishop, color).len(),
            self.positions_of(PieceType::Knight, color).len(),
        )
    }

    // helper: positions of every piece of a certain type and color
    fn positions_of(&self, piece_type: PieceType, color: Color) -> Vec<u8> {
        let mut positions = Vec::new();
        for (index, square) in self.square.iter().enumerate() {
            if let Some(piece) = square {
                if piece.piece_type() == piece_type as u8 && piece.color() == color {
                    positions.push(index as u8);
                }
            }
        }
        positions
    }

    // helper: is the given square attacked by any piece of the given color
    // this looks outward from the square ("what could reach me from here") instead of
    // generating every move of that side, so it allocates nothing and stops at the
    // first attacker it finds
    fn is_attacked(&self, position: u8, color: Color) -> bool {
        let file = (position % 8) as i8;
        let rank = (position / 8) as i8;

        // an attacking pawn stands one rank behind this square (seen from its own
        // direction of travel) on a neighbouring file - a pawn push is not an attack
        let pawn_rank = rank - pawn_direction(color);
        for file_offset in [-1, 1] {
            if self.has_piece_at(file + file_offset, pawn_rank, PieceType::Pawn as u8, color) {
                return true;
            }
        }

        for &(file_offset, rank_offset) in &KNIGHT_OFFSETS {
            if self.has_piece_at(
                file + file_offset,
                rank + rank_offset,
                PieceType::Knight as u8,
                color,
            ) {
                return true;
            }
        }

        for &(file_offset, rank_offset) in &KING_OFFSETS {
            if self.has_piece_at(
                file + file_offset,
                rank + rank_offset,
                PieceType::King as u8,
                color,
            ) {
                return true;
            }
        }

        // in each direction only the first piece can attack, everything behind it is blocked
        self.is_attacked_by_slider(file, rank, &DIAGONAL_DIRECTIONS, PieceType::Bishop, color)
            || self.is_attacked_by_slider(file, rank, &STRAIGHT_DIRECTIONS, PieceType::Rook, color)
    }

    // helper: is there a piece of that type and color on (file, rank)
    // off the board counts as no piece, so callers don't have to check the bounds
    fn has_piece_at(&self, file: i8, rank: i8, piece_type: u8, color: Color) -> bool {
        if !(0..8).contains(&file) || !(0..8).contains(&rank) {
            return false;
        }

        match self.square[(rank * 8 + file) as usize] {
            Some(piece) => piece.piece_type() == piece_type && piece.color() == color,
            None => false,
        }
    }

    // helper: walks each direction until it runs into a piece - true when that piece is
    // a queen or the given slider (bishop for the diagonals, rook for the straight lines)
    fn is_attacked_by_slider(
        &self,
        file: i8,
        rank: i8,
        directions: &[(i8, i8)],
        slider: PieceType,
        color: Color,
    ) -> bool {
        for &(file_step, rank_step) in directions {
            let mut current_file = file + file_step;
            let mut current_rank = rank + rank_step;

            while (0..8).contains(&current_file) && (0..8).contains(&current_rank) {
                if let Some(piece) = self.square[(current_rank * 8 + current_file) as usize] {
                    let attacks = piece.color() == color
                        && (piece.piece_type() == slider as u8
                            || piece.piece_type() == PieceType::Queen as u8);
                    if attacks {
                        return true;
                    }
                    // blocked, the rest of this direction cannot reach the square
                    break;
                }

                current_file += file_step;
                current_rank += rank_step;
            }
        }

        false
    }

    // helper: pseudo-legal moves of both sides (simply call pseudo_moves_for_color twice)
    #[allow(dead_code)]
    fn pseudo_moves_all(&self) -> Vec<Move> {
        let mut moves = self.pseudo_moves_for_color(Color::White);
        moves.extend(self.pseudo_moves_for_color(Color::Black));
        moves
    }

    // helper: pseudo-legal moves of one side (loops over all piece types)
    fn pseudo_moves_for_color(&self, color: Color) -> Vec<Move> {
        let piece_types = [
            PieceType::King,
            PieceType::Pawn,
            PieceType::Knight,
            PieceType::Bishop,
            PieceType::Rook,
            PieceType::Queen,
        ];

        let mut moves = Vec::new();
        for piece_type in piece_types {
            moves.extend(self.pseudo_moves_for_piece_type(piece_type, color));
        }
        moves.extend(self.castle_moves(color));
        moves
    }

    // helper: pseudo-legal moves of every piece of one type and color
    fn pseudo_moves_for_piece_type(&self, piece_type: PieceType, color: Color) -> Vec<Move> {
        let mut moves = Vec::new();
        for position in self.positions_of(piece_type, color) {
            moves.extend(self.pseudo_moves_from(position));
        }
        moves
    }

    // helper: pseudo-legal moves of the piece standing on a certain position
    // castling is not generated here but in castle_moves, so that the attack scan of
    // is_attacked can never recurse back into castle generation
    fn pseudo_moves_from(&self, position: u8) -> Vec<Move> {
        let piece = match self.get_piece(position) {
            Some(piece) => piece,
            None => return Vec::new(),
        };

        match piece.piece_type() {
            t if t == PieceType::Bishop as u8 => self.pseudo_moves_diagonal(piece, position),
            t if t == PieceType::Rook as u8 => self.pseudo_moves_straight(piece, position),
            t if t == PieceType::Queen as u8 => {
                let mut moves = self.pseudo_moves_diagonal(piece, position);
                moves.extend(self.pseudo_moves_straight(piece, position));
                moves
            }
            t if t == PieceType::Knight as u8 => self.pseudo_moves_knight(position),
            t if t == PieceType::King as u8 => self.pseudo_moves_king(position),
            t if t == PieceType::Pawn as u8 => self.pseudo_moves_pawn(position),
            _ => Vec::new(),
        }
    }

    // helper: the castling moves of one side
    // unlike the other generators this one already rules out castling out of, through or
    // into check, because get_legal_moves only ever looks at the final position
    fn castle_moves(&self, color: Color) -> Vec<Move> {
        let mut moves = Vec::new();
        let king_from = king_start_square(color);

        let king = match self.get_piece(king_from) {
            Some(piece)
                if piece.piece_type() == PieceType::King as u8 && piece.color() == color =>
            {
                piece
            }
            _ => return moves,
        };

        for side in [CastleSide::King, CastleSide::Queen] {
            if !self.castling_rights.get(color, side) {
                continue;
            }

            let rook_from = rook_start_square(color, side);
            let rook_stands_there = match self.get_piece(rook_from) {
                Some(piece) => {
                    piece.piece_type() == PieceType::Rook as u8 && piece.color() == color
                }
                None => false,
            };
            if !rook_stands_there {
                continue;
            }

            // every square between king and rook has to be empty
            if squares_between(king_from, rook_from).any(|square| self.get_piece(square).is_some())
            {
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

            moves.push(Move {
                from: king_from,
                to: king_to,
                piece: king,
                captured: None,
                castle: Some(side),
                promotion: None,
                en_passant: false,
                castling_rights_before: CastlingRights::NONE,
                en_passant_before: None,
            });
        }

        moves
    }

    // helper: diagonal sliding moves (bishop/queen)
    fn pseudo_moves_diagonal(&self, piece: Piece, position: u8) -> Vec<Move> {
        self.pseudo_moves_sliding(piece, position, &DIAGONAL_DIRECTIONS)
    }

    // helper: horizontal + vertical sliding moves (rook/queen)
    fn pseudo_moves_straight(&self, piece: Piece, position: u8) -> Vec<Move> {
        self.pseudo_moves_sliding(piece, position, &STRAIGHT_DIRECTIONS)
    }

    // helper: walks in each given (file_step, rank_step) direction until the edge of the board,
    // another piece, or a capture is hit - shared by the diagonal/straight helpers
    fn pseudo_moves_sliding(&self, piece: Piece, position: u8, directions: &[(i8, i8)]) -> Vec<Move> {
        let mut moves = Vec::new();
        let start_file = (position % 8) as i8;
        let start_rank = (position / 8) as i8;

        for &(file_step, rank_step) in directions {
            let mut file = start_file + file_step;
            let mut rank = start_rank + rank_step;

            while (0..8).contains(&file) && (0..8).contains(&rank) {
                let to = (rank * 8 + file) as u8;
                match self.square[to as usize] {
                    None => {
                        moves.push(Move::normal(position, to, piece, None));
                    }
                    Some(occupant) => {
                        if occupant.color() != piece.color() {
                            moves.push(Move::normal(position, to, piece, Some(occupant)));
                        }
                        break;
                    }
                }
                file += file_step;
                rank += rank_step;
            }
        }

        moves
    }

    // helper
    fn pseudo_moves_pawn(&self, position: u8) -> Vec<Move> {
        let piece = match self.get_piece(position) {
            Some(piece) => piece,
            None => return Vec::new(),
        };

        let file = (position % 8) as i8;
        let rank = (position / 8) as i8;
        let direction = pawn_direction(piece.color());
        let start_rank: i8 = match piece.color() {
            Color::White => 1,
            Color::Black => 6,
        };

        let mut moves = Vec::new();

        let one_forward_rank = rank + direction;
        if (0..8).contains(&one_forward_rank) {
            let one_forward = (one_forward_rank * 8 + file) as u8;
            if self.square[one_forward as usize].is_none() {
                push_pawn_move(&mut moves, position, one_forward, piece, None);

                if rank == start_rank {
                    let two_forward = ((rank + direction * 2) * 8 + file) as u8;
                    if self.square[two_forward as usize].is_none() {
                        moves.push(Move::normal(position, two_forward, piece, None));
                    }
                }
            }
        }

        for file_offset in [-1, 1] {
            let capture_file = file + file_offset;
            let capture_rank = rank + direction;
            if !(0..8).contains(&capture_file) || !(0..8).contains(&capture_rank) {
                continue;
            }

            let to = (capture_rank * 8 + capture_file) as u8;
            match self.square[to as usize] {
                Some(occupant) => {
                    if occupant.color() != piece.color() {
                        push_pawn_move(&mut moves, position, to, piece, Some(occupant));
                    }
                }
                // the rank check makes sure only the side to move can take en passant
                None if self.en_passant_target == Some(to)
                    && to / 8 == en_passant_rank(piece.color()) =>
                {
                    let captured_square = en_passant_captured_square(to, piece.color());
                    if let Some(captured_pawn) = self.square[captured_square as usize] {
                        moves.push(Move::en_passant(position, to, piece, captured_pawn));
                    }
                }
                None => {}
            }
        }

        moves
    }

    // helper
    fn pseudo_moves_king(&self, position: u8) -> Vec<Move> {
        let piece = match self.get_piece(position) {
            Some(piece) => piece,
            None => return Vec::new(),
        };

        self.pseudo_moves_stepping(piece, position, &KING_OFFSETS)
    }

    // helper
    fn pseudo_moves_knight(&self, position: u8) -> Vec<Move> {
        let piece = match self.get_piece(position) {
            Some(piece) => piece,
            None => return Vec::new(),
        };

        self.pseudo_moves_stepping(piece, position, &KNIGHT_OFFSETS)
    }

    // helper: applies each (file_offset, rank_offset) once (no sliding) - shared by knight/king
    fn pseudo_moves_stepping(&self, piece: Piece, position: u8, offsets: &[(i8, i8)]) -> Vec<Move> {
        let start_file = (position % 8) as i8;
        let start_rank = (position / 8) as i8;

        let mut moves = Vec::new();
        for &(file_offset, rank_offset) in offsets {
            let file = start_file + file_offset;
            let rank = start_rank + rank_offset;
            if !(0..8).contains(&file) || !(0..8).contains(&rank) {
                continue;
            }

            let to = (rank * 8 + file) as u8;
            match self.square[to as usize] {
                None => moves.push(Move::normal(position, to, piece, None)),
                Some(occupant) if occupant.color() != piece.color() => {
                    moves.push(Move::normal(position, to, piece, Some(occupant)));
                }
                _ => {}
            }
        }

        moves
    }
}

// (file_offset, rank_offset) tables, shared by the move generators and the attack scan
const KING_OFFSETS: [(i8, i8); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

const KNIGHT_OFFSETS: [(i8, i8); 8] = [
    (1, 2),
    (2, 1),
    (2, -1),
    (1, -2),
    (-1, -2),
    (-2, -1),
    (-2, 1),
    (-1, 2),
];

const DIAGONAL_DIRECTIONS: [(i8, i8); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];

const STRAIGHT_DIRECTIONS: [(i8, i8); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

// helper: the rank step a pawn of that color moves in
fn pawn_direction(color: Color) -> i8 {
    match color {
        Color::White => 1,
        Color::Black => -1,
    }
}

// a pawn may become any of these
const PROMOTION_CHOICES: [PieceType; 4] = [
    PieceType::Queen,
    PieceType::Rook,
    PieceType::Bishop,
    PieceType::Knight,
];

// -------------------- zobrist keys --------------------
// one random number per (square, piece), plus one for black to move, one per castling
// side and one per en passant file - the hash of a position is all of them xored
// together, which is why a move only has to xor the few keys that actually changed

// a Piece is `piece_type | color`, so the biggest value is Queen | Black = 6 | 16 = 22
// using Piece.0 directly as the index costs a few unused rows and saves a mapping
const PIECE_KEY_COUNT: usize = 23;

struct Zobrist {
    pieces: [[u64; PIECE_KEY_COUNT]; 64],
    side_to_move: u64,
    castling: [u64; 4],
    en_passant_file: [u64; 8],
}

// the keys have to be the same on every run, so they are built at compile time
static ZOBRIST: Zobrist = build_zobrist();

const ZOBRIST_SEED: u64 = 0x9E37_79B9_7F4A_7C15;

// xorshift64*, good enough for table keys and simple enough to run in a const fn
const fn next_state(state: u64) -> u64 {
    let mut state = state;
    state ^= state >> 12;
    state ^= state << 25;
    state ^= state >> 27;
    state
}

const fn random_from(state: u64) -> u64 {
    state.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

const fn build_zobrist() -> Zobrist {
    let mut pieces = [[0u64; PIECE_KEY_COUNT]; 64];
    let mut castling = [0u64; 4];
    let mut en_passant_file = [0u64; 8];
    let mut state = ZOBRIST_SEED;

    // const fn has no for loops, hence the while loops
    let mut square = 0;
    while square < 64 {
        let mut piece = 0;
        while piece < PIECE_KEY_COUNT {
            state = next_state(state);
            pieces[square][piece] = random_from(state);
            piece += 1;
        }
        square += 1;
    }

    let mut index = 0;
    while index < 4 {
        state = next_state(state);
        castling[index] = random_from(state);
        index += 1;
    }

    let mut file = 0;
    while file < 8 {
        state = next_state(state);
        en_passant_file[file] = random_from(state);
        file += 1;
    }

    state = next_state(state);
    let side_to_move = random_from(state);

    Zobrist {
        pieces,
        side_to_move,
        castling,
        en_passant_file,
    }
}

// helper: a rook that leaves its start square, or gets captured on it, ends that castling side
fn clear_castling_rights_on(castling_rights: &mut CastlingRights, position: u8) {
    for color in [Color::White, Color::Black] {
        for side in [CastleSide::King, CastleSide::Queen] {
            if rook_start_square(color, side) == position {
                castling_rights.clear(color, side);
            }
        }
    }
}

// helper: the keys of all castling sides that are still open
fn castling_hash(castling_rights: CastlingRights) -> u64 {
    let mut hash = 0;
    let mut index = 0;

    for color in [Color::White, Color::Black] {
        for side in [CastleSide::King, CastleSide::Queen] {
            if castling_rights.get(color, side) {
                hash ^= ZOBRIST.castling[index];
            }
            index += 1;
        }
    }

    hash
}

// helper: a move after which no earlier position can ever show up again
// (the same rule the 50-move counter resets on)
fn is_irreversible(chess_move: &Move) -> bool {
    chess_move.captured.is_some() || chess_move.piece.piece_type() == PieceType::Pawn as u8
}

// helper: the rank a pawn of that color promotes on
fn last_rank(color: Color) -> u8 {
    match color {
        Color::White => 7,
        Color::Black => 0,
    }
}

// helper: the rank a pawn of that color lands on when it captures en passant
fn en_passant_rank(color: Color) -> u8 {
    match color {
        Color::White => 5,
        Color::Black => 2,
    }
}

// helper: where the pawn being captured en passant stands - next to the capturing
// pawn, one rank behind the square it is captured on
fn en_passant_captured_square(to: u8, color: Color) -> u8 {
    match color {
        Color::White => to - 8,
        Color::Black => to + 8,
    }
}

// helper: adds a pawn move, split into one move per promotion choice when it ends
// on the last rank - so every promotion is its own move in the move list
fn push_pawn_move(moves: &mut Vec<Move>, from: u8, to: u8, piece: Piece, captured: Option<Piece>) {
    if to / 8 != last_rank(piece.color()) {
        moves.push(Move::normal(from, to, piece, captured));
        return;
    }

    for promote_to in PROMOTION_CHOICES {
        moves.push(Move::promoting(from, to, piece, captured, promote_to));
    }
}

// helper: e1 / e8
fn king_start_square(color: Color) -> u8 {
    match color {
        Color::White => 4,
        Color::Black => 60,
    }
}

// helper: a1 / h1 / a8 / h8
fn rook_start_square(color: Color, side: CastleSide) -> u8 {
    match (color, side) {
        (Color::White, CastleSide::King) => 7,
        (Color::White, CastleSide::Queen) => 0,
        (Color::Black, CastleSide::King) => 63,
        (Color::Black, CastleSide::Queen) => 56,
    }
}

// helper: where the king ends up after castling (g1 / c1 / g8 / c8)
fn king_castle_square(color: Color, side: CastleSide) -> u8 {
    match (color, side) {
        (Color::White, CastleSide::King) => 6,
        (Color::White, CastleSide::Queen) => 2,
        (Color::Black, CastleSide::King) => 62,
        (Color::Black, CastleSide::Queen) => 58,
    }
}

// helper: where the rook ends up after castling (f1 / d1 / f8 / d8)
// this is also the square the king crosses
fn rook_castle_square(color: Color, side: CastleSide) -> u8 {
    match (color, side) {
        (Color::White, CastleSide::King) => 5,
        (Color::White, CastleSide::Queen) => 3,
        (Color::Black, CastleSide::King) => 61,
        (Color::Black, CastleSide::Queen) => 59,
    }
}

// helper: the squares strictly between two positions on the same rank
fn squares_between(a: u8, b: u8) -> impl Iterator<Item = u8> {
    let (low, high) = if a < b { (a, b) } else { (b, a) };
    (low + 1)..high
}
