//! The bundle door: one body, any number of clips, one `.glb` to hand out.
//!
//! Everything a consumer outside this toolkit needs in a single
//! self-contained file — the skin, and every clip as a named animation. The
//! merge itself is [`forge_motion::bundle`]; what lives here is the part that
//! needs a project: turning a library name into a file (and refusing with the
//! list of what does exist), writing the output, and writing the record
//! beside it.
//!
//! A bundle is a **derived artefact**, like a baked clip. It is a pure
//! function of the body, the clips, their order and the motion scale, and the
//! fix for anything wrong with one is upstream of it — never a hand edit of
//! the `.glb` or of the record. That is what the record's hashes are for: it
//! says which files went in, so anyone can run the same door again and get
//! the same bytes.
//!
//! Nothing here writes to `assets/`. A bundle is an export, not a library
//! asset: it has no sidecar, no manifest row and no name in the catalog,
//! because the library already holds every input it was made from.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::schema::{Actor, Kind};
use crate::{Catalog, LibraryError, Project, Result, clock, hash, read_bytes, write_atomic};

/// The record schema this build writes and reads.
pub const BUNDLE_SCHEMA: u64 = 1;

/// What to bundle, and for whom.
#[derive(Debug, Clone)]
pub struct BundleRequest {
    /// The body: a library body name, or a path to any rigged `.glb`.
    pub body: String,
    /// The clips, in the order they should appear: library clip names, or
    /// paths to clip `.glb`s.
    pub clips: Vec<String>,
    /// Where to write the bundle.
    pub out: PathBuf,
    /// What to multiply the root travel by — the body's leg ratio against
    /// the profile's reference legs. 1.0 leaves every byte alone.
    pub motion_scale: f64,
    /// Who asked.
    pub created_by: Actor,
}

/// One input file, as the record names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleFile {
    /// Project-relative when the file is under the project, absolute
    /// otherwise.
    pub path: String,
    /// `sha256:…` of it, as it was read.
    pub sha256: String,
}

/// One clip that went in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleClip {
    /// What it was asked for as: the library name, or the file's stem.
    pub name: String,
    /// Where it was read from.
    pub path: String,
    /// `sha256:…` of it.
    pub sha256: String,
}

/// What came out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleOutput {
    /// Where it was written.
    pub path: String,
    /// `sha256:…` of the bytes on disk.
    pub sha256: String,
    /// How big it is.
    pub bytes: u64,
    /// The animation names it carries, in clip order — what an engine binds
    /// by, which is the clip's name *inside* its own `.glb` and so need not
    /// be the library name it was asked for.
    pub animations: Vec<String>,
}

/// The account of one bundle: `<stem>.bundle.json` beside the `.glb`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BundleRecord {
    /// Always [`BUNDLE_SCHEMA`].
    pub forge_bundle: u64,
    /// The day it was made, `YYYY-MM-DD`.
    pub created: String,
    /// `human`, `agent:<name>` or `unknown`.
    pub created_by: String,
    /// The body it was merged into.
    pub body: BundleFile,
    /// The clips, in bundle order.
    pub clips: Vec<BundleClip>,
    /// The scale applied to the root travel.
    pub motion_scale: f64,
    /// The file that was written.
    pub output: BundleOutput,
}

impl BundleRecord {
    /// The record's bytes: pretty at two-space indent with a trailing
    /// newline, the same shape every other record in this repository has.
    ///
    /// # Errors
    ///
    /// Only if the record cannot be serialized, which the types make
    /// impossible in practice.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut bytes = serde_json::to_vec_pretty(self)
            .map_err(|e| LibraryError::json(Path::new("<bundle record>"), e))?;
        bytes.push(b'\n');
        Ok(bytes)
    }
}

/// A finished bundle: the record, and where the two files landed.
#[derive(Debug, Clone)]
pub struct Bundled {
    /// The record that was written.
    pub record: BundleRecord,
    /// The `.glb`.
    pub output: PathBuf,
    /// The `<stem>.bundle.json` beside it.
    pub record_path: PathBuf,
}

/// Merge a body and its clips into one `.glb`, and write the record beside it.
///
/// # Errors
///
/// The body or a clip resolves to nothing (the refusal lists what the library
/// holds of that kind); the scale is not a positive finite number; no clips
/// were named; the merge refuses (a clip driving a bone the body lacks, a
/// file that is not self-contained); or a file cannot be read or written.
pub fn write(project: &Project, request: &BundleRequest) -> Result<Bundled> {
    if !request.motion_scale.is_finite() || request.motion_scale <= 0.0 {
        return Err(LibraryError::rejected(format!(
            "motion scale {} is not a positive number; 1.0 leaves the root travel alone",
            request.motion_scale
        )));
    }
    if request.clips.is_empty() {
        return Err(LibraryError::rejected(
            "a bundle needs at least one clip; without one it is the body's own .glb",
        ));
    }

    let catalog = Catalog::scan(project);
    let body = resolve(project, &catalog, &request.body, Kind::Body)?;
    let clips = request
        .clips
        .iter()
        .map(|wanted| resolve(project, &catalog, wanted, Kind::Clip))
        .collect::<Result<Vec<_>>>()?;

    let body_bytes = read_bytes(&body.path)?;
    let clip_bytes = clips
        .iter()
        .map(|clip| read_bytes(&clip.path))
        .collect::<Result<Vec<_>>>()?;
    let sources: Vec<forge_motion::ClipSource<'_>> = clips
        .iter()
        .zip(&clip_bytes)
        .map(|(clip, bytes)| forge_motion::ClipSource {
            name: &clip.name,
            bytes,
        })
        .collect();

    // The merge is deliberately f32: a clip's values are f32 and the scale
    // multiplies them there. The record keeps what the caller typed.
    #[allow(clippy::cast_possible_truncation)]
    let bundled = forge_motion::bundle::bundle(&body_bytes, &sources, request.motion_scale as f32)
        .map_err(|e| LibraryError::bake(e.to_string()))?;

    let out = absolute(&request.out);
    write_atomic(&out, &bundled.bytes)?;
    let record = BundleRecord {
        forge_bundle: BUNDLE_SCHEMA,
        created: clock::today_iso(),
        created_by: request.created_by.to_string(),
        body: BundleFile {
            path: named(project, &body.path),
            sha256: hash::sha256_bytes(&body_bytes),
        },
        clips: clips
            .iter()
            .zip(&clip_bytes)
            .map(|(clip, bytes)| BundleClip {
                name: clip.name.clone(),
                path: named(project, &clip.path),
                sha256: hash::sha256_bytes(bytes),
            })
            .collect(),
        motion_scale: request.motion_scale,
        output: BundleOutput {
            path: named(project, &out),
            sha256: hash::sha256_bytes(&bundled.bytes),
            bytes: bundled.bytes.len() as u64,
            animations: bundled.animations,
        },
    };
    let record_path = record_path(&out);
    write_atomic(&record_path, &record.to_bytes()?)?;
    Ok(Bundled {
        record,
        output: out,
        record_path,
    })
}

/// `<stem>.bundle.json` beside a bundle.
#[must_use]
pub fn record_path(bundle: &Path) -> PathBuf {
    let stem = bundle.file_stem().map_or_else(
        || String::from("bundle"),
        |s| s.to_string_lossy().into_owned(),
    );
    bundle.with_file_name(format!("{stem}.bundle.json"))
}

/// One resolved input: what to call it, and where it is.
#[derive(Debug, Clone)]
struct Input {
    name: String,
    path: PathBuf,
}

/// Resolve a name the caller typed: an existing file first — as typed, then
/// against the project root, because the paths this toolkit prints are
/// project-relative and the shell it is called from need not be there — then
/// the library, then a refusal naming everything of that kind.
///
/// The file comes first for the same reason it does everywhere else here: a
/// caller who typed a path meant that file, even when a library asset shares
/// its stem.
fn resolve(project: &Project, catalog: &Catalog, wanted: &str, kind: Kind) -> Result<Input> {
    let wanted = wanted.trim();
    let named = |path: PathBuf| Input {
        name: path
            .file_stem()
            .map_or_else(|| wanted.to_owned(), |s| s.to_string_lossy().into_owned()),
        path,
    };
    if wanted.is_empty() {
        return Err(LibraryError::rejected(catalog.refusal(wanted, Some(kind))));
    }
    let direct = Path::new(wanted);
    if direct.is_file() {
        return Ok(named(absolute(direct)));
    }
    let from_project = project.root.join(direct);
    if from_project.is_file() {
        return Ok(named(from_project));
    }
    if let Some(record) = catalog.resolve(wanted, Some(kind)) {
        return Ok(Input {
            name: record.name.clone(),
            path: record.path.clone(),
        });
    }
    Err(LibraryError::rejected(catalog.refusal(wanted, Some(kind))))
}

/// A path as the record names it: project-relative under the project,
/// absolute otherwise — a record that said `../../tmp/x.glb` would be
/// relative to wherever its reader stood.
fn named(project: &Project, path: &Path) -> String {
    project
        .rel_to_root(path)
        .unwrap_or_else(|| path.display().to_string())
}

/// An absolute path, falling back to the one given when the working
/// directory cannot be read.
fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_project;

    /// The toolkit's own shipped files: the one body and two clips a test can
    /// bundle without a generator.
    fn repo(relative: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(relative)
    }

    #[test]
    fn a_bundle_writes_its_glb_and_a_record_naming_every_input() {
        let (dir, project) = temp_project();
        let out = dir.path().join("out/bundles/hand_off.glb");
        let bundled = write(
            &project,
            &BundleRequest {
                body: repo("assets/bodies/vex_runner.glb").display().to_string(),
                clips: vec![
                    repo("assets/clips/walk.glb").display().to_string(),
                    repo("assets/clips/roll.glb").display().to_string(),
                ],
                out: out.clone(),
                motion_scale: 1.0,
                created_by: Actor::parse("agent:test"),
            },
        )
        .expect("the bundle");

        assert!(out.is_file(), "the .glb");
        assert_eq!(
            bundled.record_path,
            dir.path().join("out/bundles/hand_off.bundle.json")
        );
        let record = bundled.record;
        assert_eq!(record.forge_bundle, BUNDLE_SCHEMA);
        assert_eq!(record.created_by, "agent:test");
        assert_eq!(record.clips.len(), 2);
        assert_eq!(record.clips[0].name, "walk");
        assert_eq!(record.output.animations.len(), 2);
        assert_eq!(
            record.output.sha256,
            hash::sha256_file(&out).expect("hash"),
            "the record's hash is the file's"
        );
        assert_eq!(
            record.output.bytes,
            std::fs::metadata(&out).expect("stat").len()
        );
        // Inputs outside the project are named absolutely; the output is
        // under it and so is named relative to it.
        assert_eq!(record.output.path, "out/bundles/hand_off.glb");
        assert!(record.body.path.starts_with('/'), "{}", record.body.path);

        // The record is JSON somebody else can read back.
        let written: BundleRecord =
            serde_json::from_slice(&std::fs::read(&bundled.record_path).expect("read"))
                .expect("the record parses");
        assert_eq!(written, record);
    }

    #[test]
    fn an_unknown_clip_is_refused_with_what_the_library_holds() {
        let (dir, project) = temp_project();
        let refusal = write(
            &project,
            &BundleRequest {
                body: repo("assets/bodies/vex_runner.glb").display().to_string(),
                clips: vec![String::from("moonwalk")],
                out: dir.path().join("out/bundles/x.glb"),
                motion_scale: 1.0,
                created_by: Actor::Unknown,
            },
        )
        .expect_err("no such clip")
        .to_string();
        assert!(refusal.contains("moonwalk"), "{refusal}");
        assert!(refusal.contains("clip"), "{refusal}");
    }

    #[test]
    fn a_scale_that_is_not_a_positive_number_is_refused() {
        let (dir, project) = temp_project();
        for scale in [0.0, -1.0, f64::NAN] {
            let refusal = write(
                &project,
                &BundleRequest {
                    body: repo("assets/bodies/vex_runner.glb").display().to_string(),
                    clips: vec![repo("assets/clips/walk.glb").display().to_string()],
                    out: dir.path().join("out/bundles/x.glb"),
                    motion_scale: scale,
                    created_by: Actor::Unknown,
                },
            )
            .expect_err("a scale that cannot multiply a track")
            .to_string();
            assert!(refusal.contains("motion scale"), "{refusal}");
        }
    }
}
