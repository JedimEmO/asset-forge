//! `forge doctor`: what this machine can do, in one table.
//!
//! P1 scope: the project root, the rig profile and whether it has drifted
//! from its artifacts, what the library holds and whether the manifest is
//! current, and where the generator backends are (and which are there). The
//! in-environment Python probes — imports, torch, CUDA, weights, licence
//! notices — and the GPU line join in P2 with the Python layer; until then
//! the backends section says so rather than pretending a missing probe is a
//! healthy backend.
//!
//! The exit code follows the profile: a contract that does not load or no
//! longer derives from its `rig.glb` is a broken toolkit, not a missing
//! option. A missing backend is "generation is off", which is a normal state
//! for a project that only ships what an artist handed it.

use forge_library::backends::{BackendState, Backends};
use forge_library::{Catalog, Kind, Project, manifest};

use crate::outcome::{Failure, Outcome};

/// Print the table.
pub(crate) fn run(project: &Project) -> Outcome {
    println!(
        "project   {} at {} ({})",
        project.name,
        project.root.display(),
        forge_library::project::PROJECT_FILE
    );
    println!(
        "          assets {}  sources {}  out {}  library {}",
        rel(project, &project.assets),
        rel(project, &project.sources),
        rel(project, &project.out),
        project.library_version
    );

    let mut broken = Vec::new();
    match project.profile() {
        Ok(profile) => {
            let contract = &profile.contract;
            let mut status = Vec::new();
            match profile.check_sources() {
                Ok(sources) => {
                    if sources.glb_matches {
                        status.push(String::from("glb sha ok"));
                    } else {
                        status.push(String::from("glb sha MISMATCH"));
                        broken.push("rig.glb is not the file the contract was derived from");
                    }
                    match sources.blend_matches {
                        Some(true) => status.push(String::from("blend sha ok")),
                        Some(false) => status.push(String::from("blend sha differs (warning)")),
                        None => status.push(String::from("no blend")),
                    }
                }
                Err(err) => {
                    status.push(format!("sources unreadable: {err}"));
                    broken.push("the profile's rig.glb cannot be read");
                }
            }
            match std::fs::read(profile.glb_path())
                .map_err(|e| e.to_string())
                .and_then(|bytes| forge_rig::derive_from_glb(&bytes).map_err(|e| e.to_string()))
            {
                Ok(derived) => {
                    let drift = forge_rig::check_drift(contract, &derived.bones);
                    if drift.is_empty() {
                        status.push(String::from("no drift"));
                    } else {
                        status.push(format!("DRIFT in {} bone(s)", drift.len()));
                        for line in &drift {
                            println!("          drift: {line}");
                        }
                        broken.push("the contract no longer derives from rig.glb");
                    }
                }
                Err(err) => {
                    status.push(format!("cannot derive: {err}"));
                    broken.push("the profile's rig.glb does not derive");
                }
            }
            println!(
                "rig       {} v{}: {} bones ({} driven by {}), {} socket(s), {} — {}",
                contract.name,
                contract.version,
                contract.bones.len(),
                contract.driven().count(),
                contract.driven_layout,
                profile.sockets.sockets.len(),
                rel(project, &profile.dir),
                status.join(", ")
            );
        }
        Err(err) => {
            println!(
                "rig       {} at {}: does not load — {err}",
                project.rig_name,
                rel(project, &project.rig_dir())
            );
            broken.push("the rig profile does not load");
        }
    }

    let catalog = Catalog::scan(project);
    let counts: Vec<String> = Kind::ALL
        .iter()
        .map(|kind| {
            let n = catalog.records().iter().filter(|r| r.kind == *kind).count();
            format!("{n} {kind}")
        })
        .collect();
    let manifest_state = if project.manifest_path().is_file() {
        if manifest::check(project).ok() {
            "manifest current"
        } else {
            "manifest STALE — run `forge manifest`"
        }
    } else {
        "no manifest — run `forge manifest`"
    };
    println!("library   {}; {manifest_state}", counts.join(", "));

    let backends = Backends::discover(project);
    print!("{}", forge_library::backends::table(&backends));
    let found = backends
        .backends
        .iter()
        .filter(|b| b.state == BackendState::Found)
        .count();
    println!(
        "          {found} of {} found; the in-environment probes (imports, torch, CUDA, \
         weights, licence notices) and the GPU line land in P2 — a found backend is not yet \
         a checked one",
        backends.backends.len()
    );
    match (Backends::toolkit_root(project), crate::toolkit::root()) {
        (Some(root), _) => println!("toolkit   {} (python/forge_gen)", root.display()),
        (None, Some(checkout)) => println!(
            "toolkit   {} — the checkout, without python/forge_gen yet (lands in P2), so \
             generation is off",
            checkout.display()
        ),
        (None, None) => println!(
            "toolkit   not found from this executable — generation is off; set {} to the checkout",
            forge_library::backends::TOOLKIT_ENV
        ),
    }

    if broken.is_empty() {
        Ok(())
    } else {
        Err(Failure::failed(format!("doctor: {}", broken.join("; "))))
    }
}

/// A path as the table shows it: relative to the root when it is under it.
fn rel(project: &Project, path: &std::path::Path) -> String {
    project
        .rel_to_root(path)
        .filter(|r| !r.is_empty())
        .unwrap_or_else(|| path.display().to_string())
}
