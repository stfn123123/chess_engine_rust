// A move, and what the board has to remember to be able to take it back.
//
// A Move describes the move itself and nothing about the position it came from,
// so a generated move can be handed straight to Board::make_move. The state that
// only make_move knows about lives in MoveRecord, on the board's history stack.

use crate::board::castling::{CastleSide, CastlingRights};
use crate::board::piece::{Piece, PieceType};
use crate::board::square::{square_from_name, square_name};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
}

impl Move {
    // an ordinary (non castling, non promoting) move
    pub fn normal(from: u8, to: u8, piece: Piece, captured: Option<Piece>) -> Move {
        Move {
            from,
            to,
            piece,
            captured,
            castle: None,
            promotion: None,
            en_passant: false,
        }
    }

    // a pawn capturing a pawn that just passed by
    pub fn en_passant_capture(from: u8, to: u8, piece: Piece, captured: Piece) -> Move {
        Move {
            en_passant: true,
            ..Move::normal(from, to, piece, Some(captured))
        }
    }

    // a pawn move that ends on the last rank
    pub fn promoting(
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

    // the king's part of a castle; make_move moves the rook along with it
    pub fn castling(king: Piece, from: u8, to: u8, side: CastleSide) -> Move {
        Move {
            castle: Some(side),
            ..Move::normal(from, to, king, None)
        }
    }

    // the move as the two squares it goes between, e.g. "e2e4", with the promoted
    // piece after them when there is one - how a move is shown and written down
    pub fn coordinates(&self) -> String {
        let mut text = format!("{}{}", square_name(self.from), square_name(self.to));

        if let Some(promotion) = self.promotion {
            text.push(promotion.letter());
        }

        text
    }

    // a move after which no earlier position can ever show up again
    // (the same rule the 50-move counter resets on)
    pub fn is_irreversible(&self) -> bool {
        self.captured.is_some() || self.piece.is(PieceType::Pawn)
    }
}

// the two squares and the promotion in a move written the way `coordinates` writes
// it - the piece it moves is not in there, so only a board can turn this into a Move
pub fn parse_coordinates(text: &str) -> Option<(u8, u8, Option<PieceType>)> {
    // the byte slices below only line up with characters while this holds
    if !text.is_ascii() || (text.len() != 4 && text.len() != 5) {
        return None;
    }

    let from = square_from_name(&text[0..2])?;
    let to = square_from_name(&text[2..4])?;
    let promotion = match text.chars().nth(4) {
        Some(letter) => Some(PieceType::from_letter(letter)?),
        None => None,
    };

    Some((from, to, promotion))
}

// one entry of the board's history: the move plus the bits of state that cannot be
// worked out from the position afterwards
// the position key lives on its own stack instead of in here - the repetition scan
// reads nothing but keys, and packed they fit three times as many to a cache line
#[derive(Clone)]
pub struct MoveRecord {
    pub chess_move: Move,
    pub castling_rights_before: CastlingRights,
    // the en passant target that was in effect before the move
    pub en_passant_before: Option<u8>,
    // the halfmove clock as it stood before the move, which a reset cannot recover
    pub halfmove_clock_before: u16,
}
