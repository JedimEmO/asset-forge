//! One ARDY take, read straight from its `.npz`.
//!
//! ARDY's raw output is a zip of uncompressed `NumPy` arrays. Reading it directly
//! means a freshly generated take can be previewed on the character *instantly*
//! — no Blender, no glTF, no intermediate BVH — which is what makes an
//! interactive generate/reroll/edit loop feel immediate rather than batch.
//!
//! ```no_run
//! let take = forge_motion::Take::read("assets-src/takes/roll.npz")?;
//! println!("{} frames at {} fps: {}", take.frames(), take.fps, take.prompt);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::io::Read;
use std::path::Path;

use glam::{Mat3, Quat, Vec3};

use crate::{npy, skeleton};

/// One ARDY take: the motion, and what produced it.
#[derive(Debug, Clone)]
pub struct Take {
    /// Joint-local rotations, `[frame][joint]`, in ARDY's bone frames.
    pub rotations: Vec<[Quat; skeleton::JOINT_COUNT]>,
    /// Root (hips) translation per frame, metres, Y-up.
    pub root: Vec<Vec3>,
    /// Per-frame foot contact flags, `[frame][column]`, when the take
    /// carries them.
    ///
    /// Column order is `[LeftFoot, LeftToeBase, RightFoot, RightToeBase]`:
    /// ARDY writes the left side first, which is the *reverse* of
    /// [`skeleton::JOINTS`] (right leg before left). Verified against the
    /// Python motion review, whose contact order `[25, 26, 21, 22]` —
    /// [`skeleton::CONTACT_COLUMNS`], published in the rig profile's
    /// `motion_skeleton.json` — maps the columns to those joint indices with
    /// the note "npz column order is Left*, Right*".
    ///
    /// `None` means the take had no `foot_contacts.npy`. Contacts are ARDY's
    /// own labelling of its motion and are never fabricated from poses here —
    /// a reader that guessed them would poison every footstep event derived
    /// downstream.
    pub contacts: Option<Vec<[bool; 4]>>,
    /// Frames per second.
    pub fps: f32,
    /// The prompt that generated it, if the take carries one.
    pub prompt: String,
}

impl Take {
    /// Frame count.
    #[must_use]
    pub fn frames(&self) -> usize {
        self.rotations.len()
    }

    /// Length in seconds.
    #[must_use]
    pub fn duration(&self) -> f32 {
        if self.fps <= 0.0 {
            0.0
        } else {
            self.frames() as f32 / self.fps
        }
    }

    /// Read a take from a `.npz` on disk.
    ///
    /// # Errors
    ///
    /// Returns [`TakeError`] if the file cannot be opened, a required array is
    /// missing, or an array has an unexpected dtype or shape.
    pub fn read(path: impl AsRef<Path>) -> Result<Self, TakeError> {
        let file = std::fs::File::open(path.as_ref()).map_err(|e| TakeError::Io(e.to_string()))?;
        let mut zip = zip::ZipArchive::new(file).map_err(|e| TakeError::NotAnNpz(e.to_string()))?;

        let mut member = |name: &str| -> Result<Vec<u8>, TakeError> {
            let mut f = zip
                .by_name(name)
                .map_err(|_| TakeError::MissingArray(name.to_owned()))?;
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)
                .map_err(|e| TakeError::Io(e.to_string()))?;
            Ok(buf)
        };

        let rot_bytes = member("local_rot_mats.npy")?;
        let hips_bytes = member("root_positions.npy")?;
        let fps_raw = member("fps.npy").ok();
        let text_raw = member("text.npy").ok();

        let rot = npy::read_f32(&rot_bytes).map_err(TakeError::Npy)?;
        let hips = npy::read_f32(&hips_bytes).map_err(TakeError::Npy)?;

        // [T, 27, 3, 3]
        if rot.shape.len() != 4
            || rot.shape[1] != skeleton::JOINT_COUNT
            || rot.shape[2] != 3
            || rot.shape[3] != 3
        {
            return Err(TakeError::UnexpectedShape {
                array: "local_rot_mats",
                shape: rot.shape.clone(),
            });
        }
        let frames = rot.shape[0];
        if hips.shape.len() != 2 || hips.shape[0] != frames || hips.shape[1] != 3 {
            return Err(TakeError::UnexpectedShape {
                array: "root_positions",
                shape: hips.shape.clone(),
            });
        }

        let mut rotations = Vec::with_capacity(frames);
        for t in 0..frames {
            let mut frame = [Quat::IDENTITY; skeleton::JOINT_COUNT];
            for (j, slot) in frame.iter_mut().enumerate() {
                let base = (t * skeleton::JOINT_COUNT + j) * 9;
                let m = &rot.data[base..base + 9];
                // numpy is row-major; glam's from_cols_array wants columns.
                *slot = Quat::from_mat3(&Mat3::from_cols_array(&[
                    m[0], m[3], m[6], m[1], m[4], m[7], m[2], m[5], m[8],
                ]))
                .normalize();
            }
            rotations.push(frame);
        }

        let root_track = (0..frames)
            .map(|t| Vec3::new(hips.data[t * 3], hips.data[t * 3 + 1], hips.data[t * 3 + 2]))
            .collect();

        // Optional: older takes were generated before contacts were kept. A
        // missing member is honestly None; a *present* member that will not
        // parse or whose shape disagrees with the motion is a corrupt take
        // and refuses loudly, because quietly dropping it would look
        // identical to "this take never had contacts".
        let contacts = match member("foot_contacts.npy") {
            Err(TakeError::MissingArray(_)) => None,
            Err(other) => return Err(other),
            Ok(bytes) => {
                let flags = npy::read_bool(&bytes).map_err(TakeError::Npy)?;
                if flags.shape.len() != 2 || flags.shape[0] != frames || flags.shape[1] != 4 {
                    return Err(TakeError::UnexpectedShape {
                        array: "foot_contacts",
                        shape: flags.shape,
                    });
                }
                Some(
                    (0..frames)
                        .map(|t| {
                            [
                                flags.data[t * 4],
                                flags.data[t * 4 + 1],
                                flags.data[t * 4 + 2],
                                flags.data[t * 4 + 3],
                            ]
                        })
                        .collect(),
                )
            }
        };

        let fps = fps_raw
            .and_then(|b| npy::read_scalar_int(&b).ok())
            .map_or(20.0, |v| v as f32);
        let prompt = text_raw
            .and_then(|b| npy::read_scalar_str(&b).ok())
            .unwrap_or_default();

        Ok(Self {
            rotations,
            root: root_track,
            contacts,
            fps,
            prompt,
        })
    }
}

/// Why a take could not be read.
#[derive(Debug, Clone)]
pub enum TakeError {
    /// The file could not be opened or read.
    Io(String),
    /// The file is not a readable zip.
    NotAnNpz(String),
    /// A required array is absent.
    MissingArray(String),
    /// An array's dtype or header was not readable.
    Npy(npy::NpyError),
    /// An array was present but not the shape cskel27 requires.
    UnexpectedShape {
        /// Which array.
        array: &'static str,
        /// The shape found.
        shape: Vec<usize>,
    },
}

impl std::fmt::Display for TakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(msg) => write!(f, "could not read the take: {msg}"),
            Self::NotAnNpz(msg) => write!(f, "not a readable .npz: {msg}"),
            Self::MissingArray(name) => write!(f, "the take has no {name}"),
            Self::Npy(err) => write!(f, "{err}"),
            Self::UnexpectedShape { array, shape } => write!(
                f,
                "{array} has shape {shape:?}, which is not a {}-joint cskel27 take",
                skeleton::JOINT_COUNT
            ),
        }
    }
}

impl std::error::Error for TakeError {}
