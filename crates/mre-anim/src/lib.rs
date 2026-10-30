//! Motion primitives: springs and easings.
//!
//! A spring, not a duration, is the default. Durations make an interface feel
//! mechanical: every transition takes the same time regardless of distance, and
//! an interrupted transition has to restart. A spring carries velocity, so
//! retargeting mid-flight is continuous, which is what makes a UI feel physical.
//!
//! The integrator is fixed-step semi-implicit Euler. Fixed substeps make the
//! result independent of frame rate and of frame-time jitter, which matters both
//! for feel and for deterministic tests.

/// Tuning for a [`Spring`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpringConfig {
    /// Spring constant. Higher is snappier.
    pub stiffness: f32,
    /// Damping coefficient. Lower overshoots more.
    pub damping: f32,
    /// Mass. Higher is heavier and slower.
    pub mass: f32,
    /// Distance below which the spring is considered at rest.
    pub rest_distance: f32,
    /// Speed below which the spring is considered at rest.
    pub rest_velocity: f32,
}

impl Default for SpringConfig {
    fn default() -> Self {
        Self::snappy()
    }
}

impl SpringConfig {
    /// Quick, slightly springy: the default for UI transitions.
    #[must_use]
    pub const fn snappy() -> Self {
        Self {
            stiffness: 320.0,
            damping: 30.0,
            mass: 1.0,
            rest_distance: 0.01,
            rest_velocity: 0.01,
        }
    }

    /// Smooth and calm, for large surfaces.
    #[must_use]
    pub const fn gentle() -> Self {
        Self {
            stiffness: 170.0,
            damping: 26.0,
            mass: 1.0,
            rest_distance: 0.01,
            rest_velocity: 0.01,
        }
    }

    /// Bouncy, for playful accents. Deliberately under-damped.
    #[must_use]
    pub const fn bouncy() -> Self {
        Self {
            stiffness: 400.0,
            damping: 18.0,
            mass: 1.0,
            rest_distance: 0.01,
            rest_velocity: 0.01,
        }
    }

    /// Critically damped: the fastest approach with no overshoot.
    #[must_use]
    pub fn critically_damped(stiffness: f32) -> Self {
        Self {
            stiffness,
            damping: 2.0 * stiffness.sqrt(),
            mass: 1.0,
            rest_distance: 0.01,
            rest_velocity: 0.01,
        }
    }
}

/// A scalar spring.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    config: SpringConfig,
    value: f32,
    velocity: f32,
    target: f32,
}

impl Spring {
    /// A spring at rest at `value`.
    #[must_use]
    pub fn new(value: f32) -> Self {
        Self::with_config(value, SpringConfig::default())
    }

    /// A spring at rest at `value`, with explicit tuning.
    #[must_use]
    pub fn with_config(value: f32, config: SpringConfig) -> Self {
        Self {
            config,
            value,
            velocity: 0.0,
            target: value,
        }
    }

    /// Current value.
    #[must_use]
    pub const fn value(&self) -> f32 {
        self.value
    }

    /// Current velocity, in units per second.
    #[must_use]
    pub const fn velocity(&self) -> f32 {
        self.velocity
    }

    /// Where the spring is heading.
    #[must_use]
    pub const fn target(&self) -> f32 {
        self.target
    }

    /// Current tuning.
    #[must_use]
    pub const fn config(&self) -> SpringConfig {
        self.config
    }

    /// Retargets the spring, keeping its velocity.
    ///
    /// This is what makes an interrupted animation look right: the existing
    /// motion is preserved and redirected rather than discarded.
    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    /// Jumps to a value and stops.
    pub fn reset(&mut self, value: f32) {
        self.value = value;
        self.target = value;
        self.velocity = 0.0;
    }

    /// True when the spring is at rest.
    #[must_use]
    pub fn settled(&self) -> bool {
        (self.target - self.value).abs() <= self.config.rest_distance
            && self.velocity.abs() <= self.config.rest_velocity
    }

    /// Advances the spring by `dt` seconds and returns the new value.
    ///
    /// `dt` is clamped to 250 ms so that a stalled frame cannot launch the
    /// spring into instability.
    pub fn step(&mut self, dt: f32) -> f32 {
        if !dt.is_finite() || dt <= 0.0 {
            return self.value;
        }
        if self.settled() {
            self.value = self.target;
            self.velocity = 0.0;
            return self.value;
        }

        const MAX_SUBSTEP: f32 = 1.0 / 240.0;
        let mut remaining = dt.min(0.25);
        while remaining > 0.0 {
            let step = remaining.min(MAX_SUBSTEP);
            let force = self.config.stiffness * (self.target - self.value)
                - self.config.damping * self.velocity;
            let acceleration = force / self.config.mass.max(1.0e-6);
            self.velocity += acceleration * step;
            self.value += self.velocity * step;
            remaining -= step;
        }

        if self.settled() {
            self.value = self.target;
            self.velocity = 0.0;
        }
        self.value
    }
}

/// A time-based easing curve.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Easing {
    /// Constant speed.
    Linear,
    /// Slow start.
    #[default]
    EaseIn,
    /// Slow end.
    EaseOut,
    /// Slow start and end.
    EaseInOut,
    /// Overshoots, then settles.
    BackOut,
}

impl Easing {
    /// Samples the curve at `t` in `0.0..=1.0`.
    #[must_use]
    pub fn sample(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::EaseIn => t * t * t,
            Self::EaseOut => {
                let inverted = 1.0 - t;
                1.0 - inverted * inverted * inverted
            }
            Self::EaseInOut => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    let inverted = -2.0 * t + 2.0;
                    1.0 - inverted * inverted * inverted / 2.0
                }
            }
            Self::BackOut => {
                const C1: f32 = 1.701_58;
                let c3 = C1 + 1.0;
                let inverted = t - 1.0;
                1.0 + c3 * inverted * inverted * inverted + C1 * inverted * inverted
            }
        }
    }
}

/// A one-shot time-based animation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tween {
    from: f32,
    to: f32,
    elapsed: f32,
    duration: f32,
    easing: Easing,
}

impl Tween {
    /// A tween from one value to another.
    #[must_use]
    pub fn new(from: f32, to: f32, duration: f32, easing: Easing) -> Self {
        Self {
            from,
            to,
            elapsed: 0.0,
            duration: duration.max(1.0e-4),
            easing,
        }
    }

    /// Current value.
    #[must_use]
    pub fn value(&self) -> f32 {
        let t = (self.elapsed / self.duration).clamp(0.0, 1.0);
        self.from + (self.to - self.from) * self.easing.sample(t)
    }

    /// True when the tween has reached its end.
    #[must_use]
    pub fn finished(&self) -> bool {
        self.elapsed >= self.duration
    }

    /// Advances by `dt` seconds.
    pub fn step(&mut self, dt: f32) -> f32 {
        if dt.is_finite() && dt > 0.0 {
            self.elapsed = (self.elapsed + dt).min(self.duration);
        }
        self.value()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spring_reaches_its_target() {
        let mut spring = Spring::new(0.0);
        spring.set_target(100.0);

        let mut elapsed = 0.0;
        while !spring.settled() && elapsed < 2.0 {
            spring.step(1.0 / 60.0);
            elapsed += 1.0 / 60.0;
        }

        assert!(spring.settled(), "spring did not settle in 2 s");
        assert_eq!(spring.value(), 100.0);
        assert!(elapsed < 1.0, "spring took {elapsed}s");
    }

    #[test]
    fn a_bouncy_spring_overshoots() {
        let mut spring = Spring::with_config(0.0, SpringConfig::bouncy());
        spring.set_target(100.0);

        let mut peak: f32 = 0.0;
        for _ in 0..120 {
            peak = peak.max(spring.step(1.0 / 60.0));
        }

        assert!(
            peak > 100.0,
            "bouncy spring should overshoot, peaked at {peak}"
        );
    }

    #[test]
    fn retargeting_keeps_velocity_continuous() {
        let mut spring = Spring::new(0.0);
        spring.set_target(100.0);
        for _ in 0..10 {
            spring.step(1.0 / 60.0);
        }
        let velocity = spring.velocity();
        spring.set_target(-100.0);
        assert_eq!(spring.velocity(), velocity);
    }

    #[test]
    fn stepping_is_frame_rate_independent() {
        let mut fast = Spring::new(0.0);
        fast.set_target(50.0);
        let mut slow = Spring::new(0.0);
        slow.set_target(50.0);

        for _ in 0..60 {
            fast.step(1.0 / 60.0);
        }
        for _ in 0..30 {
            slow.step(1.0 / 30.0);
        }

        assert!(
            (fast.value() - slow.value()).abs() < 1.0,
            "{} vs {}",
            fast.value(),
            slow.value()
        );
    }

    #[test]
    fn a_huge_frame_delta_is_clamped() {
        let mut spring = Spring::new(0.0);
        spring.set_target(1.0);
        let value = spring.step(10.0);
        assert!(value.is_finite());
        assert!(value.abs() < 100.0);
    }

    #[test]
    fn easings_start_at_zero_and_end_at_one() {
        for easing in [
            Easing::Linear,
            Easing::EaseIn,
            Easing::EaseOut,
            Easing::EaseInOut,
            Easing::BackOut,
        ] {
            assert!((easing.sample(0.0) - 0.0).abs() < 1.0e-6);
            assert!((easing.sample(1.0) - 1.0).abs() < 1.0e-6);
        }
    }

    #[test]
    fn a_tween_finishes_exactly_on_its_target() {
        let mut tween = Tween::new(0.0, 10.0, 0.2, Easing::EaseOut);
        for _ in 0..20 {
            tween.step(1.0 / 60.0);
        }
        assert!(tween.finished());
        assert_eq!(tween.value(), 10.0);
    }
}
