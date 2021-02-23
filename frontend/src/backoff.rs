use js_sys::Math;
use std::time::Duration;

pub struct Backoff {
    attempts: u16,
}

impl Backoff {
    pub fn new() -> Backoff {
        Self { attempts: 0 }
    }

    pub fn success(&mut self) {
        self.attempts = 0
    }

    pub fn delay(&mut self) -> Duration {
        const BACKOFF_INITIAL: f64 = 0.5; // seconds
        const BACKOFF_MAX: f64 = 10.0 * 60.0; // seconds
        const BACKOFF_MULTIPLIER: f64 = 1.5;
        const BACKOFF_RANDOMIZATION: f64 = 0.5;

        let retry = BACKOFF_INITIAL * BACKOFF_MULTIPLIER.powi(self.attempts as i32);
        let limited = retry.min(BACKOFF_MAX);
        // randomness is BACKOFF_RANDOMIZATION wide centered around 1.0
        let spread = Math::random() * BACKOFF_RANDOMIZATION;
        let rand = spread + 1.0 - BACKOFF_RANDOMIZATION / 2.0;
        let delay = limited * rand;
        self.attempts = self.attempts.saturating_add(1);
        Duration::from_secs_f64(delay)
    }
}
