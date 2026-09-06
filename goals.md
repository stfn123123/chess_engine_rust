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
- MoveOrder (search.rs, ~60 lines) could be one line instead: `moves.sort_by_cached_key(|m| -move_score(board, m))`.
Both score every move once, which is where ~90% of the win over the old sort_unstable_by_key came from. The
one-liner allocates a Vec per node and orders moves the search never reaches; MoveOrder uses a stack array and
stops picking when the search stops asking, but is quadratic at nodes where every move gets searched.
Expected to be roughly a wash - TEST IT, compare nodes/sec in the info panel.
Keep MoveOrder only if the incremental interface is wanted for the TT move / killers / staged generation.
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