//! `forge gpu`: who holds the card, and whether the largest backend fits.
//!
//! One 24 GB card, and the generators do not share it: TRELLIS.2 at 1024³
//! wants ~22 GB, ARDY ~16, the ACE-Step server sits at ~8 until it is
//! stopped. The question before any generate is not "is the GPU there" but
//! "is enough of it free", and the honest threshold is the largest peak any
//! described backend declares (`vram_gb` in its `backend.toml`). Exit 1
//! when the free memory is under that, with the processes holding the card
//! named — by pid, by the command, and by the backend whose interpreter it
//! is when that can be told — so the fix (`forge gen music --stop-server`,
//! close the studio window, wait for the sweep) is the next line.
//!
//! Everything comes from `nvidia-smi`: the card's name and memory, and the
//! compute apps. No nvidia-smi is a refusal, not a pass.

use std::path::Path;
use std::process::Command;

use forge_library::Project;
use forge_library::backends::Backends;
use serde_json::json;

use crate::cli::GpuArgs;
use crate::outcome::{Failure, Outcome};

/// One card as `nvidia-smi` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Card {
    name: String,
    total_mb: u64,
    used_mb: u64,
}

/// One process on the card.
#[derive(Debug, Clone, PartialEq, Eq)]
struct App {
    pid: u64,
    name: String,
    used_mb: u64,
}

/// Print the lines (or the object) and exit 1 when the largest backend
/// would not fit.
pub(crate) fn run(project: &Project, args: &GpuArgs) -> Outcome {
    if args.free {
        free(project);
    }
    let binary = which("nvidia-smi").ok_or_else(|| {
        Failure::refused(
            "nvidia-smi is not on PATH — no NVIDIA driver, or it is installed somewhere \
             else; the generators need the card it reports",
        )
    })?;
    let cards = parse_cards(&query(
        &binary,
        "--query-gpu=name,memory.total,memory.used",
    )?);
    let Some(card) = cards.first() else {
        return Err(Failure::refused("nvidia-smi listed no GPU"));
    };
    let apps = parse_apps(
        &query(&binary, "--query-compute-apps=pid,process_name,used_memory").unwrap_or_default(),
    );

    let backends = Backends::discover(project);
    let largest = backends.largest();
    let need_mb = backends
        .largest_vram_gb()
        .map(|gb| (gb * 1024.0).round() as u64);
    let free_mb = card.total_mb.saturating_sub(card.used_mb);
    let fits = need_mb.is_none_or(|need| free_mb >= need);

    let labelled: Vec<(App, Option<String>)> = apps
        .iter()
        .map(|app| (app.clone(), holder(&backends, &app.name)))
        .collect();

    if args.json {
        let object = json!({
            "ok": fits,
            "name": card.name,
            "total_mb": card.total_mb,
            "used_mb": card.used_mb,
            "free_mb": free_mb,
            "largest": largest.map(|b| json!({ "backend": b.name, "vram_gb": b.vram_gb })),
            "need_mb": need_mb,
            "apps": labelled.iter().map(|(app, backend)| json!({
                "pid": app.pid,
                "name": app.name,
                "used_mb": app.used_mb,
                "backend": backend,
            })).collect::<Vec<_>>(),
        });
        println!("{object}");
    } else {
        println!(
            "gpu       {}  {} / {} MiB in use, {} MiB free",
            card.name, card.used_mb, card.total_mb, free_mb
        );
        if labelled.is_empty() {
            println!("holding   nobody");
        }
        for (app, backend) in &labelled {
            println!(
                "holding   pid {} {:.1} GB  {}{}",
                app.pid,
                app.used_mb as f64 / 1024.0,
                app.name,
                backend
                    .as_deref()
                    .map_or_else(String::new, |b| format!("  ({b})"))
            );
        }
        match (largest, need_mb) {
            (Some(backend), Some(need)) => println!(
                "largest   {} needs {} GB ({need} MiB): {}",
                backend.name,
                backend.vram_gb.unwrap_or(0.0),
                if fits {
                    "fits"
                } else {
                    "does NOT fit — stop what holds the card before a generate"
                }
            ),
            _ => println!("largest   no backend declares a VRAM peak; nothing to hold the card to"),
        }
    }
    if fits {
        Ok(())
    } else {
        let who: Vec<String> = labelled
            .iter()
            .map(|(app, backend)| match backend {
                Some(b) => format!(
                    "pid {} ({b}, {:.1} GB)",
                    app.pid,
                    app.used_mb as f64 / 1024.0
                ),
                None => format!("pid {} ({:.1} GB)", app.pid, app.used_mb as f64 / 1024.0),
            })
            .collect();
        Err(Failure::failed(format!(
            "gpu: {free_mb} MiB free of {} and {} needs {}; held by {}",
            card.total_mb,
            largest.map_or("the largest backend", |b| b.name.as_str()),
            need_mb.unwrap_or(0),
            if who.is_empty() {
                String::from("no compute app nvidia-smi can see (a display, or another container)")
            } else {
                who.join(", ")
            }
        )))
    }
}

/// `forge gpu --free`: ask the `ComfyUI` host to unload, and clear a
/// withheld lease once the card is back.
///
/// This is what replaced `forge gen music --stop-server`, which went with
/// the resident ACE-Step server. Two endpoints and no graph: `POST /free`
/// then `GET /system_stats`, the same pair the daemon's release ladder
/// uses, because the card must answer with no Python alive.
///
/// **"The card is back" is a claim about the card, not about this call.**
/// It used to compare free VRAM against a number read one line earlier, so
/// it printed "14.7 GB free before, 14.7 GB after … the card is back" with
/// its own next line naming pid 693788 holding 8.1 GB (2026-08-30). The
/// ladder now judges against the card's idle floor, and this door only
/// clears a withholding when that floor is met — a withheld lease is the
/// one safety net `designs/hosting.md` makes load-bearing for the MOSS
/// pack, and the command its own note tells the user to run must not clear
/// it on no evidence.
fn free(project: &Project) {
    let Some(url) = comfy_url(project) else {
        println!("free      no ComfyUI host is configured, so there is nothing to unload");
        return;
    };
    let before = forge_serve::comfy_free_gb(&url);
    let release =
        forge_serve::release_comfy(&url, None, before, |line| println!("free      {line}"));
    match (before, release.after_gb) {
        (Some(before), Some(after)) => {
            println!("free      {before:.1} GB free before, {after:.1} GB after");
        }
        (_, Some(after)) => println!("free      {after:.1} GB free"),
        _ => println!("free      {url} did not answer /system_stats"),
    }
    if let Some(floor) = release.floor_gb {
        println!("floor     {floor:.1} GB is what this card shows with nothing loaded");
    } else {
        println!(
            "floor     unknown — /system_stats did not say how big the card is, so this is the \
             weaker check: did this call give back what it took"
        );
    }
    let state = forge_serve::state_dir(&project.root);
    if release.returned {
        // A card that is provably back clears a withholding: this is the
        // one door that can say so, because it just measured it against the
        // floor.
        forge_serve::release_withhold(&state);
        println!("free      the card is back; any withheld lease is cleared");
    } else if let Some(note) = release.note {
        let _ = forge_serve::withhold(&state, &note);
        println!("free      {note}");
        println!("free      the withheld lease stays: nothing here proved the card is free");
    }
}

/// Where the `ComfyUI` host is: the environment first, then the host
/// backend's own `[server]` block.
fn comfy_url(project: &Project) -> Option<String> {
    if let Ok(url) = std::env::var("FORGE_COMFY_URL")
        && !url.trim().is_empty()
    {
        return Some(url);
    }
    let backends = Backends::discover(project);
    let comfy = backends.get("comfy")?;
    let text = std::fs::read_to_string(comfy.dir.join("backend.toml")).ok()?;
    let host = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("host = "))?
        .trim()
        .trim_matches('"')
        .to_owned();
    let port: u16 = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("port = "))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    Some(format!("http://{host}:{port}"))
}

/// `nvidia-smi <query> --format=csv,noheader,nounits`, its stdout.
fn query(binary: &Path, what: &str) -> Result<String, Failure> {
    let output = Command::new(binary)
        .arg(what)
        .arg("--format=csv,noheader,nounits")
        .output()
        .map_err(|e| Failure::refused(format!("nvidia-smi did not run: {e}")))?;
    if !output.status.success() {
        return Err(Failure::refused(format!(
            "nvidia-smi exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `name, total, used` per line.
fn parse_cards(text: &str) -> Vec<Card> {
    text.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split(',').map(str::trim).collect();
            if parts.len() < 3 {
                return None;
            }
            Some(Card {
                name: parts[0].to_owned(),
                total_mb: parts[1].parse::<f64>().ok()? as u64,
                used_mb: parts[2].parse::<f64>().ok()? as u64,
            })
        })
        .collect()
}

/// `pid, process_name, used_memory` per line. A process name can carry a
/// comma (a command line with arguments), so the split is from both ends.
fn parse_apps(text: &str) -> Vec<App> {
    text.lines()
        .filter_map(|line| {
            let (pid, rest) = line.split_once(',')?;
            let (name, used) = rest.rsplit_once(',')?;
            Some(App {
                pid: pid.trim().parse().ok()?,
                name: name.trim().to_owned(),
                used_mb: used.trim().parse::<f64>().ok()? as u64,
            })
        })
        .collect()
}

/// The backend whose interpreter a process is, when its command path sits
/// under that backend's env prefix.
///
/// The binary itself is not resolved: a uv venv's `bin/python` is a symlink
/// out to the managed interpreter, and following it would leave the venv.
/// Its directory is — `.env/bin` resolves to the real prefix's `bin` — so
/// the process is compared both as nvidia-smi spelled it and with its
/// directory resolved.
fn holder(backends: &Backends, process: &str) -> Option<String> {
    let spelled = Path::new(process.split_whitespace().next()?);
    let resolved = spelled
        .parent()
        .and_then(|dir| dir.canonicalize().ok())
        .map(|dir| dir.join(spelled.file_name().unwrap_or_default()));
    for backend in &backends.backends {
        let Some(interpreter) = &backend.interpreter else {
            continue;
        };
        for prefix in [
            interpreter.clone(),
            interpreter.canonicalize().unwrap_or_default(),
        ] {
            if prefix.as_os_str().is_empty() {
                continue;
            }
            let prefix = if prefix.is_file() {
                // An override naming the binary: its prefix is above bin/.
                prefix
                    .parent()
                    .and_then(Path::parent)
                    .map_or(prefix.clone(), Path::to_path_buf)
            } else {
                prefix
            };
            if spelled.starts_with(&prefix)
                || resolved.as_deref().is_some_and(|r| r.starts_with(&prefix))
            {
                return Some(backend.name.clone());
            }
        }
    }
    None
}

/// `which`, without the crate.
fn which(name: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_process_under_a_backend_env_is_named_even_through_a_uv_symlink() {
        let dir = tempfile::tempdir().expect("tempdir");
        let backends = dir.path().join("backends");
        let ardy = backends.join("ardy");
        std::fs::create_dir_all(&ardy).expect("mkdir");
        std::fs::write(
            ardy.join("backend.toml"),
            "name = \"ardy\"\nentry = \"motion\"\n",
        )
        .expect("toml");
        // A venv whose bin/python is a symlink out to a managed interpreter.
        let managed = dir.path().join("managed/bin");
        std::fs::create_dir_all(&managed).expect("mkdir");
        std::fs::write(managed.join("python3.12"), b"#!/bin/sh\n").expect("python");
        let venv = dir.path().join("venv");
        std::fs::create_dir_all(venv.join("bin")).expect("mkdir");
        std::os::unix::fs::symlink(managed.join("python3.12"), venv.join("bin/python"))
            .expect("link");
        std::os::unix::fs::symlink(&venv, ardy.join(".env")).expect("link");
        let mut project =
            forge_library::Project::init(&dir.path().join("game"), "game").expect("init");
        project.backends_dir = Some(backends);
        let found = Backends::discover(&project);
        assert!(found.is_found("ardy"), "{:?}", found.get("ardy"));
        let through_link = ardy.join(".env/bin/python");
        assert_eq!(
            holder(&found, through_link.to_str().expect("utf-8")).as_deref(),
            Some("ardy")
        );
        let direct = venv.join("bin/python");
        assert_eq!(
            holder(&found, direct.to_str().expect("utf-8")).as_deref(),
            Some("ardy")
        );
        assert_eq!(holder(&found, "/usr/bin/blender"), None);
    }

    #[test]
    fn nvidia_smi_lines_parse_and_a_comma_in_a_command_survives() {
        let cards = parse_cards("NVIDIA GeForce RTX 4090, 24564, 463\n");
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].total_mb, 24564);
        assert_eq!(cards[0].used_mb, 463);
        let apps = parse_apps("1234, /x/.venv/bin/python -m a,b, 8300\n77, blender, 210\n");
        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0].pid, 1234);
        assert_eq!(apps[0].name, "/x/.venv/bin/python -m a,b");
        assert_eq!(apps[0].used_mb, 8300);
        assert_eq!(apps[1].name, "blender");
        assert!(parse_apps("\n").is_empty());
    }
}
