//! Which music track plays when: the menu theme, the seven map themes in shuffled rotation,
//! the two battle themes, and the triumph / defeat pieces. Pure logic (no macroquad): the
//! player ([`super::audio`]) asks for a change each frame, starts the track and reports how
//! long it lasts. macroquad has no "sound ended" callback, so the end is the start time plus
//! the track's length from its sample count.
//!
//! Tracks are named by their `_Sounds.ini` keys (`[Backgrounds]`).

pub const MENU: &str = "BkgMenuMain";
pub const MAP: [&str; 7] = ["BkgMap1", "BkgMap2", "BkgMap3", "BkgMap4", "BkgMap5", "BkgMap6", "BkgMap7"];
pub const BATTLE: [&str; 2] = ["BkgBattle1", "BkgBattle2"];
pub const TRIUMPH: &str = "BkgTriumph";
pub const DEFEAT: &str = "BkgDefeat";

/// What the current screen wants to hear.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mood {
    Silent,
    /// Scenario and class select: the menu theme, looped.
    Menu,
    /// World map and its windows: the map themes, one after another in random order.
    Map,
    /// The battle themes, alternating.
    Battle,
    /// The scenario is won: the triumph piece once.
    Won,
    /// The hero has fallen: the defeat piece once.
    Lost,
}

/// A request to the player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// Stop whatever plays and start this track.
    Play(&'static str),
    Stop,
}

#[derive(Clone, Copy, Debug)]
struct Current {
    track: &'static str,
    /// When it ends, in the player's clock; infinite until [`Jukebox::started`].
    ends: f64,
    /// A one-shot piece over the mood (triumph after a won battle).
    sting: bool,
}

pub struct Jukebox {
    /// Tracks that have a file and have not failed.
    available: Vec<&'static str>,
    mood: Mood,
    current: Option<Current>,
    /// The Won/Lost piece has played: silence until the mood changes.
    finished: bool,
    /// Map themes still to play in this round, last first.
    map_queue: Vec<&'static str>,
    last_map: Option<&'static str>,
    battle_next: usize,
    rng: u64,
}

impl Jukebox {
    /// `has(track)`: whether the install names a file for the track.
    pub fn new(has: impl Fn(&str) -> bool, seed: u64) -> Jukebox {
        let all = [MENU, TRIUMPH, DEFEAT].into_iter().chain(MAP).chain(BATTLE);
        Jukebox {
            available: all.filter(|t| has(t)).collect(),
            mood: Mood::Silent,
            current: None,
            finished: false,
            map_queue: Vec::new(),
            last_map: None,
            battle_next: 0,
            rng: seed | 1,
        }
    }

    /// The track playing (or being started), if any.
    #[cfg(test)]
    pub fn current(&self) -> Option<&'static str> {
        self.current.map(|c| c.track)
    }

    fn random(&mut self) -> u64 {
        // xorshift64
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    fn has(&self, track: &str) -> bool {
        self.available.contains(&track)
    }

    fn next_map(&mut self) -> Option<&'static str> {
        if self.map_queue.is_empty() {
            let mut round: Vec<&'static str> = MAP.into_iter().filter(|t| self.has(t)).collect();
            for i in (1..round.len()).rev() {
                let j = (self.random() % (i as u64 + 1)) as usize;
                round.swap(i, j);
            }
            // The queue pops from the end: never the same theme twice in a row.
            if round.len() > 1 && round.last().copied() == self.last_map {
                let last = round.len() - 1;
                round.swap(0, last);
            }
            self.map_queue = round;
        }
        let t = self.map_queue.pop()?;
        self.last_map = Some(t);
        Some(t)
    }

    fn next_battle(&mut self) -> Option<&'static str> {
        let tracks: Vec<&'static str> = BATTLE.into_iter().filter(|t| self.has(t)).collect();
        let t = *tracks.get(self.battle_next % tracks.len().max(1))?;
        self.battle_next += 1;
        Some(t)
    }

    /// The mood's next track, or a stop when it has none.
    fn next_for_mood(&mut self) -> Option<Change> {
        let track = match self.mood {
            Mood::Silent => None,
            Mood::Menu => Some(MENU).filter(|t| self.has(t)),
            Mood::Map => self.next_map(),
            Mood::Battle => self.next_battle(),
            Mood::Won => Some(TRIUMPH).filter(|t| self.has(t)),
            Mood::Lost => Some(DEFEAT).filter(|t| self.has(t)),
        };
        match track {
            Some(track) => {
                self.current = Some(Current { track, ends: f64::INFINITY, sting: false });
                Some(Change::Play(track))
            }
            None => self.current.take().map(|_| Change::Stop),
        }
    }

    /// Plays `track` once now, over the mood's music, which resumes after it (the triumph
    /// after a won battle). A mood change to anything but the map interrupts it.
    pub fn sting(&mut self, track: &'static str) -> Option<Change> {
        if !self.has(track) {
            return None;
        }
        self.current = Some(Current { track, ends: f64::INFINITY, sting: true });
        Some(Change::Play(track))
    }

    /// What to change for `mood` at time `now` (seconds).
    pub fn update(&mut self, mood: Mood, now: f64) -> Option<Change> {
        if mood != self.mood {
            self.mood = mood;
            self.finished = false;
            if mood == Mood::Battle {
                self.battle_next = (self.random() % BATTLE.len() as u64) as usize;
            }
            if let Some(c) = self.current.as_mut().filter(|c| c.sting) {
                if mood == Mood::Map {
                    return None;
                }
                // The battle that won the scenario: its triumph is the end piece.
                if mood == Mood::Won && c.track == TRIUMPH {
                    c.sting = false;
                    return None;
                }
            }
            return self.next_for_mood();
        }
        match self.current {
            Some(c) if now < c.ends => None,
            Some(c) => {
                self.current = None;
                if !c.sting && matches!(mood, Mood::Won | Mood::Lost) {
                    self.finished = true;
                    return None;
                }
                self.next_for_mood()
            }
            None if self.finished => None,
            None => self.next_for_mood(),
        }
    }

    /// The player started `track` at `now`; it lasts `secs`.
    pub fn started(&mut self, track: &'static str, now: f64, secs: f64) {
        if let Some(c) = self.current.as_mut().filter(|c| c.track == track) {
            c.ends = now + secs;
        }
    }

    /// `track` could not be played: it is never picked again.
    pub fn failed(&mut self, track: &'static str) {
        self.available.retain(|t| *t != track);
        self.map_queue.retain(|t| *t != track);
        if self.current.is_some_and(|c| c.track == track) {
            self.current = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Jukebox {
        Jukebox::new(|_| true, 42)
    }

    /// Applies `update` like the player: a started track lasts `secs`.
    fn step(j: &mut Jukebox, mood: Mood, now: f64, secs: f64) -> Option<Change> {
        let c = j.update(mood, now);
        if let Some(Change::Play(t)) = c {
            j.started(t, now, secs);
        }
        c
    }

    #[test]
    fn menu_theme_loops() {
        let mut j = all();
        assert_eq!(step(&mut j, Mood::Menu, 0.0, 10.0), Some(Change::Play(MENU)));
        assert_eq!(step(&mut j, Mood::Menu, 9.9, 10.0), None);
        assert_eq!(step(&mut j, Mood::Menu, 10.0, 10.0), Some(Change::Play(MENU)));
        assert_eq!(step(&mut j, Mood::Silent, 11.0, 10.0), Some(Change::Stop));
        assert_eq!(step(&mut j, Mood::Silent, 30.0, 10.0), None);
    }

    #[test]
    fn map_themes_rotate_without_repeats() {
        let mut j = all();
        let mut played = Vec::new();
        let mut now = 0.0;
        for _ in 0..70 {
            match step(&mut j, Mood::Map, now, 5.0) {
                Some(Change::Play(t)) => played.push(t),
                other => panic!("{other:?}"),
            }
            assert_eq!(step(&mut j, Mood::Map, now + 1.0, 5.0), None);
            now += 5.0;
        }
        for round in played.chunks(7) {
            let mut r = round.to_vec();
            r.sort();
            assert_eq!(r, MAP, "each round plays every theme once");
        }
        assert!(played.windows(2).all(|w| w[0] != w[1]));
        assert_ne!(played[..7], played[7..14], "shuffled");
    }

    #[test]
    fn battle_then_triumph_then_map() {
        let mut j = all();
        let Some(Change::Play(m)) = step(&mut j, Mood::Map, 0.0, 100.0) else { panic!() };
        assert!(MAP.contains(&m));
        let Some(Change::Play(b)) = step(&mut j, Mood::Battle, 1.0, 20.0) else { panic!() };
        assert!(BATTLE.contains(&b));
        // The battle theme ends: the other one follows.
        let Some(Change::Play(b2)) = step(&mut j, Mood::Battle, 21.0, 20.0) else { panic!() };
        assert_ne!(b, b2);
        // Won: the triumph plays over the map mood, which resumes after it.
        assert_eq!(j.sting(TRIUMPH), Some(Change::Play(TRIUMPH)));
        j.started(TRIUMPH, 30.0, 8.0);
        assert_eq!(step(&mut j, Mood::Map, 30.0, 100.0), None);
        assert_eq!(step(&mut j, Mood::Map, 37.0, 100.0), None);
        assert_eq!(j.current(), Some(TRIUMPH));
        let Some(Change::Play(m2)) = step(&mut j, Mood::Map, 38.0, 100.0) else { panic!() };
        assert!(MAP.contains(&m2));
    }

    #[test]
    fn a_battle_interrupts_the_triumph() {
        let mut j = all();
        step(&mut j, Mood::Map, 0.0, 100.0);
        j.sting(TRIUMPH);
        j.started(TRIUMPH, 1.0, 50.0);
        let Some(Change::Play(b)) = step(&mut j, Mood::Battle, 2.0, 20.0) else { panic!() };
        assert!(BATTLE.contains(&b));
    }

    #[test]
    fn the_last_battle_triumph_plays_once() {
        let mut j = all();
        step(&mut j, Mood::Battle, 0.0, 100.0);
        j.sting(TRIUMPH);
        j.started(TRIUMPH, 1.0, 8.0);
        assert_eq!(step(&mut j, Mood::Won, 1.0, 8.0), None);
        assert_eq!(step(&mut j, Mood::Won, 9.0, 8.0), None);
        assert_eq!(step(&mut j, Mood::Won, 20.0, 8.0), None);
        assert_eq!(j.current(), None);
    }

    #[test]
    fn end_pieces_play_once() {
        let mut j = all();
        assert_eq!(step(&mut j, Mood::Lost, 0.0, 5.0), Some(Change::Play(DEFEAT)));
        assert_eq!(step(&mut j, Mood::Lost, 5.0, 5.0), None);
        assert_eq!(step(&mut j, Mood::Lost, 60.0, 5.0), None);
        assert_eq!(j.current(), None);
        assert_eq!(step(&mut j, Mood::Menu, 61.0, 5.0), Some(Change::Play(MENU)));
        assert_eq!(step(&mut j, Mood::Won, 62.0, 5.0), Some(Change::Play(TRIUMPH)));
        assert_eq!(step(&mut j, Mood::Won, 70.0, 5.0), None);
    }

    #[test]
    fn missing_and_failed_tracks_are_skipped() {
        let mut j = Jukebox::new(|t| t == "BkgMap2" || t == "BkgMap5" || t == DEFEAT, 7);
        assert_eq!(step(&mut j, Mood::Menu, 0.0, 5.0), None);
        assert_eq!(step(&mut j, Mood::Battle, 0.0, 5.0), None);
        assert_eq!(j.sting(TRIUMPH), None);
        let Some(Change::Play(first)) = j.update(Mood::Map, 1.0) else { panic!() };
        j.failed(first);
        let other = if first == "BkgMap2" { "BkgMap5" } else { "BkgMap2" };
        for k in 0..5 {
            let now = 2.0 + k as f64 * 10.0;
            assert_eq!(step(&mut j, Mood::Map, now, 10.0), Some(Change::Play(other)));
        }
        j.failed(other);
        assert_eq!(step(&mut j, Mood::Map, 100.0, 10.0), None);
        assert_eq!(step(&mut j, Mood::Lost, 101.0, 10.0), Some(Change::Play(DEFEAT)));
    }
}
