// What the engine is allowed to spend on a move, and the two clocks of a real game.
//
// Three ways to bound a search, from the panel: a fixed depth (what the engine always
// did, and no clock at all), a fixed time per move, or a game clock per side that has to
// last the whole game. Only the last one needs managing - budget_for is where a remaining
// clock is turned into a budget for one move.

use std::time::{Duration, Instant};

use crate::board::piece::Color;

// what bounds a search
#[derive(Clone, Copy, PartialEq)]
pub enum TimeControl {
    // deepen to the depth set in the panel, however long that takes - no clock runs
    Depth,
    // the same budget for every move, whatever the game has cost so far - no clock runs
    MoveTime(Duration),
    // one clock per side for the whole game, `increment` added back after each move
    Clock { base: Duration, increment: Duration },
}

impl TimeControl {
    // whether a game played under this has clocks that run down
    pub fn is_clocked(self) -> bool {
        matches!(self, TimeControl::Clock { .. })
    }

    // the presets the panel offers, as (label, base minutes, increment seconds)
    pub const PRESETS: [(&'static str, u64, u64); 5] = [
        ("1+0", 1, 0),
        ("3+2", 3, 2),
        ("5+3", 5, 3),
        ("10+0", 10, 0),
        ("15+10", 15, 10),
    ];

    pub fn clock(base_minutes: u64, increment_seconds: u64) -> TimeControl {
        TimeControl::Clock {
            base: Duration::from_secs(base_minutes * 60),
            increment: Duration::from_secs(increment_seconds),
        }
    }
}

// one move gets a share of what is left on the clock, spread over this many moves. The
// number falls as the clock drains, so a full clock holds a little back and the endgame
// gets a little more than a flat share would hand it
const MOVES_LEFT_MIN: u32 = 15;
const MOVES_LEFT_MAX: u32 = 25;
// one more move's worth of spreading for every minute still on the clock
const MOVES_LEFT_PER_SECS: u64 = 60;

// the share of the increment spent as well as the budget - not all of it, so the clock
// creeps up rather than down when the position is quiet
const INCREMENT_SHARE: u32 = 4;
const INCREMENT_PARTS: u32 = 5;

// never spend more than this share of what is left on one move - the increment alone can
// be worth more than the clock it is being added to
const MAX_SHARE_OF_CLOCK: u32 = 3;

// held back from every budget, to cover the move being played and the panel redrawing
const RESERVE: Duration = Duration::from_millis(150);

// a search still has to come back with something, even on a nearly dead clock
const MIN_THINK: Duration = Duration::from_millis(50);

// the two clocks of a game, and which side is currently burning time
pub struct GameClock {
    // indexed with Color::index()
    remaining: [Duration; 2],
    // the side on the clock and when its turn started; None while paused or unclocked
    running: Option<(Color, Instant)>,
    // who ran out, once someone has - the game is over at that point
    flagged: Option<Color>,
}

impl GameClock {
    // both sides on the full base time, not yet running
    pub fn new(control: TimeControl) -> GameClock {
        let base = match control {
            TimeControl::Clock { base, .. } => base,
            // nothing to count down, but the struct still exists so the panel has
            // something to read
            _ => Duration::ZERO,
        };

        GameClock {
            remaining: [base; 2],
            running: None,
            flagged: None,
        }
    }

    // both sides on the clocks a caller already has, not running - UCI reports what is
    // left rather than a base, so there is nothing to count down from
    pub fn with_remaining(white: Duration, black: Duration) -> GameClock {
        GameClock {
            remaining: [white, black],
            running: None,
            flagged: None,
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    pub fn flagged(&self) -> Option<Color> {
        self.flagged
    }

    // puts `side` on the clock from now
    pub fn start(&mut self, side: Color) {
        if self.flagged.is_none() {
            self.running = Some((side, Instant::now()));
        }
    }

    // stops the clock, keeping what the side to move has spent so far
    pub fn pause(&mut self) {
        let Some((side, since)) = self.running.take() else {
            return;
        };
        self.spend(side, since.elapsed());
    }

    // what `side` has left right now: the side on the clock is counted down live, so the
    // panel can read this every frame without anything having to tick
    pub fn remaining_now(&self, side: Color) -> Duration {
        let stored = self.remaining[side.index()];
        match self.running {
            Some((running, since)) if running == side => stored.saturating_sub(since.elapsed()),
            _ => stored,
        }
    }

    // `side` has moved: what the move cost comes off, the increment goes back on, and the
    // other side goes on the clock
    pub fn move_played(&mut self, side: Color, increment: Duration) {
        let Some((running, since)) = self.running else {
            return;
        };
        if running != side {
            return;
        }

        self.spend(side, since.elapsed());

        // a move made on a dead clock does not earn its increment back
        if self.flagged.is_some() {
            self.running = None;
            return;
        }

        self.remaining[side.index()] += increment;
        self.running = Some((side.opponent(), Instant::now()));
    }

    // whether the side on the clock has run out - asked once a frame, since a human's
    // clock can drain with nothing else happening at all
    pub fn check_flag(&mut self) -> Option<Color> {
        if let Some((side, _)) = self.running {
            if self.remaining_now(side).is_zero() {
                self.pause();
            }
        }
        self.flagged
    }

    // how long `side` may think about the move it is on, out of what it has left
    pub fn budget_for(&self, side: Color, control: TimeControl) -> Duration {
        let increment = match control {
            TimeControl::Depth => return Duration::ZERO,
            TimeControl::MoveTime(budget) => return budget,
            TimeControl::Clock { increment, .. } => increment,
        };

        let remaining = self.remaining_now(side);
        if remaining <= RESERVE {
            return MIN_THINK;
        }

        // a share of what is left, so the clock decays rather than draining - spread over
        // fewer moves as it empties, which flattens what a game actually spends per move
        let moves_left = (MOVES_LEFT_MIN as u64 + remaining.as_secs() / MOVES_LEFT_PER_SECS)
            .min(MOVES_LEFT_MAX as u64) as u32;
        let share = remaining / moves_left;
        // not the whole increment: spending under what comes back keeps the clock rising
        // on a quiet position instead of grinding down
        let from_increment = increment * INCREMENT_SHARE / INCREMENT_PARTS;

        (share + from_increment)
            // the increment can outrun the clock itself, and no one move gets a third of it
            .min(remaining / MAX_SHARE_OF_CLOCK)
            // the search must be back before the flag falls, not exactly as it does
            .min(remaining - RESERVE)
            .max(MIN_THINK)
    }

    // takes `spent` off a side's clock, flagging it if that empties it
    fn spend(&mut self, side: Color, spent: Duration) {
        let left = &mut self.remaining[side.index()];
        *left = left.saturating_sub(spent);

        if left.is_zero() && self.flagged.is_none() {
            self.flagged = Some(side);
        }
    }
}

// a clock as the panel shows it: "9:41", or "8.4" once it is down to seconds
pub fn format_remaining(remaining: Duration) -> String {
    let seconds = remaining.as_secs();
    if seconds >= 10 {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    } else {
        format!("{:.1}", remaining.as_secs_f32())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ten_minutes() -> TimeControl {
        TimeControl::clock(10, 0)
    }

    #[test]
    fn a_fresh_clock_gives_both_sides_the_base_time() {
        let clock = GameClock::new(ten_minutes());

        assert_eq!(clock.remaining_now(Color::White), Duration::from_secs(600));
        assert_eq!(clock.remaining_now(Color::Black), Duration::from_secs(600));
        assert!(!clock.is_running());
    }

    #[test]
    fn only_the_side_to_move_burns_time() {
        let mut clock = GameClock::new(ten_minutes());
        clock.start(Color::White);
        clock.remaining[Color::White.index()] = Duration::from_secs(100);

        std::thread::sleep(Duration::from_millis(30));

        assert!(clock.remaining_now(Color::White) < Duration::from_secs(100));
        assert_eq!(clock.remaining_now(Color::Black), Duration::from_secs(600));
    }

    #[test]
    fn the_increment_goes_back_on_after_a_move() {
        let mut clock = GameClock::new(TimeControl::clock(10, 5));
        clock.start(Color::White);
        clock.move_played(Color::White, Duration::from_secs(5));

        // the move itself cost next to nothing, so the five seconds are nearly all profit
        assert!(clock.remaining_now(Color::White) > Duration::from_secs(604));

        // and black is the one on the clock now, so it is black's time running down
        // while white's stands still
        std::thread::sleep(Duration::from_millis(30));

        assert!(clock.remaining_now(Color::Black) < Duration::from_secs(600));
        assert!(clock.remaining_now(Color::White) > Duration::from_secs(604));
    }

    #[test]
    fn running_out_flags_that_side() {
        let mut clock = GameClock::new(ten_minutes());
        clock.remaining[Color::White.index()] = Duration::from_millis(10);
        clock.start(Color::White);

        std::thread::sleep(Duration::from_millis(30));

        assert_eq!(clock.check_flag(), Some(Color::White));
        assert!(!clock.is_running(), "the clock kept running past a flag");
    }

    // a full ten minutes is spread over the most moves there are: 24 seconds, not the 30
    // a flat twentieth would give
    #[test]
    fn a_budget_is_a_share_of_what_is_left() {
        let clock = GameClock::new(ten_minutes());

        assert_eq!(
            clock.budget_for(Color::White, ten_minutes()),
            Duration::from_secs(24)
        );
    }

    // and the same clock down to its last minute is spread over the fewest, so a move
    // late on gets a larger slice of a smaller clock than it would early
    #[test]
    fn a_draining_clock_spends_a_larger_share_per_move() {
        let mut clock = GameClock::new(ten_minutes());
        let full = clock.budget_for(Color::White, ten_minutes());

        clock.remaining[Color::White.index()] = Duration::from_secs(60);
        let low = clock.budget_for(Color::White, ten_minutes());

        // a tenth of the clock is left, but far more than a tenth of the budget
        assert_eq!(low, Duration::from_secs(4));
        assert!(low * 10 > full, "{low:?} against {full:?}");
    }

    // a share of what is left can only ever shrink what is left, never empty it - which
    // is the whole point of budgeting this way rather than counting moves out
    #[test]
    fn spending_the_budget_every_move_never_runs_the_clock_out() {
        let mut clock = GameClock::new(ten_minutes());

        // a hundred moves is a long game, and a twentieth at a time never gets there
        for _ in 0..100 {
            let left = clock.remaining_now(Color::White);
            let budget = clock.budget_for(Color::White, ten_minutes());
            assert!(budget < left, "{budget:?} of a {left:?} clock");

            // spend exactly the budget, as a search using all of it would
            clock.remaining[Color::White.index()] = left - budget;
        }

        assert!(clock.remaining_now(Color::White) > Duration::ZERO);
    }

    // the increment is worth more than a twentieth of a clock this short, and the caps
    // are what stop it being handed over whole
    #[test]
    fn a_short_clock_does_not_spend_its_whole_increment() {
        let control = TimeControl::clock(1, 30);
        let mut clock = GameClock::new(control);
        clock.remaining[Color::White.index()] = Duration::from_secs(6);

        let budget = clock.budget_for(Color::White, control);

        assert!(budget <= Duration::from_secs(2), "{budget:?} of a 6s clock");
    }

    #[test]
    fn a_nearly_dead_clock_still_allows_a_search() {
        let mut clock = GameClock::new(ten_minutes());
        clock.remaining[Color::White.index()] = Duration::from_millis(20);

        let budget = clock.budget_for(Color::White, ten_minutes());

        assert_eq!(budget, MIN_THINK);
    }

    #[test]
    fn a_fixed_move_time_ignores_the_clock() {
        let clock = GameClock::new(ten_minutes());
        let control = TimeControl::MoveTime(Duration::from_secs(15));

        assert_eq!(
            clock.budget_for(Color::White, control),
            Duration::from_secs(15)
        );
    }

    #[test]
    fn a_clock_reads_as_minutes_until_the_last_ten_seconds() {
        assert_eq!(format_remaining(Duration::from_secs(601)), "10:01");
        assert_eq!(format_remaining(Duration::from_secs(61)), "1:01");
        assert_eq!(format_remaining(Duration::from_millis(8_400)), "8.4");
    }
}
