//! The flash guard: what drives the pictures' brightness (the bars, the
//! beat's pulse, the bass, the energy) may not swing back and forth faster
//! than a photosensitive viewer can bear. WCAG 2.3.1 allows at most three
//! flashes - pairs of opposing changes - in any second; here a value may
//! turn (from rising to falling or back) only once in TURN_CALM (0.25 s:
//! at most two flashes a second) or TURN_INTENSE (0.17 s: under three),
//! and it moves smoothly (no jump within a frame). A face's brightness
//! that follows these values follows that limit.

/// How the pictures move: calm (the default: slower, at most two flashes a
/// second) or intense (livelier, still under three).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Motion {
    #[default]
    Calm,
    Intense,
    /// The desktop asks for reduced motion: calmer still.
    Reduced,
}

impl Motion {
    /// The least time between two turns of a value (s).
    pub fn turn(self) -> f32 {
        match self {
            Motion::Calm => 0.25,
            Motion::Intense => 0.17,
            Motion::Reduced => 0.4,
        }
    }

    /// Time constants of a rise and a fall (s).
    fn ease(self) -> (f32, f32) {
        match self {
            Motion::Calm => (0.06, 0.14),
            Motion::Intense => (0.03, 0.09),
            Motion::Reduced => (0.12, 0.25),
        }
    }

    /// The pictures' clock against the music's (calm: slower).
    pub fn pace(self) -> f32 {
        match self {
            Motion::Calm => 0.7,
            Motion::Intense => 1.0,
            Motion::Reduced => 0.45,
        }
    }
}

/// One guarded value.
#[derive(Clone, Copy, Debug, Default)]
pub struct Guard {
    pub value: f32,
    /// Rising (1), falling (-1) or still (0); seconds since it last turned.
    dir: i8,
    since: f32,
}

/// Changes smaller than this are no turn (noise round a level).
const STILL: f32 = 0.004;

impl Guard {
    /// Towards `target` over `dt` seconds, turning no sooner than the
    /// motion allows (held where it would).
    pub fn step(&mut self, target: f32, dt: f32, motion: Motion) -> f32 {
        self.since += dt;
        let want = target - self.value;
        let d = if want > STILL {
            1
        } else if want < -STILL {
            -1
        } else {
            0
        };
        if d != 0 && d != self.dir {
            if self.dir != 0 && self.since < motion.turn() {
                // (too soon to turn: held)
                return self.value;
            }
            self.dir = d;
            self.since = 0.0;
        }
        let (rise, fall) = motion.ease();
        let tau = if want > 0.0 { rise } else { fall };
        self.value += want * (1.0 - (-dt / tau).exp());
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Turns of a series (changes of direction of at least `step`).
    fn turns(series: &[f32], step: f32) -> usize {
        let (mut extreme, mut dir, mut n) = (series[0], 0i8, 0);
        for &v in series {
            if dir >= 0 && v > extreme || dir <= 0 && v < extreme {
                extreme = v;
            }
            if dir >= 0 && extreme - v >= step {
                n += 1;
                dir = -1;
                extreme = v;
            } else if dir <= 0 && v - extreme >= step {
                n += 1;
                dir = 1;
                extreme = v;
            }
        }
        n
    }

    #[test]
    fn a_strobe_comes_out_slower_than_three_flashes_a_second() {
        // a value switched on and off 8 times a second for 4 s, at 60 fps:
        // calm, at most 2 flashes (4 turns) a second; intense, under 3
        for (motion, most) in [(Motion::Calm, 2.0), (Motion::Intense, 3.0)] {
            let mut g = Guard::default();
            let out: Vec<f32> = (0..240)
                .map(|i| {
                    let on = (((i as f32 / 60.0) * 16.0) as u32).is_multiple_of(2);
                    g.step(if on { 1.0 } else { 0.0 }, 1.0 / 60.0, motion)
                })
                .collect();
            let flashes = turns(&out, 0.1) as f32 / 2.0 / 4.0;
            assert!(flashes <= most, "{motion:?}: {flashes} a second");
        }
    }

    #[test]
    fn a_slow_change_goes_through_and_does_not_jump() {
        let mut g = Guard::default();
        let mut last = 0.0;
        for i in 0..120 {
            let target = (i as f32 / 120.0 * std::f32::consts::PI).sin();
            let v = g.step(target, 1.0 / 60.0, Motion::Calm);
            assert!((v - last).abs() < 0.2, "a jump at {i}");
            last = v;
        }
        assert!(g.step(1.0, 1.0, Motion::Calm) > 0.9, "it gets there");
    }
}
