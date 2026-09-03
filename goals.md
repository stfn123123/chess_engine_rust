# Goals / Todo

## Evaluation

## Search
- quiescence search
- improve Zobrist (TESTING)
- transposition table
  - storing information:
  - Zobrist Key
  - best move done
  - depth
  - score
  - Type of Node (exact, upper bound, lower bound)

## Performance
- Board::king_square searches the board for the king every time it is asked
  (squares_with(King, color).next()), and is asked several times per node -
  by is_check, legal_moves_for, evaluate_pins, possible_king_attackers.
  Worth a look: keep the two king squares on the board and update them in
  set_square.
- psqt inefficient? always have to find all pieces positions at every evaluation?

## Visuals
- add interactable and visual chess board
- show the current evaluation as a sidebar

## Gameplay
- add a time
- add different time modes and increment
- add bot vs bot, bot vs player

## Search
- iterative deepening

## Connect to Lichess
- using the api