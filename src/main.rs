mod board;

use board::Board;

fn main() {
    let mut board = Board::new();
    board.create_basic_layout();
    board.display();
}
