// Macro recorder (Java's core/Recorder, beta): samples keys, rotation and
// hotbar slot once per tick while no screen is open, then compresses the
// samples into workflow steps (move / wait / look / hotbar / attack / use).
// Checked against tests/fixtures/workflows.json (recorder_*). Added to the
// game as `game::omnitool_recorder`; also a module of this crate.

use super::omnitool_workflows::MOVE_KEYS;

/// One tick: bit i of `keys` = `MOVE_KEYS[i]` is held.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub keys: u32,
    pub yaw: f32,
    pub pitch: f32,
    pub slot: i32,
}

/// Attack/use held at most this many ticks counts as a single click.
pub const CLICK_TICKS: usize = 3;
/// Rotation change (degrees) that starts a new segment with a new look step.
pub const TURN_SPLIT: f32 = 5.0;
/// Recording stops by itself after 20 minutes.
pub const MAX_TICKS: usize = 20 * 60 * 20;
const ATTACK: u32 = 7;
const USE: u32 = 8;

/// Java's `Recorder` state: the workflow recorded into and its samples.
#[derive(Default)]
pub struct Recorder {
    pub target: Option<u64>,
    pub samples: Vec<Sample>,
}

impl Recorder {
    pub fn recording(&self, uid: u64) -> bool {
        self.target == Some(uid)
    }

    pub fn start(&mut self, uid: u64) {
        self.samples.clear();
        self.target = Some(uid);
    }

    /// Ends the recording: the workflow and its new steps.
    pub fn stop(&mut self) -> Option<(u64, Vec<String>)> {
        let target = self.target.take()?;
        let steps = compress(&self.samples);
        self.samples.clear();
        Some((target, steps))
    }

    /// Adds a tick's sample; whether the action bar shows the progress now
    /// and whether the limit is reached.
    pub fn add(&mut self, s: Sample) -> (bool, bool) {
        self.samples.push(s);
        let n = self.samples.len();
        (n % 10 == 1, n >= MAX_TICKS)
    }
}

/// Java's `compress`: per-tick samples to steps; idle time at both ends is dropped.
pub fn compress(samples: &[Sample]) -> Vec<String> {
    let n = samples.len();
    let mut out = Vec::new();
    if n == 0 {
        return out;
    }
    let mut mask: Vec<u32> = samples.iter().map(|s| s.keys).collect();
    // short attack/use presses become single clicks
    let mut click = [vec![false; n], vec![false; n]];
    for (b, bit) in [ATTACK, USE].into_iter().enumerate() {
        let bit = 1 << bit;
        for s in 0..n {
            if mask[s] & bit == 0 || (s > 0 && samples[s - 1].keys & bit != 0) {
                continue;
            }
            let mut e = s;
            while e < n && samples[e].keys & bit != 0 {
                e += 1;
            }
            if e - s <= CLICK_TICKS {
                for m in &mut mask[s..e] {
                    *m &= !bit;
                }
                click[b][s] = true;
            }
        }
    }
    let active = |i: usize| {
        mask[i] != 0
            || click[0][i]
            || click[1][i]
            || (i > 0 && samples[i].slot != samples[i - 1].slot)
    };
    let Some(first) = (0..n).find(|&i| active(i)) else {
        return out;
    };
    let end = (0..n).rfind(|&i| active(i)).unwrap_or(first) + 1;
    let mut last_slot = samples[0].slot;
    let (mut last_yaw, mut last_pitch) = (f32::NAN, f32::NAN);
    let mut i = first;
    while i < end {
        let s = samples[i];
        if s.slot != last_slot {
            out.push(format!("hotbar:{}", s.slot + 1));
            last_slot = s.slot;
        }
        if last_yaw.is_nan()
            || angle(s.yaw, last_yaw).abs() > 0.5
            || (s.pitch - last_pitch).abs() > 0.5
        {
            out.push(format!(
                "look:{} {}",
                half_up(wrap(s.yaw).into(), 1),
                half_up(s.pitch.into(), 1)
            ));
            last_yaw = s.yaw;
            last_pitch = s.pitch;
        }
        if click[0][i] {
            out.push("attack".into());
        }
        if click[1][i] {
            out.push("use".into());
        }
        let mut j = i + 1;
        while j < end {
            let t = samples[j];
            if mask[j] != mask[i]
                || t.slot != s.slot
                || click[0][j]
                || click[1][j]
                || angle(t.yaw, s.yaw).abs() > TURN_SPLIT
                || (t.pitch - s.pitch).abs() > TURN_SPLIT
            {
                break;
            }
            j += 1;
        }
        out.push(if mask[i] == 0 {
            format!("wait:{}", seconds(j - i))
        } else {
            format!("move:{} {}", keys(mask[i]), seconds(j - i))
        });
        i = j;
    }
    out
}

/// Key names joined with + for a bit mask.
pub fn keys(mask: u32) -> String {
    MOVE_KEYS
        .iter()
        .enumerate()
        .filter(|(i, _)| mask & (1 << i) != 0)
        .map(|(_, k)| *k)
        .collect::<Vec<_>>()
        .join("+")
}

/// `1.2s`, `0.05s`, `3s` for a number of ticks.
pub fn seconds(ticks: usize) -> String {
    let v = half_up(ticks as f64 / 20.0, 2);
    let v = v.trim_end_matches('0').trim_end_matches('.');
    format!("{v}s")
}

/// Shortest signed difference a - b in degrees.
fn angle(a: f32, b: f32) -> f32 {
    wrap(a - b)
}

/// Degrees in [-180, 180), never -0.
fn wrap(deg: f32) -> f32 {
    let d = ((deg % 360.0) + 540.0) % 360.0 - 180.0;
    if d == 0.0 { 0.0 } else { d }
}

/// Java's `String.format(Locale.ROOT, "%.Nf", v)`: HALF_UP on the shortest
/// decimal form of the value.
pub fn half_up(v: f64, decimals: usize) -> String {
    let s = format!("{}", v.abs());
    let (int, frac) = s.split_once('.').unwrap_or((&s, ""));
    let mut digits: Vec<u8> = int
        .bytes()
        .chain(frac.bytes().chain(std::iter::repeat(b'0')).take(decimals))
        .collect();
    if frac.as_bytes().get(decimals).is_some_and(|&d| d >= b'5') {
        let mut k = digits.len();
        loop {
            if k == 0 {
                digits.insert(0, b'1');
                break;
            }
            k -= 1;
            if digits[k] == b'9' {
                digits[k] = b'0';
            } else {
                digits[k] += 1;
                break;
            }
        }
    }
    let split = digits.len() - decimals;
    let (i, f) = digits.split_at(split);
    let mut out = String::new();
    if v.is_sign_negative() {
        out.push('-');
    }
    out.push_str(std::str::from_utf8(i).unwrap_or("0"));
    if decimals > 0 {
        out.push('.');
        out.push_str(std::str::from_utf8(f).unwrap_or(""));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omnitool_workflows::tests::fixtures;

    #[test]
    fn compress_matches_java() {
        let f = fixtures();
        for c in f["recorder_compress"].as_array().unwrap() {
            let samples: Vec<Sample> = c["samples"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| Sample {
                    keys: s[0].as_u64().unwrap() as u32,
                    yaw: s[1].as_f64().unwrap() as f32,
                    pitch: s[2].as_f64().unwrap() as f32,
                    slot: s[3].as_i64().unwrap() as i32,
                })
                .collect();
            let want: Vec<&str> = c["steps"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s.as_str().unwrap())
                .collect();
            assert_eq!(compress(&samples), want, "{}", c["name"]);
        }
        for c in f["recorder_seconds"].as_array().unwrap() {
            assert_eq!(
                seconds(c["ticks"].as_u64().unwrap() as usize),
                c["text"].as_str().unwrap()
            );
        }
        for c in f["recorder_keys"].as_array().unwrap() {
            assert_eq!(
                keys(c["mask"].as_u64().unwrap() as u32),
                c["keys"].as_str().unwrap()
            );
        }
    }

    #[test]
    fn java_rounding() {
        assert_eq!(half_up(0.125, 2), "0.13");
        assert_eq!(half_up(-0.04, 1), "-0.0");
        assert_eq!(half_up(9.96, 1), "10.0");
        assert_eq!(half_up(90.0, 1), "90.0");
    }

    #[test]
    fn recorder_appends_steps() {
        let mut r = Recorder::default();
        r.start(4);
        assert!(r.recording(4));
        let s = Sample {
            keys: 1,
            yaw: 0.0,
            pitch: 0.0,
            slot: 0,
        };
        assert_eq!(r.add(s), (true, false));
        assert_eq!(r.add(s), (false, false));
        assert_eq!(
            r.stop(),
            Some((4, vec!["look:0.0 0.0".into(), "move:forward 0.1s".into()]))
        );
        assert_eq!(r.stop(), None);
    }
}
