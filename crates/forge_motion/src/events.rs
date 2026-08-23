//! Gameplay events derived from the take's own labels.
//!
//! ARDY marks each frame with per-side foot contact flags as it generates,
//! and those labels are ground truth for when a foot is planted — deriving
//! the same thing from poses would mean guessing a floor height and a
//! velocity threshold, twice removed from what the model actually decided.
//! This module turns those flags into the events a game consumes: footstep
//! times a sound or a decal can hang off.
//!
//! Times come back in *take* seconds. A built clip's timeline differs
//! whenever a recipe trims or retimes, so callers place events on a shipped
//! clip through [`crate::edit::Edit::map_time`].

/// Which side struck the ground.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Foot {
    /// The left heel or toe.
    Left,
    /// The right heel or toe.
    Right,
}

/// Frames a side must have been fully airborne before a touch-down counts
/// as a footstep.
///
/// ARDY's contact labels flicker: a planted foot can drop out for a single
/// frame mid-stance, and treating its return as a fresh strike would fire
/// footstep events machine-gun style through a standing pose. Two frames is
/// 100 ms at the 20 fps ARDY generates — longer than one-frame flicker by
/// construction, far shorter than any real swing phase, so genuine steps
/// always qualify.
const AIRBORNE_RUN_FRAMES: usize = 2;

/// Derive footstep times from per-frame contact flags.
///
/// Columns follow [`crate::Take::contacts`]: `[LeftFoot, LeftToeBase,
/// RightFoot, RightToeBase]`. A side is down when either its heel or its toe
/// touches — which of the two lands first varies with the gait, and a
/// heel-then-toe plant is one step, not two. A footstep is the first frame a
/// side is down after at least `AIRBORNE_RUN_FRAMES` (two) fully airborne
/// frames. A contact already present at frame 0 is stance the take started
/// in, not a strike, and frames 0 and 1 can never satisfy the airborne run —
/// so a clip that opens mid-plant stays silent until the foot genuinely
/// lifts and returns.
///
/// Events come back ordered by time, as `(side, frame / fps)` in seconds on
/// the take the contacts belong to.
///
/// # Panics
///
/// Panics if `fps` is not positive: a frame divided by no clock is not a
/// measurement, and inventing one would poison every event downstream.
#[must_use]
pub fn footsteps(contacts: &[[bool; 4]], fps: f32) -> Vec<(Foot, f32)> {
    assert!(fps > 0.0, "footsteps need a positive fps, got {fps}");

    let mut events = Vec::new();
    for (foot, heel, toe) in [(Foot::Left, 0, 1), (Foot::Right, 2, 3)] {
        let down: Vec<bool> = contacts.iter().map(|c| c[heel] || c[toe]).collect();
        for frame in AIRBORNE_RUN_FRAMES..down.len() {
            let airborne_run = down[frame - AIRBORNE_RUN_FRAMES..frame].iter().all(|d| !d);
            if down[frame] && airborne_run {
                events.push((foot, frame as f32 / fps));
            }
        }
    }
    events.sort_by(|a, b| a.1.total_cmp(&b.1));
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Contact rows with only one side active, from a compact string:
    /// `'-'` airborne, `'h'` heel, `'t'` toe, `'b'` both.
    fn side(pattern: &str, foot: Foot) -> Vec<[bool; 4]> {
        let (heel, toe) = match foot {
            Foot::Left => (0, 1),
            Foot::Right => (2, 3),
        };
        pattern
            .chars()
            .map(|c| {
                let mut row = [false; 4];
                match c {
                    '-' => {}
                    'h' => row[heel] = true,
                    't' => row[toe] = true,
                    'b' => {
                        row[heel] = true;
                        row[toe] = true;
                    }
                    other => panic!("bad pattern char {other}"),
                }
                row
            })
            .collect()
    }

    #[test]
    fn a_clean_strike_after_airborne_frames_is_an_event() {
        let contacts = side("---hh", Foot::Left);
        assert_eq!(footsteps(&contacts, 20.0), vec![(Foot::Left, 3.0 / 20.0)]);
    }

    #[test]
    fn one_frame_flicker_mid_stance_is_not_a_new_step() {
        // Planted, drops out for a single frame, returns: classic label
        // flicker. The return at frame 5 has only one airborne frame behind
        // it, so only the true strike at frame 3 counts.
        let contacts = side("---hh-hh", Foot::Left);
        assert_eq!(footsteps(&contacts, 20.0), vec![(Foot::Left, 3.0 / 20.0)]);
    }

    #[test]
    fn two_airborne_frames_between_strikes_is_a_real_second_step() {
        let contacts = side("---h--h", Foot::Right);
        assert_eq!(
            footsteps(&contacts, 20.0),
            vec![(Foot::Right, 3.0 / 20.0), (Foot::Right, 6.0 / 20.0)]
        );
    }

    #[test]
    fn heel_and_toe_landing_together_are_one_event() {
        let contacts = side("---b", Foot::Left);
        assert_eq!(footsteps(&contacts, 20.0), vec![(Foot::Left, 3.0 / 20.0)]);
    }

    #[test]
    fn toe_following_heel_within_a_plant_is_still_one_event() {
        let contacts = side("---htb", Foot::Right);
        assert_eq!(footsteps(&contacts, 20.0), vec![(Foot::Right, 3.0 / 20.0)]);
    }

    #[test]
    fn contact_at_frame_zero_is_stance_not_a_strike() {
        let contacts = side("hh---hh", Foot::Left);
        assert_eq!(footsteps(&contacts, 20.0), vec![(Foot::Left, 5.0 / 20.0)]);
    }

    #[test]
    fn a_strike_at_frame_one_has_no_airborne_run_either() {
        let contacts = side("-h", Foot::Right);
        assert!(footsteps(&contacts, 20.0).is_empty());
    }

    #[test]
    fn both_feet_interleave_in_time_order() {
        let left = side("---h----", Foot::Left);
        let right = side("------h-", Foot::Right);
        let contacts: Vec<[bool; 4]> = left
            .iter()
            .zip(&right)
            .map(|(l, r)| [l[0], l[1], r[2], r[3]])
            .collect();
        assert_eq!(
            footsteps(&contacts, 20.0),
            vec![(Foot::Left, 3.0 / 20.0), (Foot::Right, 6.0 / 20.0)]
        );
    }

    #[test]
    fn empty_contacts_yield_no_events() {
        assert!(footsteps(&[], 20.0).is_empty());
    }
}
