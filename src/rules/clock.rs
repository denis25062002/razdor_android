//! Game time. It only advances while the party travels.

const MINUTES_PER_DAY: u32 = 24 * 60;
const WEEKDAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clock {
    /// Minutes since day 1, 00:00.
    minutes: f64,
}

impl Clock {
    /// Day 1 (Monday), 08:00.
    pub fn start() -> Self {
        Clock { minutes: 8.0 * 60.0 }
    }

    pub fn total_minutes(&self) -> f64 {
        self.minutes
    }

    fn whole(&self) -> u32 {
        self.minutes as u32
    }

    /// 1-based day number.
    pub fn day(&self) -> u32 {
        self.whole() / MINUTES_PER_DAY + 1
    }

    pub fn weekday(&self) -> &'static str {
        WEEKDAYS[((self.day() - 1) % 7) as usize]
    }

    pub fn hour(&self) -> u32 {
        self.whole() % MINUTES_PER_DAY / 60
    }

    pub fn minute(&self) -> u32 {
        self.whole() % 60
    }

    /// Advance and return how many midnights were crossed.
    pub fn advance(&mut self, minutes: f64) -> u32 {
        let before = self.day();
        self.minutes += minutes.max(0.0);
        self.day() - before
    }

    pub fn label(&self) -> String {
        format!("Day {}, {} {:02}:{:02}", self.day(), self.weekday(), self.hour(), self.minute())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_monday_morning() {
        let c = Clock::start();
        assert_eq!(c.label(), "Day 1, Monday 08:00");
    }

    #[test]
    fn advance_reports_midnights() {
        let mut c = Clock::start();
        assert_eq!(c.advance(15.0 * 60.0), 0); // 23:00
        assert_eq!(c.advance(60.0), 1); // 00:00 day 2
        assert_eq!(c.label(), "Day 2, Tuesday 00:00");
        assert_eq!(c.advance(3.0 * 24.0 * 60.0), 3);
        assert_eq!(c.weekday(), "Friday");
    }
}
