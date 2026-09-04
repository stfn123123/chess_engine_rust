# Goals / Todo

## Evaluation

## Search
- improve Zobrist (TESTING)
- transposition table
  - storing information:
  - Zobrist Key
  - best move done
  - depth
  - score
  - Type of Node (exact, upper bound, lower bound)

## Performance
- psqt inefficient? always have to find all pieces positions at every evaluation?
- Phase only changes on a capture or a promotion, so it could live on Board and be maintained in set_square, 
exactly like the zobrist hash and the king squares. That removes the 64-square walk from evaluate entirely.
Worth doing after the weights change, and it's the same pattern you've already got twice.

## Gameplay
- add a time
- add different time modes and increment
- add bot vs bot, bot vs player

## Search
- iterative deepening

## Connect to Lichess
- using the api


# Performance results
- PSQT, without saving the king for efficiency, no Quiescience: 2.5M to 2.9M
- PSQT, WITH saving the king for efficienxy, no Quiescience: up to 3M
- PSQT, WITH saving the king for efficienxy, WITH Quiescience: early up to 2m, endgame up to 3.5m