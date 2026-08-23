//! The edit knobs, applied to a take in memory.
//!
//! These mirrored `tools/ardy_npz_to_glb.py` exactly, because a clip you tune
//! in the app and a clip you promote from the CLI must be the same clip. The
//! ordering below is not arbitrary and not interchangeable — `exaggerate`
//! measures a mean pose over the whole clip, so it has to see the style offsets
//! already layered in, and `wrap_blend` has to run last or the blend it just
//! made gets overwritten.
//!
//! This is the only implementation of the edit: the Python it was originally
//! pinned against retired with the Blender bake, and `just audit` — every
//! shipped clip rebuilding from its own recipe within a millimetre — is what
//! keeps the maths honest now.
//!
//! # One frame convention, converted here and nowhere else
//!
//! ARDY generates takes with the character facing **+Z**; the rig contract —
//! and everything a game sees — faces **−Z** (Bevy's forward). The last thing
//! [`Edit::apply`] does, after every knob, is turn the whole pose 180° about
//! Y: the root joint's orientation and the root travel together. Everything
//! downstream of a built take — the bake, the studio preview, the measured
//! root-motion track in the sidecar and manifest — is therefore in **rig
//! space**, and none of them carries its own compensation. Raw, un-applied
//! takes are the only +Z data left, and they never leave the authoring side.
//! Character-relative quantities (foot contacts, styling, travel headings)
//! are invariant under the turn.

use glam::{Quat, Vec3};

use crate::{Take, skeleton};

/// How net travel is removed from the root.
///
/// Not a bool: on `gen_roll` the difference between [`Self::Strip`] and
/// [`Self::Detrend`] is 417 mm of hip travel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum InPlace {
    /// Keep the take's travel. The root moves as generated.
    #[default]
    Off,
    /// Pin the hips' X/Z entirely. Right for steady locomotion loops.
    Strip,
    /// Remove only the linear drift, keeping within-clip surges. Right for
    /// travelling one-shots like rolls, where the body genuinely lunges and
    /// settles around a constant gameplay velocity.
    Detrend,
}

/// How root **height** is treated.
///
/// Separate knob from [`InPlace`], which only ever touches X/Z: jump, land and
/// vault takes carry their rise and fall in root Y, and a game that moves the
/// capsule itself would lift the character twice.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum YMode {
    /// Keep the take's Y. The root rises and falls as generated.
    #[default]
    Off,
    /// Pin root Y to frame 0's value. Flattens dips as well as travel, so it
    /// is right only where every departure from the standing height is the
    /// game's business.
    Strip,
    /// Remove only the linear drift from first frame to last, keeping surges
    /// like a landing crouch. Right for takes that end at a different height
    /// than they start but still have real vertical motion inside them.
    Detrend,
}

/// A full edit recipe: everything that turns a raw take into a shipped clip.
///
/// [`Self::default`] is the identity edit, so a take passed through it comes out
/// unchanged apart from the recentring every clip gets.
#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    /// Frames dropped from the start.
    pub trim_start: usize,
    /// Frames dropped from the end.
    pub trim_end: usize,
    /// Piecewise-linear `(source_s, output_s)` re-pacing keypoints. Empty
    /// leaves the timing alone.
    pub retime: Vec<(f32, f32)>,
    /// Root travel treatment. X/Z only.
    pub in_place: InPlace,
    /// Root height treatment. The only *knob* that targets root Y — though
    /// `loop_blend_s` also eases the last frames' height into frame 0's as
    /// part of its seam blend.
    pub y_mode: YMode,
    /// Elbow bend, degrees. Positive swings the forearms forward.
    pub arm_bend_deg: f32,
    /// Forward lean, degrees, spread over three spine joints so it reads as a
    /// lean rather than a kink.
    pub lean_deg: f32,
    /// Upper-arm pull-back, degrees, so the hands ride beside the hips.
    pub shoulder_back_deg: f32,
    /// Scale on each arm joint's deviation from its clip-mean pose. `1.0` is
    /// unchanged; above that amplifies arm pump.
    pub exaggerate: f32,
    /// Seconds of tail blended back toward frame 0 to close a loop.
    pub loop_blend_s: f32,
}

impl Default for Edit {
    fn default() -> Self {
        Self {
            trim_start: 0,
            trim_end: 0,
            retime: Vec::new(),
            in_place: InPlace::Off,
            y_mode: YMode::Off,
            arm_bend_deg: 0.0,
            lean_deg: 0.0,
            shoulder_back_deg: 0.0,
            exaggerate: 1.0,
            loop_blend_s: 0.0,
        }
    }
}

/// How [`Edit::lean_deg`] is spread down the spine.
const LEAN_SPLIT: [(&str, f32); 3] = [("Spine", 0.4), ("Spine1", 0.35), ("Spine2", 0.25)];

impl Edit {
    /// Whether this recipe would leave the take untouched.
    #[must_use]
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }

    /// Apply the whole recipe, returning a new take **in rig space** (facing
    /// −Z; see the module docs — the 180° turn is the final stage of every
    /// apply, including the identity edit).
    ///
    /// Returns the take otherwise unchanged if the trim would leave fewer
    /// than 2 frames, rather than producing a clip that cannot be sampled.
    #[must_use]
    pub fn apply(&self, take: &Take) -> Take {
        let total = take.frames();
        let start = self.trim_start.min(total);
        let end = total.saturating_sub(self.trim_end);
        if end.saturating_sub(start) < 2 || take.fps <= 0.0 {
            let mut out = take.clone();
            to_rig_space(&mut out);
            return out;
        }

        let mut out = Take {
            rotations: take.rotations[start..end].to_vec(),
            root: take.root[start..end].to_vec(),
            // Contacts ride the same window as the motion. The stages after
            // retime never touch them: in_place moves the root, style and
            // exaggerate rotate joints, and wrap_blend nudges poses toward
            // frame 0 without changing the frame count — none of which alters
            // when a foot was on the ground.
            contacts: take.contacts.as_ref().map(|c| c[start..end].to_vec()),
            fps: take.fps,
            prompt: take.prompt.clone(),
        };

        if !self.retime.is_empty() {
            retime(&mut out, &self.retime);
        }

        // Recentre unconditionally. ARDY's root positions are absolute world
        // coordinates, so a window cut from the middle of a take starts metres
        // from the origin — which shows up as a character standing beside its
        // own collider. Y is left alone: that is real height above the floor.
        let origin = out.root[0];
        for p in &mut out.root {
            p.x -= origin.x;
            p.z -= origin.z;
        }

        self.apply_in_place(&mut out);
        self.apply_y_mode(&mut out);
        self.apply_style(&mut out);
        if (self.exaggerate - 1.0).abs() > f32::EPSILON {
            self.apply_exaggerate(&mut out);
        }
        if self.loop_blend_s > 0.0 {
            wrap_blend(&mut out, self.loop_blend_s);
        }
        to_rig_space(&mut out);
        out
    }

    /// Map a time on the source take to its home on the built clip, seconds
    /// to seconds.
    ///
    /// An event authored against the raw take — a reviewer marking the frame
    /// a pistol fires, a footstep read off a contact strip — has to land on
    /// the clip that ships, and that landing spot moves every time a knob
    /// does. Only two knobs move the clock: `trim_start` shifts it, and
    /// `retime` warps it. So the map is: subtract the trimmed-away lead-in,
    /// then evaluate the retime keypoints **forward** — piecewise-linear with
    /// clamped ends, `np.interp` over the same `(source_s, output_s)` pairs
    /// [`Self::apply`] inverts when it resamples frames. Every other stage
    /// edits poses or the root track and is a time no-op.
    ///
    /// Returns `None` for a time in the lead-in `trim_start` removed, and for
    /// a non-positive `fps` (no clock to map against). The *end* of the kept
    /// window is not judged here: `trim_end` counts frames off a take whose
    /// length this recipe alone does not know, so times past the tail come
    /// back mapped, and the caller holding the built clip's duration owns
    /// that bound.
    #[must_use]
    pub fn map_time(&self, t_src_s: f32, fps: f32) -> Option<f32> {
        if fps <= 0.0 {
            return None;
        }
        let shifted = t_src_s - self.trim_start as f32 / fps;
        if shifted < 0.0 {
            return None;
        }
        if self.retime.is_empty() {
            return Some(shifted);
        }
        let src: Vec<f32> = self.retime.iter().map(|m| m.0).collect();
        let dst: Vec<f32> = self.retime.iter().map(|m| m.1).collect();
        Some(interp(shifted, &src, &dst))
    }

    fn apply_in_place(&self, take: &mut Take) {
        if self.in_place == InPlace::Off {
            return;
        }
        let frames = take.root.len();
        // A retime can collapse the clip to a single frame; detrend's
        // `/(frames - 1)` would then poison every root position with NaN.
        if frames < 2 {
            return;
        }
        let last = take.root[frames - 1];
        for (i, p) in take.root.iter_mut().enumerate() {
            match self.in_place {
                InPlace::Off => {}
                InPlace::Strip => {
                    p.x = 0.0;
                    p.z = 0.0;
                }
                InPlace::Detrend => {
                    let k = i as f32 / (frames - 1) as f32;
                    p.x -= last.x * k;
                    p.z -= last.z * k;
                }
            }
        }
    }

    /// The Y counterpart of [`Self::apply_in_place`]. The recentre above
    /// leaves Y alone on purpose; the only other stage that touches root
    /// height is `wrap_blend`'s seam ease on looped clips.
    ///
    /// The drift is measured against frame 0 explicitly:
    /// [`Self::apply_in_place`] can subtract `last * k` because the recentre
    /// already put frame 0 at zero on X/Z, and `(last - first) * k` is that
    /// same linear drift written for an axis with no such head start.
    fn apply_y_mode(&self, take: &mut Take) {
        if self.y_mode == YMode::Off {
            return;
        }
        let frames = take.root.len();
        // Same single-frame guard as `apply_in_place`: detrend's
        // `/(frames - 1)` is NaN on a one-frame clip.
        if frames < 2 {
            return;
        }
        let first = take.root[0].y;
        let last = take.root[frames - 1].y;
        for (i, p) in take.root.iter_mut().enumerate() {
            match self.y_mode {
                YMode::Off => {}
                YMode::Strip => p.y = first,
                YMode::Detrend => {
                    let k = i as f32 / (frames - 1) as f32;
                    p.y -= (last - first) * k;
                }
            }
        }
    }

    /// Constant pose offsets layered on top of the motion.
    ///
    /// Post-multiplied, so they ride on whatever the clip is already doing
    /// rather than replacing it.
    fn apply_style(&self, take: &mut Take) {
        let mut post = |joint: &str, rotation: Quat| {
            if let Some(j) = skeleton::index_of(joint) {
                for frame in &mut take.rotations {
                    frame[j] *= rotation;
                }
            }
        };

        if self.shoulder_back_deg != 0.0 {
            // T-pose upper arms point along ±X, so a -Y rotation swings the
            // right one behind the torso; mirrored for the left.
            let a = self.shoulder_back_deg.to_radians();
            post("RightArm", Quat::from_rotation_y(-a));
            post("LeftArm", Quat::from_rotation_y(a));
        }
        if self.arm_bend_deg != 0.0 {
            let a = self.arm_bend_deg.to_radians();
            post("RightForeArm", Quat::from_rotation_y(a));
            post("LeftForeArm", Quat::from_rotation_y(-a));
        }
        if self.lean_deg != 0.0 {
            for (joint, fraction) in LEAN_SPLIT {
                post(
                    joint,
                    Quat::from_rotation_x((self.lean_deg * fraction).to_radians()),
                );
            }
        }
    }

    fn apply_exaggerate(&self, take: &mut Take) {
        for joint in skeleton::ARM_JOINTS {
            let Some(j) = skeleton::index_of(joint) else {
                continue;
            };
            let track: Vec<Quat> = take.rotations.iter().map(|f| f[j]).collect();
            let mean = quat_mean(&track);
            let inverse = mean.inverse();
            for (frame, q) in take.rotations.iter_mut().zip(track) {
                let scaled = from_rotvec(to_rotvec(inverse * q) * self.exaggerate);
                frame[j] = mean * scaled;
            }
        }
    }
}

/// The chordal L2 quaternion mean: the dominant eigenvector of `Σ qqᵀ`.
///
/// This is what `scipy`'s `Rotation.mean()` computes, and matching it is why
/// `exaggerate` is the knob the parity test watches most closely. Power
/// iteration is used rather than a full eigendecomposition — the matrix is 4×4,
/// symmetric and positive semi-definite, so it converges quickly. The result's
/// sign is arbitrary, which does not matter: `q` and `-q` are the same rotation.
fn quat_mean(rotations: &[Quat]) -> Quat {
    let mut m = [[0.0f64; 4]; 4];
    for q in rotations {
        let v = [
            f64::from(q.x),
            f64::from(q.y),
            f64::from(q.z),
            f64::from(q.w),
        ];
        for (i, row) in m.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell += v[i] * v[j];
            }
        }
    }

    let first = rotations[0];
    let mut v = [
        f64::from(first.x),
        f64::from(first.y),
        f64::from(first.z),
        f64::from(first.w),
    ];
    for _ in 0..256 {
        let mut next = [0.0f64; 4];
        for (i, row) in m.iter().enumerate() {
            for (j, cell) in row.iter().enumerate() {
                next[i] += cell * v[j];
            }
        }
        let norm = next.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm < 1e-12 {
            break;
        }
        for (slot, value) in v.iter_mut().zip(next) {
            *slot = value / norm;
        }
    }

    Quat::from_xyzw(v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32).normalize()
}

/// Rotation vector (axis × angle), angle in `[0, π]`, as `scipy`'s `as_rotvec`.
fn to_rotvec(q: Quat) -> Vec3 {
    // Canonicalise to the short way around, or scaling a 350° rotation would
    // amplify the long path instead of the -10° one anybody means.
    let q = if q.w < 0.0 { -q } else { q };
    let sin_half = (1.0 - q.w * q.w).max(0.0).sqrt();
    let axis = Vec3::new(q.x, q.y, q.z);
    if sin_half < 1e-7 {
        axis * 2.0 // small-angle: sin(θ/2) ≈ θ/2
    } else {
        axis * (2.0 * q.w.clamp(-1.0, 1.0).acos() / sin_half)
    }
}

/// Inverse of [`to_rotvec`].
fn from_rotvec(v: Vec3) -> Quat {
    let angle = v.length();
    if angle < 1e-7 {
        Quat::IDENTITY
    } else {
        Quat::from_axis_angle(v / angle, angle)
    }
}

/// Piecewise-linear interpolation with clamped ends, as `np.interp`.
fn interp(x: f32, xp: &[f32], fp: &[f32]) -> f32 {
    if x <= xp[0] {
        return fp[0];
    }
    for w in 1..xp.len() {
        if x <= xp[w] {
            let span = xp[w] - xp[w - 1];
            if span.abs() < f32::EPSILON {
                return fp[w];
            }
            let t = (x - xp[w - 1]) / span;
            return fp[w - 1] + t * (fp[w] - fp[w - 1]);
        }
    }
    fp[fp.len() - 1]
}

/// Nonlinear re-pacing against a `(source_s, output_s)` map — compress a slow
/// windup while keeping the action 1:1, say. Output runs `0..last_output` at the
/// same fps.
///
/// Contacts are resampled from the same source-frame float `f` the pose
/// interpolation uses, but nearest-frame: a boolean cannot be lerped, and the
/// honest substitute is whichever labelled frame the sample sits closest to.
/// Ties round half **up** — `f = 2.5` reads frame 3 — so a strike keeps the
/// same one-directional bias everywhere it is resampled instead of flickering
/// with float noise around the midpoint.
fn retime(take: &mut Take, mapping: &[(f32, f32)]) {
    let src: Vec<f32> = mapping.iter().map(|m| m.0).collect();
    let dst: Vec<f32> = mapping.iter().map(|m| m.1).collect();
    let frames = take.frames();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "frame counts are small and non-negative"
    )]
    let new_frames = (dst[dst.len() - 1] * take.fps).round() as usize + 1;

    let mut rotations = Vec::with_capacity(new_frames);
    let mut root = Vec::with_capacity(new_frames);
    let mut contacts = take
        .contacts
        .as_ref()
        .map(|_| Vec::with_capacity(new_frames));
    for i in 0..new_frames {
        let source_s = interp(i as f32 / take.fps, &dst, &src);
        let f = (source_s * take.fps).clamp(0.0, (frames - 1) as f32);
        let f0 = f.floor();
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped above"
        )]
        let a = f0 as usize;
        let b = (a + 1).min(frames - 1);
        let w = f - f0;

        root.push(take.root[a].lerp(take.root[b], w));
        let mut frame = [Quat::IDENTITY; skeleton::JOINT_COUNT];
        for (j, slot) in frame.iter_mut().enumerate() {
            *slot = nlerp(take.rotations[a][j], take.rotations[b][j], w);
        }
        rotations.push(frame);

        if let (Some(out), Some(source)) = (contacts.as_mut(), take.contacts.as_ref()) {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "f is clamped to [0, frames - 1], so f + 0.5 floors within range"
            )]
            let nearest = (f + 0.5).floor() as usize;
            out.push(source[nearest]);
        }
    }
    take.rotations = rotations;
    take.root = root;
    take.contacts = contacts;
}

/// Normalised lerp, taking the short way around.
///
/// Deliberately not `slerp`: the retired Python used a component lerp, the
/// frozen fixtures were baked with it, and on frames this close together the
/// two are indistinguishable — but only one of them matches.
fn nlerp(a: Quat, b: Quat, w: f32) -> Quat {
    let b = if a.dot(b) < 0.0 { -b } else { b };
    (a * (1.0 - w) + b * w).normalize()
}

/// Blend the tail back toward frame 0 so the clip loops cleanly.
///
/// Contacts are left untouched: the blend nudges poses, the frame count does
/// not change, and when a foot touched the ground is a fact about the source
/// motion, not about how its tail is eased.
fn wrap_blend(take: &mut Take, blend_s: f32) {
    let frames = take.frames();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "frame counts are small and non-negative"
    )]
    let k = ((blend_s * take.fps).round() as usize).min(frames / 2);
    if k < 1 {
        return;
    }
    let first = take.rotations[0];
    let first_y = take.root[0].y;
    for i in 0..k {
        let t = frames - k + i;
        let w = (i + 1) as f32 / (k + 1) as f32;
        for (slot, target) in take.rotations[t].iter_mut().zip(first) {
            *slot = nlerp(*slot, target, w);
        }
        take.root[t].y = (1.0 - w) * take.root[t].y + w * first_y;
    }
}

/// Turn the whole pose 180° about Y, from ARDY's +Z facing into the rig
/// contract's −Z: the root joint's orientation and the root travel rotate
/// together, so every child bone rides along and nothing character-relative
/// changes. The final stage of every [`Edit::apply`] — built takes are rig
/// space, full stop (see the module docs).
fn to_rig_space(take: &mut Take) {
    debug_assert_eq!(skeleton::JOINTS[0], "Hips", "the root joint moved");
    let yaw = Quat::from_rotation_y(std::f32::consts::PI);
    for frame in &mut take.rotations {
        frame[0] = (yaw * frame[0]).normalize();
    }
    for p in &mut take.root {
        p.x = -p.x;
        p.z = -p.z;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A take whose contact rows encode their own frame index in binary, so
    /// any resampled row identifies exactly which source frame it came from.
    fn indexed_take(frames: usize, fps: f32) -> Take {
        Take {
            rotations: vec![[Quat::IDENTITY; skeleton::JOINT_COUNT]; frames],
            root: vec![Vec3::ZERO; frames],
            contacts: Some((0..frames).map(index_row).collect()),
            fps,
            prompt: String::new(),
        }
    }

    fn index_row(i: usize) -> [bool; 4] {
        [i & 1 != 0, i & 2 != 0, i & 4 != 0, i & 8 != 0]
    }

    fn assert_close(actual: f32, expected: f32, context: &str) {
        assert!(
            (actual - expected).abs() < 1e-6,
            "{context}: got {actual}, expected {expected}"
        );
    }

    #[test]
    fn every_apply_lands_in_rig_space() {
        // A take travelling +Z (ARDY forward) with identity root rotation.
        let mut take = indexed_take(4, 20.0);
        for (i, p) in take.root.iter_mut().enumerate() {
            p.z = i as f32 * 0.1;
        }

        // The identity edit still turns the pose: travel comes out along −Z
        // and the root joint carries the 180° yaw.
        let out = Edit::default().apply(&take);
        assert_close(out.root[3].z, -0.3, "identity edit travel");
        let expected = Quat::from_rotation_y(std::f32::consts::PI);
        assert!(
            out.rotations[0][0].angle_between(expected) < 1e-5,
            "root joint yawed into rig space"
        );

        // The degenerate path (trim leaves < 2 frames) turns it too — rig
        // space is apply's postcondition, not a side effect of some knob.
        let out = Edit {
            trim_start: 10,
            ..Edit::default()
        }
        .apply(&take);
        assert_close(out.root[3].z, -0.3, "degenerate edit travel");
    }

    #[test]
    fn trim_slices_contacts_with_the_motion() {
        let take = indexed_take(5, 20.0);
        let edit = Edit {
            trim_start: 1,
            trim_end: 1,
            ..Edit::default()
        };
        let out = edit.apply(&take);
        assert_eq!(out.frames(), 3);
        assert_eq!(
            out.contacts,
            Some(vec![index_row(1), index_row(2), index_row(3)])
        );
    }

    #[test]
    fn retime_resamples_contacts_nearest_frame_rounding_half_up() {
        // fps 4 and a 2x stretch keep every source-frame float exact in f32,
        // so this pins the tie-break rather than floating past it: output
        // frame i samples source frame float i/2, and the halves must round
        // UP — [0, 1, 1, 2, 2, 3, 3, 4, 4], not [0, 0, 1, 1, 2, 2, 3, 3, 4].
        let take = indexed_take(5, 4.0);
        let edit = Edit {
            retime: vec![(0.0, 0.0), (1.0, 2.0)],
            ..Edit::default()
        };
        let out = edit.apply(&take);
        assert_eq!(out.frames(), 9);
        let expected: Vec<[bool; 4]> = [0, 1, 1, 2, 2, 3, 3, 4, 4]
            .into_iter()
            .map(index_row)
            .collect();
        assert_eq!(out.contacts, Some(expected));
    }

    #[test]
    fn absent_contacts_stay_absent_through_every_stage() {
        let mut take = indexed_take(10, 20.0);
        take.contacts = None;
        let edit = Edit {
            trim_start: 1,
            retime: vec![(0.0, 0.0), (0.4, 0.2)],
            in_place: InPlace::Strip,
            arm_bend_deg: 10.0,
            exaggerate: 1.2,
            loop_blend_s: 0.1,
            ..Edit::default()
        };
        assert_eq!(edit.apply(&take).contacts, None);
    }

    #[test]
    fn pose_and_root_stages_leave_contacts_byte_identical() {
        let take = indexed_take(8, 20.0);
        let edit = Edit {
            in_place: InPlace::Detrend,
            arm_bend_deg: 15.0,
            lean_deg: 5.0,
            shoulder_back_deg: 10.0,
            exaggerate: 1.5,
            loop_blend_s: 0.15,
            ..Edit::default()
        };
        assert_eq!(edit.apply(&take).contacts, take.contacts);
    }

    /// Hips a metre off the floor, rising 0.6 m over the clip, with a 0.25 m
    /// dip at frame 3 — a jump's net lift and a landing crouch in one take.
    /// The start height is deliberately not zero: that is what separates
    /// detrending against frame 0 from detrending against the origin.
    fn rising_take() -> Take {
        let mut take = indexed_take(7, 20.0);
        for (i, p) in take.root.iter_mut().enumerate() {
            p.y = 1.0 + 0.1 * i as f32;
        }
        take.root[3].y -= 0.25;
        take
    }

    #[test]
    fn y_strip_pins_height_to_frame_zero() {
        let take = rising_take();
        let edit = Edit {
            y_mode: YMode::Strip,
            ..Edit::default()
        };
        assert!(!edit.is_identity(), "a Y knob is not the identity edit");
        for (i, p) in edit.apply(&take).root.iter().enumerate() {
            assert_close(p.y, 1.0, &format!("frame {i} pinned"));
        }
    }

    #[test]
    fn y_detrend_removes_the_net_rise_and_keeps_the_dip() {
        let take = rising_take();
        let out = Edit {
            y_mode: YMode::Detrend,
            ..Edit::default()
        }
        .apply(&take);
        // The 0.1 m per frame drift is gone: every frame sits back at the
        // starting height, except the crouch, which keeps its full 0.25 m.
        let expected = [1.0, 1.0, 1.0, 0.75, 1.0, 1.0, 1.0];
        for (i, (p, want)) in out.root.iter().zip(expected).enumerate() {
            assert_close(p.y, want, &format!("frame {i} detrended"));
        }
    }

    #[test]
    fn y_off_and_the_default_edit_leave_height_byte_identical() {
        let take = rising_take();
        let heights: Vec<f32> = take.root.iter().map(|p| p.y).collect();
        for edit in [
            Edit::default(),
            Edit {
                y_mode: YMode::Off,
                ..Edit::default()
            },
            // The X/Z knob must not reach Y either, and neither must the
            // recentre it runs after.
            Edit {
                in_place: InPlace::Strip,
                ..Edit::default()
            },
        ] {
            let out = edit.apply(&take);
            let got: Vec<f32> = out.root.iter().map(|p| p.y).collect();
            assert_eq!(got, heights, "{edit:?} moved root Y");
        }
    }

    #[test]
    fn a_retime_collapsed_to_one_frame_never_makes_nan() {
        let take = rising_take();
        // 0.35 s of source crushed into 0.02 s of output: at 20 fps that
        // rounds to a single frame, where detrend's `/(frames - 1)` used to
        // poison every root position with NaN.
        let out = Edit {
            retime: vec![(0.0, 0.0), (0.35, 0.02)],
            in_place: InPlace::Detrend,
            y_mode: YMode::Detrend,
            ..Edit::default()
        }
        .apply(&take);
        assert!(!out.root.is_empty(), "the clip still has a frame");
        for (i, p) in out.root.iter().enumerate() {
            assert!(
                p.x.is_finite() && p.y.is_finite() && p.z.is_finite(),
                "frame {i} stayed finite: {p:?}"
            );
        }
    }

    #[test]
    fn map_time_is_identity_for_edits_that_do_not_touch_time() {
        let edits = [
            Edit::default(),
            Edit {
                in_place: InPlace::Strip,
                arm_bend_deg: 25.0,
                lean_deg: 12.0,
                shoulder_back_deg: 18.0,
                exaggerate: 1.6,
                loop_blend_s: 0.4,
                ..Edit::default()
            },
        ];
        for edit in &edits {
            for t in [0.0, 0.05, 0.5, 1.35, 4.0] {
                let mapped = edit.map_time(t, 20.0).expect("inside the kept window");
                assert_close(mapped, t, "identity map");
            }
        }
    }

    #[test]
    fn map_time_shifts_by_the_trimmed_lead_in_and_refuses_times_inside_it() {
        let edit = Edit {
            trim_start: 5,
            trim_end: 26,
            ..Edit::default()
        };
        assert_eq!(edit.map_time(0.2, 20.0), None, "inside the trimmed lead-in");
        let at_cut = edit.map_time(0.25, 20.0).expect("first kept frame");
        assert_close(at_cut, 0.0, "first kept frame maps to zero");
        let later = edit.map_time(1.0, 20.0).expect("kept");
        assert_close(later, 0.75, "shifted by trim_start / fps");
    }

    #[test]
    fn map_time_sends_retime_knot_sources_to_their_destinations() {
        let knots = vec![(0.0, 0.0), (1.2, 0.5), (2.4, 2.0)];
        let edit = Edit {
            retime: knots.clone(),
            ..Edit::default()
        };
        for (src, dst) in &knots {
            let mapped = edit.map_time(*src, 20.0).expect("kept");
            assert_close(mapped, *dst, "knot source maps to its destination");
        }
        // Between knots the map is linear; past the last knot it clamps,
        // matching np.interp and therefore matching what apply() resampled.
        let mid = edit.map_time(0.6, 20.0).expect("kept");
        assert_close(mid, 0.25, "linear between knots");
        let past = edit.map_time(3.0, 20.0).expect("kept");
        assert_close(past, 2.0, "clamped past the last knot");
    }

    #[test]
    fn map_time_composes_trim_then_retime() {
        let edit = Edit {
            trim_start: 4,
            retime: vec![(0.0, 0.0), (1.2, 0.5), (2.4, 2.0)],
            ..Edit::default()
        };
        // 0.2 s of lead-in is cut, so take-time 1.4 s is retime-source 1.2 s.
        let mapped = edit.map_time(1.4, 20.0).expect("kept");
        assert_close(mapped, 0.5, "trim shift feeds the retime map");
    }

    #[test]
    fn map_time_refuses_a_clockless_take() {
        assert_eq!(Edit::default().map_time(1.0, 0.0), None);
        assert_eq!(Edit::default().map_time(1.0, -20.0), None);
    }
}
