## Search
- qscience takes up a HUGE portion of nodes, has to be improved drastically.
  (1,393,130 / 1,655,546 and 4,212,964 / 5,396,998 - ~78-84% of all nodes)

  - check evasions inside quiescence are completely unpruned and unordered: generates
    the FULL legal move list (not just captures). perhaps create a seperate "capture only" method, to save time.
  - loops in raw generation order (no
    MoveOrder/MVV-LVA), no delta pruning, no depth cap - likely the single biggest cost,
    since any capture sequence that passes through one check brute-forces its subtree

  - no quiescence depth/ply cap at all - only stops when captures/checks run out

  - possible fixes, roughly in order of impact-to-effort:
    1. order and prune check evasions the same way normal captures are (MoveOrder at least)
    2. probe/store the transposition table inside quiescence
    3. SEE-based pruning instead of MVV-LVA-only ordering

- iterative deepening + adding time needed for it to work
- move ordering
  - idea: look at pieces positions and their psqt tables. prioritize pieces, that are on a negative psqt table entry.
  - goal: move pieces from bad spots to good spots.



## Performance
- save all pieces positions?
- piece centric board, no downsides in MY current implementation?

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
- add different time modes and increment
- add bot vs bot, bot vs player

## Connect to Lichess
- using the api

# Performance results
- PSQT, without saving the king for efficiency, no Quiescience: 2.5M to 2.9M
- PSQT, WITH saving the king for efficienxy, no Quiescience: up to 3M
- PSQT, WITH saving the king for efficienxy, WITH Quiescience: early up to 2m, endgame up to 3.5m