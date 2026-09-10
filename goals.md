# TODO Today:
- import Fen notation
- check stockfish for a position button
- improve GUI
- import many different positions, ranging from mid to late game
- save the stockfish move for each position
- be able to manually check each position with my current engine
- build an automatic script that evaluates many positions and finds the move, compare it to stockfish and print out the result in the terminal
  - print "accuracy", how often the correct move was found, and also how good the move was according to stockfish
  - print overall positions searched, nodes/sec and time taken for the entire test.
  - create a png showing the result, and listing all hyperparameters, when starting the script I add a name for the test/png.
- be able to adjust search depth in the GUI
- 


# Search & Evaluation
Search (biggest gaps)
- Principal Variation Search
- Zero/Null Window Search
- improving move ordering
- improving evaluation
    - Pawn structure
    - king safety
- improve LMR
- improve qsence

## Performance
- MoveOrder (search.rs, ~60 lines) could be one line instead: `moves.sort_by_cached_key(|m| -move_score(board, m))`.
  Both score every move once, which is where ~90% of the win over the old sort_unstable_by_key came from. The
  one-liner allocates a Vec per node and orders moves the search never reaches; MoveOrder uses a stack array and
  stops picking when the search stops asking, but is quadratic at nodes where every move gets searched.
  Expected to be roughly a wash - TEST IT, compare nodes/sec in the info panel.
  Keep MoveOrder only if the incremental interface is wanted for the TT move / killers / staged generation.


## Gameplay
- add different time modes and increment
- connect to lichess using the api









# Bitboards
generate_into and evaluate iterate pieces[color][type] now, and the sliders read the
attack tables in src/board/attacks.rs instead of walking rays square by square. Both
were measured and improved every number.

Still open: least_valuable_attacker re-walks rays on every SEE call (search.rs).

A bitboard version of it was written and reverted on 2026-09-09. With it in, perft lost
about 1M nodes/sec and the search 100-200k - but perft never calls SEE, so the SEE code
itself cannot be what cost that. The suspicion is inlining: attacks::bishop_attacks and
rook_attacks had one hot caller (sliding_moves) and were almost certainly inlined there
with the direction folded to a constant; least_valuable_attacker gave them a second hot
caller, which can flip that decision and slow the movegen path down instead.

attacks.rs has since been given #[inline] on every entry point, and the RUNS_UP lookup
split into ray_up/ray_down so the bitscan direction is constant where it is compiled.
That change is NOT measured yet.

Next time: re-measure perft with attacks.rs as it stands now, on its own. If it is at or
above the post-movegen number, redo the SEE rewrite (the reverted version read the tables
from the target square and intersected with the piece boards, masking `gone` out of both
the occupancy and each piece board) and measure again. If perft still sags, the cause is
binary layout rather than anything in the code, and the SEE rewrite is worth taking for
its own sake. One more thing left on the table either way: the RAYS index still carries a
bounds check, because the compiler cannot prove a bitscan result is under 64 - writing it
as (blockers.trailing_zeros() & 63) makes it provable without unsafe.
