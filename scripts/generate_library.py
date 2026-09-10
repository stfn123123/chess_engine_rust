#!/usr/bin/env python3
"""Pre-computes Stockfish's top 5 moves and eval for a batch of positions.

Reads FENs (one per line, blank lines and "#" comments skipped) from an
input file - assets/positions.txt by default - and for every one not
already in assets/stockfish_library.txt, asks the bundled Stockfish binary
for its top 5 moves and records them, together with the position's game
phase (0.0 bare kings, 1.0 a full board). Talks to Stockfish directly over
UCI, the same way src/stockfish.rs does for the GUI's "Ask Stockfish"
button - this script replaces the old `generate-library` Rust command.

The library file itself is read by scripts/benchmark.py and by the GUI's
"Stored" window (src/stockfish_library.rs).

Usage:
    scripts/.venv/bin/python3 scripts/generate_library.py [positions.txt]
"""

import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_POSITIONS = REPO_ROOT / "assets" / "positions.txt"
LIBRARY_PATH = REPO_ROOT / "assets" / "stockfish_library.txt"
STOCKFISH_BINARY = REPO_ROOT / "assets" / "stockfish-linux"

# how many of Stockfish's top lines to ask for and record
MULTIPV = 5
# bounds how long each position takes - Stockfish always answers within this,
# whatever depth it reaches
SEARCH_MOVETIME_MS = 10_000

# PieceType::phase_weight() in src/board/piece.rs - kings and pawns count
# nothing towards the phase, a board of nothing but pawns is an endgame
PHASE_WEIGHTS = {"n": 1, "b": 1, "r": 2, "q": 4}
# 2*(2*knight + 2*bishop + 2*rook + queen), the same derivation evaluate.rs's
# TOTAL_PHASE uses - a full board's worth of minor/major pieces
TOTAL_PHASE = 2 * (2 * 1 + 2 * 1 + 2 * 2 + 4)


def phase_of_fen(fen):
    placement = fen.split()[0]
    weight = sum(PHASE_WEIGHTS.get(char.lower(), 0) for char in placement)
    return min(weight, TOTAL_PHASE) / TOTAL_PHASE


def read_positions(path):
    positions = []
    for line in path.read_text().splitlines():
        fen = line.strip()
        if fen and not fen.startswith("#"):
            positions.append(fen)
    return positions


def format_eval(eval_tuple):
    kind, value = eval_tuple
    return f"{kind}{value}"


def parse_eval(text):
    if text.startswith("cp"):
        return ("cp", int(text[2:]))
    if text.startswith("mate"):
        return ("mate", int(text[4:]))
    raise ValueError(f"unrecognised eval {text!r}")


def load_existing(path):
    """Parses the library file, migrating the old (pre-id/phase) format in
    place: a legacy line is `fen, move1..5, eval` (7 fields); the current
    one is `id, fen, phase, move1..5, eval` (9 fields)."""
    positions = []
    if not path.exists():
        return positions

    for line in path.read_text().splitlines():
        line = line.strip()
        if not line:
            continue
        fields = line.split("\t")

        if len(fields) >= 9:
            id_, fen, phase, *moves, eval_text = fields
            positions.append({
                "id": int(id_),
                "fen": fen,
                "phase": float(phase),
                "moves": moves,
                "eval": parse_eval(eval_text),
            })
        elif len(fields) >= 3:
            fen, *moves, eval_text = fields
            positions.append({
                "id": len(positions) + 1,
                "fen": fen,
                "phase": phase_of_fen(fen),
                "moves": moves,
                "eval": parse_eval(eval_text),
            })

    return positions


def save(positions, path):
    lines = []
    for position in positions:
        fields = [
            str(position["id"]),
            position["fen"],
            f"{position['phase']:.4f}",
            *position["moves"],
            format_eval(position["eval"]),
        ]
        lines.append("\t".join(fields))

    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n" if lines else "")


def send(process, command):
    process.stdin.write(command + "\n")
    process.stdin.flush()


def wait_for(process, marker):
    for line in process.stdout:
        if line.strip() == marker:
            return
    raise RuntimeError(f"stockfish closed before sending {marker!r}")


def read_top_lines(process):
    # keeps only the latest line per rank - stockfish repeats and refines
    # them as it searches deeper
    slots = [None] * MULTIPV

    for line in process.stdout:
        line = line.strip()
        if line.startswith("bestmove"):
            break

        parsed = parse_info_line(line)
        if parsed is not None:
            rank, move, eval_tuple = parsed
            if 1 <= rank <= MULTIPV:
                slots[rank - 1] = (move, eval_tuple)

    found = [slot for slot in slots if slot is not None]
    if not found:
        raise RuntimeError("stockfish gave no usable lines")
    return found


def parse_info_line(line):
    tokens = line.split()
    if not tokens or tokens[0] != "info":
        return None

    rank = token_after(tokens, "multipv")
    if rank is None:
        return None

    if (cp := token_after(tokens, "cp")) is not None:
        eval_tuple = ("cp", int(cp))
    elif (mate := token_after(tokens, "mate")) is not None:
        eval_tuple = ("mate", int(mate))
    else:
        return None

    move = token_after(tokens, "pv")
    if move is None:
        return None

    return int(rank), move, eval_tuple


def token_after(tokens, key):
    try:
        return tokens[tokens.index(key) + 1]
    except (ValueError, IndexError):
        return None


def best_moves(fen):
    process = subprocess.Popen(
        [str(STOCKFISH_BINARY)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
        bufsize=1,
    )
    try:
        send(process, "uci")
        wait_for(process, "uciok")

        send(process, f"setoption name MultiPV value {MULTIPV}")
        send(process, "isready")
        wait_for(process, "readyok")

        send(process, f"position fen {fen}")
        send(process, f"go movetime {SEARCH_MOVETIME_MS}")

        lines = read_top_lines(process)
    finally:
        try:
            send(process, "quit")
        except Exception:
            pass
        process.wait(timeout=5)

    moves = [move for move, _ in lines]
    # the position's evaluation is what its best move scored - there is only
    # one of these kept, not one per move
    eval_tuple = lines[0][1]
    return moves, eval_tuple


def main():
    if not STOCKFISH_BINARY.exists():
        print(f"{STOCKFISH_BINARY} not found")
        sys.exit(1)

    positions_path = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_POSITIONS
    if not positions_path.exists():
        print(f"{positions_path} not found")
        sys.exit(1)

    fens = read_positions(positions_path)
    library = load_existing(LIBRARY_PATH)
    known_fens = {position["fen"] for position in library}
    next_id = max((position["id"] for position in library), default=0) + 1

    total = len(fens)
    added = 0

    for index, fen in enumerate(fens, start=1):
        if fen in known_fens:
            print(f"[{index}/{total}] already in library, skipping")
            continue

        try:
            moves, eval_tuple = best_moves(fen)
        except Exception as error:
            print(f"[{index}/{total}] {fen}: {error}")
            continue

        position = {
            "id": next_id,
            "fen": fen,
            "phase": phase_of_fen(fen),
            "moves": moves,
            "eval": eval_tuple,
        }
        library.append(position)
        known_fens.add(fen)
        next_id += 1
        added += 1

        save(library, LIBRARY_PATH)
        print(f"[{index}/{total}] {fen} -> {' '.join(moves)}")

    print(f"\nadded {added} of {total} positions, {len(library)} total in {LIBRARY_PATH}")


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("\ninterrupted")
        sys.exit(1)
