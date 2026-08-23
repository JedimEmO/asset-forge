"""``forge-gen motion review``: objective metrics + a visual contact sheet for generated character motion.

This is the standard acceptance gate for every ARDY take before it becomes a
game asset. Two outputs, meant to be used together:

  1. **Metrics** — foot skate, jitter, ground contact, speed profile, loop gap,
     heading drift, activity. Printed as a table (and written as JSON with
     ``--metrics``), with a FLAGS column naming every threshold the clip
     violates.
  2. **Contact sheet** — a PNG (``--sheet``) of evenly spaced keyframes drawn
     as a stick figure, which an agent (or human) actually looks at. Metrics
     catch mechanical defects; only the sheet catches "that isn't a sword
     swing".

    # compare a sweep: one row per candidate, ranked metric table
    forge-gen motion review out/sweeps/walk/*.npz --sheet out/sweeps/walk/sheet.png --metrics out/sweeps/walk/metrics.json

    # inspect one take closely: 3 camera rows + onion-skin strip + plots
    forge-gen motion review out/sweeps/walk/walk__d4_c2_s0_3.npz --detail --sheet roll.png

Reads ARDY ``.npz`` takes. No GPU: this reads motion data, it does not
generate it. It runs under the ardy env for numpy and matplotlib; when that
env is absent and the interpreter running the outer can import numpy, it
runs in-process instead (a sheet then needs matplotlib there too).

The joint tables (which index is the head, the hands, the feet, the
foot-contact column order, which side is which) come from the rig
profile's ``motion_skeleton.json`` and the thresholds from ``profile.toml``
``[review]``; nothing here is a constant.

THE JSON SHAPE IS A CONTRACT. A Rust port of these metrics must keep it::

    {
      "<take name>": {
        "frames": int, "fps": float, "duration_s": float,
        "path_len_m": float, "net_travel_m": float,
        "avg_speed_mps": float, "peak_speed_mps": float,
        "start_ratio": float, "stop_ratio": float, "moving_frac": float,
        "foot_skate_mps": float (null when no foot is ever planted),
        "contact_frac": float, "penetration_m": float, "lowest_foot_m": float,
        "jitter_mm": float, "jitter_pct": float, "activity_m": float,
        "head_min_m": float, "head_max_m": float, "head_end_m": float,
        "hand_top_m": float, "hand_reach_m": float, "lean_max_deg": float,
        "loop_gap_m": float, "loop_vel_gap": float,
        "drift_deg": float, "net_turn_deg": float,
        "travel_dir_deg": float (null when not travelling; 0 fwd, +90 right, 180 back, -90 left),
        "turn_deg": float, "left_steps": int, "action_beats": int,
        "speed_curve": [8 floats],
        "flags": ["SKATE"|"JITTER"|"SINKS"|"FLOATS"|"STATIC"|"SEAM"|"SEAMVEL"|"DRIFT"|"TURNS"|"STOPS", ...],
        "suggest": {
          "oneshot": {"trim_start_s": float, "trim_end_s": float, "kept_s": float},
          "loop": {"trim_start_s": float, "trim_end_s": float, "kept_s": float,
                   "seam_gap_m": float, "mean_speed_mps": float, "speed_cv": float}
        },
        "prompt": str,
        "path": str
      }, ...
    }

Key order as listed; numbers rounded as the code rounds them (the printed
table shows the same rounding, and prints ``nan`` where the JSON says
``null`` — strict JSON has no NaN, and ``null`` is what a reader may rely
on). ``--intent oneshot`` drops the loop-only
flags (SEAM, SEAMVEL, STOPS, DRIFT) from ``flags`` and nothing else. The
same object is returned inline as ``metrics`` in the ``--json`` result line.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import sys
from pathlib import Path

from forge_gen import profile as profile_mod
from forge_gen.exit_codes import InputRejected, UsageError

#: The metric columns the table prints, in order.
COLS = [
    "name", "duration_s", "avg_speed_mps", "start_ratio", "stop_ratio",
    "foot_skate_mps", "jitter_pct", "activity_m", "loop_gap_m", "drift_deg",
    "net_turn_deg", "left_steps", "flags",
]

#: Every threshold ``[review]`` must state, as the old TH table named them.
THRESHOLDS = (
    "foot_skate_mps", "jitter_pct", "penetration_m", "float_m", "activity_m",
    "static_speed_mps", "loop_gap_m", "loop_vel_gap", "drift_deg", "net_turn_deg", "stop_ratio",
)

# Flags that only matter for a clip meant to repeat. A death or a sword strike
# is *supposed* to end somewhere else, standing still, facing a new direction.
#
# TURNS is deliberately *not* here. `--in-place` strips root translation but not
# root rotation, so a oneshot whose heading does not come back leaves the
# character permanently swivelled the moment the clip ends — an attack that
# ended 45 deg off nearly shipped that way. Deaths and knockdowns pass it
# comfortably: every clip in the library lands within 20 deg of where it began.
LOOP_ONLY_FLAGS = {"SEAM", "SEAMVEL", "STOPS", "DRIFT"}

INTENTS = ("loop", "oneshot")
WINDOWS = ("action", "loop", "full")


# ---------------------------------------------------------------- skeleton --


class Tables:
    """The joint tables of one profile, in the names the metrics use (cskel27 for the humanoid profile)."""

    def __init__(self, prof) -> None:
        ms = prof.motion_skeleton
        self.joint_names: list[str] = list(ms["joints"])
        self.parents: list[int] = [-1 if p is None else int(p) for p in ms["parents"]]
        index = {name: i for i, name in enumerate(self.joint_names)}
        self.head: int = int(ms.get("head", index["Head"]))
        self.hands: list[int] = [int(i) for i in ms.get("hands", [index["RightHand"], index["LeftHand"]])]
        self.foot_joints: list[int] = [int(i) for i in ms.get("feet", [index[n] for n in ("RightFoot", "RightToeBase", "LeftFoot", "LeftToeBase")])]
        # order of the npz foot_contacts columns (Left*, Right*)
        self.contact_order: list[int] = [int(i) for i in ms.get("contact_columns", [index[n] for n in ("LeftFoot", "LeftToeBase", "RightFoot", "RightToeBase")])]
        self.right_joints: set[int] = {int(i) for i in ms.get("right", [])}
        self.left_joints: set[int] = {int(i) for i in ms.get("left", [])}
        self.left_foot: int = index["LeftFoot"]
        self.right_foot: int = index["RightFoot"]
        self.left_up_leg: int = index["LeftUpLeg"]
        self.right_up_leg: int = index["RightUpLeg"]
        self.count = len(self.joint_names)


def thresholds_from(prof) -> dict:
    """The ``[review]`` table; refuses a profile that leaves one out rather than inventing it."""
    section = prof.section("review")
    missing = [key for key in THRESHOLDS if key not in section]
    if missing:
        raise profile_mod.ProfileError(f"{prof.dir}/profile.toml [review] lacks {', '.join(missing)}")
    return {key: float(section[key]) for key in THRESHOLDS}


# --------------------------------------------------------------------- io --


def load_motion(path: str, tables: Tables) -> dict:
    """Return {joints[T,J,3], root[T,3], contacts[T,4]|None, fps, text, name, path}."""
    import numpy as np

    try:
        d = np.load(path, allow_pickle=True)
    except (OSError, ValueError) as err:
        raise InputRejected(f"{path} does not read as an .npz: {err}") from err
    if "root_positions" not in d.files:
        raise InputRejected(f"{path} carries no root_positions — not an ARDY take")
    if "posed_joints" in d.files:
        joints = np.asarray(d["posed_joints"], dtype=np.float64)
    elif "local_rot_mats" in d.files:
        # A take without posed joints: chain them from the local rotations
        # (needs ardy's skeleton; session imports it lazily).
        from forge_gen.motion import session

        joints = np.asarray(session.posed_joints(d["local_rot_mats"], d["root_positions"]), dtype=np.float64)
    else:
        raise InputRejected(f"{path} carries neither posed_joints nor local_rot_mats")
    root = np.asarray(d["root_positions"], dtype=np.float64)
    contacts = np.asarray(d["foot_contacts"]) if "foot_contacts" in d.files else None
    fps = float(d["fps"]) if "fps" in d.files else 20.0
    text = str(d["text"]) if "text" in d.files else ""
    if joints.ndim == 4:  # stray batch dim
        joints, root = joints[0], root[0]
        if contacts is not None and contacts.ndim == 3:
            contacts = contacts[0]
    if joints.ndim != 3 or joints.shape[1] != tables.count:
        raise InputRejected(f"{path}: posed_joints is {joints.shape}, expected (T, {tables.count}, 3) for this profile")
    return {
        "joints": joints, "root": root, "contacts": contacts, "fps": fps,
        "text": text, "name": os.path.splitext(os.path.basename(path))[0],
        "path": path,
    }


# ---------------------------------------------------------------------- metrics


def _smooth(a, k: int = 3):
    """Moving average along axis 0 with edge padding."""
    import numpy as np

    if a.shape[0] < k:
        return a.copy()
    pad = k // 2
    padded = np.concatenate([np.repeat(a[:1], pad, 0), a, np.repeat(a[-1:], pad, 0)])
    ker = np.ones(k) / k
    out = np.empty_like(a)
    flat_in = padded.reshape(padded.shape[0], -1)
    flat_out = out.reshape(out.shape[0], -1)
    for c in range(flat_in.shape[1]):
        flat_out[:, c] = np.convolve(flat_in[:, c], ker, mode="valid")
    return out


def analyze(mo: dict, tables: Tables, th: dict, intent: str = "loop") -> dict:
    import numpy as np

    joints, root, fps = mo["joints"], mo["root"], mo["fps"]
    T = joints.shape[0]
    dt = 1.0 / fps
    dur = (T - 1) * dt
    rel = joints - joints[:, :1, :]  # root-relative poses

    # --- root travel & speed profile ------------------------------------
    horiz = np.diff(root[:, [0, 2]], axis=0)
    step = np.linalg.norm(horiz, axis=1)
    speed = _smooth(step[:, None] / dt, 3)[:, 0] if T > 1 else np.zeros(1)
    peak = float(speed.max()) if speed.size else 0.0
    moving_frac = float((speed > 0.5 * peak).mean()) if peak > 0.3 else 0.0
    tail = speed[max(1, int(0.8 * speed.size)):]
    head_seg = speed[: max(1, int(0.2 * speed.size))]
    path_len = float(step.sum())
    net = float(np.linalg.norm(root[-1, [0, 2]] - root[0, [0, 2]]))

    # --- foot skate ------------------------------------------------------
    fj = joints[:, tables.foot_joints, :]
    foot_speed = np.linalg.norm(np.diff(fj[:, :, [0, 2]], axis=0), axis=2) / dt  # [T-1,4]
    if mo["contacts"] is not None and mo["contacts"].shape[0] == T:
        # npz column order is Left*, Right*; reorder to match foot_joints
        remap = [tables.contact_order.index(j) for j in tables.foot_joints]
        planted = np.asarray(mo["contacts"])[:, remap].astype(bool)[:-1]
    else:
        planted = fj[:-1, :, 1] < 0.06
    skate = float(foot_speed[planted].mean()) if planted.any() else float("nan")
    contact_frac = float(planted.any(axis=1).mean())

    # --- ground -----------------------------------------------------------
    low = fj[:, :, 1].min(axis=1)
    penetration = float(max(0.0, -low.min()))
    float_h = float(low.min())

    # --- jitter: energy a 3-tap smoother cannot explain ---------------------
    # Absolute (mm) reads high for any genuinely fast motion at 20 fps, so the
    # flag uses the ratio to real per-frame travel: that is dimensionless and
    # rises only with frequency, which is what jitter actually is.
    resid = rel - _smooth(rel, 3)
    jitter = float(np.sqrt((resid ** 2).sum(-1).mean())) * 1000.0
    disp = float(np.sqrt((np.diff(rel, axis=0) ** 2).sum(-1).mean())) if T > 1 else 0.0
    jitter_pct = 100.0 * jitter / 1000.0 / disp if disp > 1e-6 else 0.0

    # --- how much is happening --------------------------------------------
    activity = float(rel.std(axis=0).mean())
    head_y = joints[:, tables.head, 1]

    # --- gesture reach: is the strike/aim/punch actually big? ---------------
    hands = joints[:, tables.hands, :]
    hand_top = float(hands[:, :, 1].max())
    hand_reach = float(np.linalg.norm(hands[:, :, [0, 2]] - joints[:, :1, [0, 2]], axis=-1).max())
    spine = joints[:, tables.head, :] - joints[:, 0, :]
    pitch = np.degrees(np.arctan2(np.linalg.norm(spine[:, [0, 2]], axis=1), np.maximum(spine[:, 1], 1e-6)))

    # --- loop seam ---------------------------------------------------------
    loop_gap = float(np.linalg.norm(rel[-1] - rel[0], axis=-1).mean())
    if T > 2:
        v_end = (rel[-1] - rel[-2]) / dt
        v_start = (rel[1] - rel[0]) / dt
        loop_vel = float(np.linalg.norm(v_end - v_start, axis=-1).mean())
    else:
        loop_vel = 0.0

    # --- heading ------------------------------------------------------------
    # LeftUpLeg - RightUpLeg is the character's lateral (left-pointing) axis;
    # forward sits 90 deg from it. Measuring travel against that says whether a
    # "walks backwards" / "side steps right" prompt was actually obeyed.
    lat = joints[:, tables.left_up_leg, :] - joints[:, tables.right_up_leg, :]
    ang = np.degrees(np.unwrap(np.arctan2(lat[:, 2], lat[:, 0])))
    drift = float(ang[-1] - ang[0])
    turn = float(np.abs(np.diff(ang)).sum())
    travel_vec = root[-1, [0, 2]] - root[0, [0, 2]]
    if np.linalg.norm(travel_vec) > 0.3:
        lm = lat[:, [0, 2]].mean(axis=0)
        lm = lm / np.linalg.norm(lm)
        tv = travel_vec / np.linalg.norm(travel_vec)
        rel_ang = math.degrees(math.atan2(tv[1] * lm[0] - tv[0] * lm[1], float(tv @ lm)))
        travel_dir = (rel_ang - 90.0 + 180.0) % 360.0 - 180.0
    else:
        travel_dir = float("nan")  # not going anywhere: direction is meaningless

    # --- action beats: one bounded action, or the same thing over and over? --
    e = motion_energy(joints, fps)
    beats = 0
    if e.size > 2 and e.max() > 1e-6:
        hi, lo = 0.55 * e.max(), 0.30 * e.max()
        armed = True
        for v in e:  # Schmitt trigger, so one noisy peak is not counted twice
            if armed and v > hi:
                beats += 1
                armed = False
            elif not armed and v < lo:
                armed = True

    # --- gait ---------------------------------------------------------------
    if mo["contacts"] is not None and mo["contacts"].shape[0] == T:
        lc = np.asarray(mo["contacts"])[:, 0].astype(bool)
    else:
        lc = joints[:, tables.left_foot, 1] < 0.06
    steps = int((np.diff(lc.astype(int)) > 0).sum())

    m = {
        "frames": T, "fps": fps, "duration_s": round(dur, 2),
        "path_len_m": round(path_len, 2), "net_travel_m": round(net, 2),
        "avg_speed_mps": round(path_len / dur, 2) if dur else 0.0,
        "peak_speed_mps": round(peak, 2),
        "start_ratio": round(float(head_seg.mean()) / peak, 2) if peak > 1e-3 else 0.0,
        "stop_ratio": round(float(tail.mean()) / peak, 2) if peak > 1e-3 else 0.0,
        "moving_frac": round(moving_frac, 2),
        "foot_skate_mps": round(skate, 3),
        "contact_frac": round(contact_frac, 2),
        "penetration_m": round(penetration, 3),
        "lowest_foot_m": round(float_h, 3),
        "jitter_mm": round(jitter, 2),
        "jitter_pct": round(jitter_pct, 1),
        "activity_m": round(activity, 3),
        "head_min_m": round(float(head_y.min()), 2),
        "head_max_m": round(float(head_y.max()), 2),
        "head_end_m": round(float(head_y[-1]), 2),
        "hand_top_m": round(hand_top, 2),
        "hand_reach_m": round(hand_reach, 2),
        "lean_max_deg": round(float(pitch.max()), 1),
        "loop_gap_m": round(loop_gap, 3),
        "loop_vel_gap": round(loop_vel, 2),
        "drift_deg": round(drift, 1),
        # Same number folded into (-180, 180]: a dive-and-roll that turns a full
        # revolution is back where it started, and only the remainder is a fault.
        "net_turn_deg": round((drift + 180.0) % 360.0 - 180.0, 1),
        "travel_dir_deg": round(travel_dir, 0),  # 0 fwd, +90 right, 180 back, -90 left
        "turn_deg": round(turn, 1),
        "left_steps": steps,
        "action_beats": beats,
        "speed_curve": [round(float(x), 2) for x in np.interp(
            np.linspace(0, 1, 8), np.linspace(0, 1, max(speed.size, 1)),
            speed if speed.size else np.zeros(1))],
    }
    m["flags"] = flags_for(m, th, intent)
    return m


def flags_for(m: dict, th: dict, intent: str = "loop") -> list[str]:
    f = []
    if not math.isnan(m["foot_skate_mps"]) and m["foot_skate_mps"] > th["foot_skate_mps"]:
        f.append("SKATE")
    if m["jitter_pct"] > th["jitter_pct"]:
        f.append("JITTER")
    if m["penetration_m"] > th["penetration_m"]:
        f.append("SINKS")
    if m["lowest_foot_m"] > th["float_m"]:
        f.append("FLOATS")
    # "Nothing happens" needs both a still body *and* a still character: a
    # strafe has little root-relative limb excursion (the torso just rides
    # along) yet is plainly not a static clip, because it travels.
    if m["activity_m"] < th["activity_m"] and m["avg_speed_mps"] < th["static_speed_mps"]:
        f.append("STATIC")
    if m["loop_gap_m"] > th["loop_gap_m"]:
        f.append("SEAM")
    if m["loop_vel_gap"] > th["loop_vel_gap"]:
        f.append("SEAMVEL")
    if abs(m["drift_deg"]) > th["drift_deg"]:
        f.append("DRIFT")
    if abs(m["net_turn_deg"]) > th["net_turn_deg"]:
        f.append("TURNS")
    if m["peak_speed_mps"] > 0.8 and m["stop_ratio"] < th["stop_ratio"]:
        f.append("STOPS")  # decelerates to a halt: an episode, not a loop
    if intent == "oneshot":
        f = [x for x in f if x not in LOOP_ONLY_FLAGS]
    return f


# ------------------------------------------------------------ window selection
#
# ARDY completes an *action arc* inside whatever duration it is given: a 3 s
# "swings a sword" is ~1 s of swing plus 2 s of standing, and an 8 s sprint
# accelerates, stops, restarts and stops again. Neither is a usable game clip.
# The fix is not a better prompt — it is generating long and cutting the good
# part out, which these two functions do automatically.


def motion_energy(joints, fps: float):
    """Per-frame speed of the body (root-relative limb motion + root travel)."""
    import numpy as np

    rel = joints - joints[:, :1, :]
    limb = np.linalg.norm(np.diff(rel, axis=0), axis=-1).mean(axis=1) * fps
    root = np.linalg.norm(np.diff(joints[:, 0, [0, 2]], axis=0), axis=1) * fps
    return _smooth((limb + 0.3 * root)[:, None], 3)[:, 0]


def find_action_window(joints, fps: float, thresh: float = 0.18, pad_s: float = 0.15) -> tuple[int, int]:
    """Frame span of the actual action for a oneshot: drop the standing-around
    at both ends, keeping a short pad so the clip can blend in and out."""
    import numpy as np

    e = motion_energy(joints, fps)
    if e.size == 0 or e.max() <= 1e-6:
        return 0, joints.shape[0] - 1
    active = np.flatnonzero(e > thresh * e.max())
    pad = round(pad_s * fps)
    lo = max(0, int(active[0]) - pad)
    hi = min(joints.shape[0] - 1, int(active[-1]) + 1 + pad)
    return lo, hi


def find_loop_window(joints, fps: float, min_s: float = 0.5, max_s: float = 3.0, min_speed: float = 0.0) -> dict:
    """Best cycle-aligned window to cut a seamless loop from.

    Scores every (start, period) pair on how well frame ``start`` matches frame
    ``start+period`` in pose *and* velocity — a walk cycle only loops cleanly
    when both feet are back where they were, moving the same way — and on how
    steady the root speed is inside the window, which is what rejects the
    stop-and-go stretches ARDY produces over long durations.
    """
    import numpy as np

    T = joints.shape[0]
    rel = (joints - joints[:, :1, :]).reshape(T, -1)
    vel = np.diff(rel, axis=0) * fps
    speed = np.concatenate([[0.0], np.linalg.norm(np.diff(joints[:, 0, [0, 2]], axis=0), axis=1) * fps])
    speed = _smooth(speed[:, None], 5)[:, 0]
    peak = float(speed.max())
    nj = joints.shape[1]

    best = None
    p_lo, p_hi = int(min_s * fps), min(int(max_s * fps), T - 2)
    for p in range(max(p_lo, 2), max(p_hi, 2) + 1):
        for s in range(T - p - 1):
            gap = float(np.linalg.norm(rel[s] - rel[s + p])) / math.sqrt(nj)
            vgap = float(np.linalg.norm(vel[s] - vel[s + p])) / math.sqrt(nj)
            win = speed[s:s + p + 1]
            mean_v = float(win.mean())
            cv = float(win.std()) / (mean_v + 0.05)
            # vgap is weighted as heavily as the pose gap: at sprint speed the
            # limbs move far enough per frame that a seam can match in pose and
            # still visibly hitch, which is what the SEAMVEL flag catches.
            score = gap + 0.10 * vgap + 0.10 * cv - 0.02 * (p / fps)
            if peak > 0.4:  # locomotion: reject windows that include a stop
                score += 0.6 * max(0.0, 0.7 - mean_v / peak)
            if mean_v < min_speed:
                # Asked for a moving loop: a window where the character stands
                # still loops perfectly and is worthless, so rule it out.
                score += 10.0 + (min_speed - mean_v)
            if best is None or score < best["score"]:
                best = {"score": score, "start": s, "end": s + p,
                        "period_s": round(p / fps, 2), "gap_m": round(gap, 3),
                        "vel_gap": round(vgap, 2), "mean_speed": round(mean_v, 2),
                        "speed_cv": round(cv, 2)}
    if best is None:
        return {"start": 0, "end": T - 1, "period_s": round((T - 1) / fps, 2),
                "gap_m": 0.0, "vel_gap": 0.0, "mean_speed": 0.0, "speed_cv": 0.0,
                "score": 0.0}
    return best


def suggest(mo: dict, min_speed: float = 0.0) -> dict:
    """Trim arguments for both intended uses of a take."""
    joints, fps, T = mo["joints"], mo["fps"], mo["joints"].shape[0]
    a0, a1 = find_action_window(joints, fps)
    loop = find_loop_window(joints, fps, min_speed=min_speed)
    return {
        "oneshot": {
            "trim_start_s": round(a0 / fps, 2),
            "trim_end_s": round((T - 1 - a1) / fps, 2),
            "kept_s": round((a1 - a0) / fps, 2),
        },
        "loop": {
            "trim_start_s": round(loop["start"] / fps, 2),
            "trim_end_s": round((T - 1 - loop["end"]) / fps, 2),
            "kept_s": loop["period_s"],
            "seam_gap_m": loop["gap_m"],
            "mean_speed_mps": loop["mean_speed"],
            "speed_cv": loop["speed_cv"],
        },
    }


# ---------------------------------------------------------------------- drawing


def project(pts, azim_deg: float, elev_deg: float):
    """Orthographic projection. azim 0 looks at the character's front (+Z)."""
    import numpy as np

    a, e = math.radians(azim_deg), math.radians(elev_deg)
    cam = np.array([math.sin(a) * math.cos(e), math.sin(e), math.cos(a) * math.cos(e)])
    right = np.cross([0.0, 1.0, 0.0], cam)
    right /= np.linalg.norm(right)
    up = np.cross(cam, right)
    flat = pts.reshape(-1, 3)
    out = np.stack([flat @ right, flat @ up], axis=-1)
    return out.reshape(*pts.shape[:-1], 2)


def draw_pose(ax, pts2, tables: Tables, alpha: float = 1.0, lw: float = 2.2) -> None:
    for j, p in enumerate(tables.parents):
        if p < 0:
            continue
        if j in tables.right_joints:
            c = "#e8563f"
        elif j in tables.left_joints:
            c = "#3f8fe8"
        else:
            c = "#333333"
        ax.plot([pts2[p, 0], pts2[j, 0]], [pts2[p, 1], pts2[j, 1]],
                color=c, lw=lw, alpha=alpha, solid_capstyle="round", zorder=3)
    ax.plot(pts2[tables.head, 0], pts2[tables.head, 1], "o", color="#333333", ms=lw * 2.6,
            alpha=alpha, zorder=4)


def _panel(ax, joints, idx, azim, elev, half_w, tables: Tables, ghost=(), center_xz=True):
    import numpy as np

    frame = joints[idx].copy()
    ghosts = [joints[g].copy() for g in ghost]
    if center_xz:
        c = joints[idx, 0, [0, 2]]
        for arr in [frame] + ghosts:
            arr[:, 0] -= c[0]
            arr[:, 2] -= c[1]
    for g, a in zip(ghosts, np.linspace(0.12, 0.3, max(len(ghosts), 1))):
        draw_pose(ax, project(g, azim, elev), tables, alpha=a, lw=1.6)
    draw_pose(ax, project(frame, azim, elev), tables, lw=2.4)
    ground = project(np.array([[-half_w, 0.0, 0.0], [half_w, 0.0, 0.0]]), azim, elev)
    ax.plot(ground[:, 0], ground[:, 1], color="#bbbbbb", lw=1.0, zorder=1)
    ax.set_xlim(-half_w, half_w)
    ax.set_ylim(-0.15, 2.0)
    ax.set_aspect("equal")
    ax.set_xticks([])
    ax.set_yticks([])
    for s in ax.spines.values():
        s.set_color("#dddddd")


def window_frames(mo: dict, mode: str, min_speed: float = 0.0) -> tuple[int, int]:
    """Frame span the sheet should sample over."""
    T = mo["joints"].shape[0]
    if mode == "action":
        return find_action_window(mo["joints"], mo["fps"])
    if mode == "loop":
        w = find_loop_window(mo["joints"], mo["fps"], min_speed=min_speed)
        return w["start"], w["end"]
    return 0, T - 1


def contact_sheet(motions: list[dict], out_png: str, tables: Tables, n_frames: int = 8,
                  azim: float = 35.0, elev: float = 8.0,
                  window: str = "action", min_speed: float = 0.0) -> None:
    """One row per clip: n_frames evenly spaced keyframes, 3/4 view.

    Frames are sampled across the clip's *active* window by default, so a
    oneshot padded with two seconds of standing still still gets eight useful
    panels instead of one pose repeated.
    """
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    import numpy as np

    n = len(motions)
    fig, axes = plt.subplots(n, n_frames, figsize=(1.55 * n_frames, 2.35 * n), squeeze=False)
    for r, mo in enumerate(motions):
        joints = mo["joints"]
        lo, hi = window_frames(mo, window, min_speed)
        idxs = np.linspace(lo, hi, n_frames).round().astype(int)
        for c, i in enumerate(idxs):
            ghost = [max(0, i - 2), max(0, i - 1)]
            _panel(axes[r][c], joints, i, azim, elev, 1.05, tables, ghost=ghost)
            axes[r][c].set_title(f"{i / mo['fps']:.2f}s", fontsize=6, pad=1.5, color="#777777")
        m = mo["metrics"]
        head = (f"{mo['name']}   {m['duration_s']}s  {m['avg_speed_mps']}m/s"
                f"   travel {m['net_travel_m']}m  steps {m['left_steps']}"
                f"   [{window} {lo / mo['fps']:.2f}-{hi / mo['fps']:.2f}s]")
        sub = (f"skate {m['foot_skate_mps']}  jit {m['jitter_pct']}%  "
               f"act {m['activity_m']}  seam {m['loop_gap_m']}  "
               f"stop {m['stop_ratio']}  " + (" ".join(m["flags"]) or "clean"))
        axes[r][0].text(-0.06, 1.44, head, transform=axes[r][0].transAxes,
                        fontsize=9, fontweight="bold", va="bottom", ha="left")
        axes[r][0].text(-0.06, 1.26, sub, transform=axes[r][0].transAxes,
                        fontsize=7.5, va="bottom", ha="left",
                        color="#b03030" if m["flags"] else "#308030")
        if mo["text"]:
            axes[r][0].text(-0.06, 1.09, '"' + mo["text"][:150] + '"',
                            transform=axes[r][0].transAxes, fontsize=7.5,
                            va="bottom", ha="left", color="#555599", style="italic")
    # The first row's header block sits ~0.58 axes-heights above its axes
    # (three text lines at y = 1.09/1.26/1.44 in axes coords). A fixed
    # top=0.90 clipped it off the PNG for small n — the very row an agent
    # names when it picks a take — so reserve exactly that much: with
    # hspace rows between the others get their headroom for free, only the
    # top row needs the figure margin.
    rows = n + 0.55 * (n - 1)
    top = (1 + 0.01 / rows) / (1 + 0.62 / rows)
    fig.subplots_adjust(left=0.01, right=0.99, top=top, bottom=0.02, wspace=0.02, hspace=0.55)
    fig.savefig(out_png, dpi=110, facecolor="white")
    plt.close(fig)


def detail_sheet(mo: dict, out_png: str, tables: Tables, n_frames: int = 10) -> None:
    """Single clip: three camera rows, a world-space onion-skin strip, plots."""
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    import numpy as np

    joints, fps = mo["joints"], mo["fps"]
    T = joints.shape[0]
    a0, a1 = find_action_window(joints, fps)
    loop = find_loop_window(joints, fps)
    idxs = np.linspace(a0, a1, n_frames).round().astype(int)
    views = [("3/4", 35.0, 8.0), ("front", 0.0, 5.0), ("side", 90.0, 5.0)]

    fig = plt.figure(figsize=(1.55 * n_frames, 12.0))
    gs = fig.add_gridspec(6, n_frames, height_ratios=[1, 1, 1, 1.1, 0.75, 0.75], hspace=0.42, wspace=0.03)
    for r, (label, az, el) in enumerate(views):
        for c, i in enumerate(idxs):
            ax = fig.add_subplot(gs[r, c])
            _panel(ax, joints, i, az, el, 1.05, tables, ghost=[max(0, i - 2), max(0, i - 1)])
            if c == 0:
                ax.set_ylabel(label, fontsize=9)
            if r == 0:
                ax.set_title(f"{i / fps:.2f}s", fontsize=6, pad=1.5, color="#777777")

    # world-space onion skin: travel and stride length become visible
    ax = fig.add_subplot(gs[3, :])
    span = float(np.linalg.norm(joints[-1, 0, [0, 2]] - joints[0, 0, [0, 2]]))
    for k, i in enumerate(np.linspace(0, T - 1, min(T, 24)).round().astype(int)):
        p = project(joints[i], 90.0, 4.0)
        draw_pose(ax, p, tables, alpha=0.25 + 0.65 * k / max(1, min(T, 24) - 1), lw=1.6)
    ax.axhline(0.0, color="#bbbbbb", lw=1.0)
    ax.set_aspect("equal")
    ax.set_xticks([])
    ax.set_yticks([])
    ax.set_title(f"world-space onion skin (side view) — net travel {span:.2f} m", fontsize=9)

    t = np.arange(T) / fps
    ax = fig.add_subplot(gs[4, :])
    step = np.linalg.norm(np.diff(mo["root"][:, [0, 2]], axis=0), axis=1) * fps
    ax.plot(t[1:], step, color="#3f8fe8", lw=1.6, label="root speed m/s")
    ax.plot(t, joints[:, tables.head, 1], color="#e8563f", lw=1.4, label="head height m")
    ax.axvspan(a0 / fps, a1 / fps, color="#3f8fe8", alpha=0.08, lw=0)
    ax.axvspan(loop["start"] / fps, loop["end"] / fps, color="#30a030", alpha=0.14, lw=0)
    ax.set_title(f"blue span = action window {a0 / fps:.2f}-{a1 / fps:.2f}s   "
                 f"green span = best loop window {loop['start'] / fps:.2f}-"
                 f"{loop['end'] / fps:.2f}s ({loop['period_s']}s, seam "
                 f"{loop['gap_m']}m, speed cv {loop['speed_cv']})", fontsize=8)
    ax.legend(fontsize=7, loc="upper right")
    ax.grid(alpha=0.25)
    ax.set_xlim(0, t[-1] if T > 1 else 1)
    ax.tick_params(labelsize=7)

    ax = fig.add_subplot(gs[5, :])
    for j, name, col in [(tables.left_foot, "L foot", "#3f8fe8"), (tables.right_foot, "R foot", "#e8563f")]:
        ax.plot(t, joints[:, j, 1], color=col, lw=1.4, label=name)
    ax.axhline(0.0, color="#999999", lw=0.8)
    ax.axhline(0.06, color="#cccccc", lw=0.8, ls="--")
    ax.legend(fontsize=7, loc="upper right")
    ax.grid(alpha=0.25)
    ax.set_xlim(0, t[-1] if T > 1 else 1)
    ax.set_xlabel("seconds", fontsize=8)
    ax.tick_params(labelsize=7)

    m = mo["metrics"]
    fig.suptitle(
        f"{mo['name']}   |   {mo['text'][:110]}\n"
        f"{m['duration_s']}s @{m['fps']:.0f}fps  travel {m['net_travel_m']}m  "
        f"avg {m['avg_speed_mps']}m/s  skate {m['foot_skate_mps']}  "
        f"jitter {m['jitter_pct']}% ({m['jitter_mm']}mm)  activity {m['activity_m']}  "
        f"seam {m['loop_gap_m']}  drift {m['drift_deg']}deg  "
        f"steps {m['left_steps']}   FLAGS: {' '.join(m['flags']) or 'none'}",
        fontsize=10, y=0.985)
    fig.subplots_adjust(left=0.02, right=0.99, top=0.945, bottom=0.03)
    fig.savefig(out_png, dpi=110, facecolor="white")
    plt.close(fig)


# ------------------------------------------------------------------------- table


def format_table(motions: list[dict]) -> str:
    rows = []
    for mo in motions:
        m = mo["metrics"]
        rows.append([mo["name"]] + [
            " ".join(m["flags"]) or "-" if c == "flags" else str(m[c])
            for c in COLS[1:]])
    widths = [max(len(str(r[i])) for r in [COLS] + rows) for i in range(len(COLS))]
    line = "  ".join(h.ljust(w) for h, w in zip(COLS, widths))
    out = [line, "-" * len(line)]
    for r in rows:
        out.append("  ".join(str(v).ljust(w) for v, w in zip(r, widths)))
    return "\n".join(out)


def format_suggestions(motions: list[dict]) -> str:
    out = ["Suggested trims (feed to promote clip):"]
    for mo in motions:
        s = mo["metrics"]["suggest"]
        o, lp = s["oneshot"], s["loop"]
        out.append(f"  {mo['name']}")
        out.append(f"    oneshot: --trim-start {o['trim_start_s']} --trim-end {o['trim_end_s']}   ({o['kept_s']}s kept)")
        out.append(f"    loop:    --trim-start {lp['trim_start_s']} --trim-end {lp['trim_end_s']} --loop   ({lp['kept_s']}s cycle, "
                   f"seam {lp['seam_gap_m']}m, {lp['mean_speed_mps']}m/s, cv {lp['speed_cv']})")
    return "\n".join(out)


def _json_safe(value):
    """NaN and infinities become ``null``: strict JSON (and serde) has no spelling for them."""
    if isinstance(value, float):
        return value if math.isfinite(value) else None
    if isinstance(value, dict):
        return {k: _json_safe(v) for k, v in value.items()}
    if isinstance(value, list):
        return [_json_safe(v) for v in value]
    return value


def metrics_document(motions: list[dict]) -> dict:
    """The frozen JSON shape: ``{name: {metrics..., flags, suggest, prompt, path}}``."""
    return _json_safe({m["name"]: dict(m["metrics"], prompt=m["text"], path=m["path"]) for m in motions})


# --------------------------------------------------------------- argparse --


def _add_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("files", nargs="+", metavar="NPZ", help="ARDY .npz takes")
    parser.add_argument("--intent", choices=INTENTS, default="loop", help="oneshot suppresses flags that only matter for looping clips")
    parser.add_argument("--sheet", metavar="PNG", help="write the contact sheet here (none without it)")
    parser.add_argument("--metrics", metavar="JSON", help="also write the metrics document here")
    parser.add_argument("--detail", action="store_true", help="full single-clip sheet (3 views + onion skin + plots)")
    parser.add_argument("--frames", type=int, default=8, help="keyframes per row (default 8)")
    parser.add_argument("--sort", metavar="METRIC", help="sort rows by this metric (e.g. jitter_pct, foot_skate_mps)")
    parser.add_argument("--window", choices=WINDOWS, default="action", help="frame span the sheet samples over (default action)")
    parser.add_argument("--min-speed", type=float, default=0.0, metavar="MPS",
                        help="loop windows must average at least this speed — set it for locomotion, or a standing-still window wins")
    parser.add_argument("--suggest", action="store_true", help="print the trim args that cut the usable clip out of each take")
    parser.add_argument("--trim-start", type=float, default=0.0, metavar="S", help="review the take as if trimmed (same units as promote)")
    parser.add_argument("--trim-end", type=float, default=0.0, metavar="S")
    parser.add_argument("--profile", metavar="DIR", help="rig profile directory (default: $FORGE_RIG_PROFILE or rigs/humanoid)")


def add_parser(subparsers) -> None:
    """Register ``motion review``."""
    parser = subparsers.add_parser(
        "review",
        help="Metrics table and contact sheet for a sweep (no GPU)",
        description=__doc__,
    )
    _add_arguments(parser)


def _inner_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="forge_gen.motion.review --inner", description="inner half of motion review")
    _add_arguments(parser)
    return parser


def _forward_argv(args, files: list[Path], sheet: Path | None, metrics: Path | None, profile_dir: Path) -> list[str]:
    argv = [str(f) for f in files]
    argv += ["--intent", args.intent, "--frames", str(args.frames), "--window", args.window]
    argv += ["--min-speed", f"{args.min_speed:g}", "--trim-start", f"{args.trim_start:g}", "--trim-end", f"{args.trim_end:g}"]
    argv += ["--profile", str(profile_dir)]
    if sheet is not None:
        argv += ["--sheet", str(sheet)]
    if metrics is not None:
        argv += ["--metrics", str(metrics)]
    if args.detail:
        argv.append("--detail")
    if args.sort:
        argv += ["--sort", args.sort]
    if args.suggest:
        argv.append("--suggest")
    return argv


def _validate(args) -> tuple[list[Path], Path | None, Path | None, Path]:
    files = [Path(f).expanduser().resolve() for f in args.files]
    for f in files:
        if not f.is_file():
            raise InputRejected(f"{f} is not a file")
    if args.detail and len(files) != 1:
        raise UsageError("--detail takes exactly one file")
    if args.frames < 1:
        raise UsageError("--frames must be at least 1")
    if args.trim_start < 0 or args.trim_end < 0:
        raise UsageError("--trim-start/--trim-end are non-negative seconds")
    sheet = Path(args.sheet).expanduser().resolve() if args.sheet else None
    metrics = Path(args.metrics).expanduser().resolve() if args.metrics else None
    for target in (sheet, metrics):
        if target is not None:
            target.parent.mkdir(parents=True, exist_ok=True)
    profile_dir = Path(args.profile).expanduser().resolve() if args.profile else profile_mod.default_dir()
    try:
        profile_mod.load_profile(profile_dir)
    except profile_mod.ProfileError as err:
        raise InputRejected(str(err)) from err
    return files, sheet, metrics, profile_dir


def _numpy_here() -> bool:
    try:
        import numpy  # noqa: F401
    except ImportError:
        return False
    return True


# ------------------------------------------------------------------ outer --


def run(args) -> dict:
    """Outer: validate, run the inner under the ardy env (or here, when ardy is absent and numpy is not)."""
    from forge_gen import backends as backends_mod
    from forge_gen import launcher
    from forge_gen.exit_codes import MissingBackend

    files, sheet, metrics, profile_dir = _validate(args)
    argv = _forward_argv(args, files, sheet, metrics, profile_dir)
    try:
        backend = backends_mod.load_backend("ardy")
        launcher.resolve_interpreter(backend)
    except MissingBackend as err:
        if not _numpy_here():
            raise MissingBackend(
                f"{err.message}; review needs numpy (and matplotlib for a sheet), which this interpreter lacks too",
                backend="ardy",
                hint=err.hint,
            ) from err
        sys.stderr.write("forge-gen: ardy is not installed; reviewing with this interpreter's numpy\n")
        result = main_inner(argv)
        return _summary(result)
    result = launcher.run_inner_checked(backend, "motion.review", argv)
    return _summary(result)


def _summary(result: dict) -> dict:
    # The inner already printed the table (and the sheet line) as progress;
    # the human summary here is only what the inner could not know.
    outputs = [p for p in (result.get("sheet"), result.get("metrics_json")) if p]
    names = list(result.get("metrics", {}))
    flagged = [name for name in names if result["metrics"][name].get("flags")]
    text = f"reviewed {len(names)} take(s); {len(flagged)} flagged"
    if result.get("fake"):
        text = result.get("table", "") + "\n" + text + " (fake)"
    if result.get("metrics_json"):
        text += f"\nmetrics: {result['metrics_json']}"
    return {
        "record": None,
        "outputs": outputs,
        "sheet": result.get("sheet"),
        "metrics_json": result.get("metrics_json"),
        "metrics": result.get("metrics", {}),
        "fake": bool(result.get("fake", False)),
        "_text": text,
    }


# ------------------------------------------------------------------- fake --


def run_fake(args) -> dict:
    """A placeholder sheet and a metrics document of zeros in the frozen shape, no numpy."""
    from forge_gen import placeholders

    files, sheet, metrics, _ = _validate(args)
    doc = {}
    for f in files:
        name = f.stem
        zero = {key: 0.0 for key in (
            "duration_s", "path_len_m", "net_travel_m", "avg_speed_mps", "peak_speed_mps", "start_ratio",
            "stop_ratio", "moving_frac", "foot_skate_mps", "contact_frac", "penetration_m", "lowest_foot_m",
            "jitter_mm", "jitter_pct", "activity_m", "head_min_m", "head_max_m", "head_end_m", "hand_top_m",
            "hand_reach_m", "lean_max_deg", "loop_gap_m", "loop_vel_gap", "drift_deg", "net_turn_deg",
            "travel_dir_deg", "turn_deg",
        )}
        entry = {"frames": 0, "fps": 0.0}
        entry.update(zero)
        entry.update({
            "left_steps": 0, "action_beats": 0, "speed_curve": [0.0] * 8, "flags": [],
            "suggest": {
                "oneshot": {"trim_start_s": 0.0, "trim_end_s": 0.0, "kept_s": 0.0},
                "loop": {"trim_start_s": 0.0, "trim_end_s": 0.0, "kept_s": 0.0, "seam_gap_m": 0.0, "mean_speed_mps": 0.0, "speed_cv": 0.0},
            },
            "prompt": "", "path": str(f),
        })
        doc[name] = entry
    if sheet is not None:
        placeholders.tile_png(sheet, columns=args.frames, rows=len(files))
    if metrics is not None:
        with open(metrics, "w", encoding="utf-8") as handle:
            json.dump(doc, handle, indent=2)
            handle.write("\n")
    table = "\n".join(f"{name}  (fake: no metrics)" for name in doc)
    return _summary({"metrics": doc, "sheet": str(sheet) if sheet else None, "metrics_json": str(metrics) if metrics else None, "table": table, "fake": True})


# ------------------------------------------------------------------ inner --


def main_inner(argv: list[str]) -> dict:
    args = _inner_parser().parse_args(argv)
    files, sheet, metrics_path, profile_dir = _validate(args)
    prof = profile_mod.load_profile(profile_dir)
    tables = Tables(prof)
    th = thresholds_from(prof)

    motions = []
    for f in files:
        mo = load_motion(str(f), tables)
        if args.trim_start or args.trim_end:
            a = round(args.trim_start * mo["fps"])
            b = mo["joints"].shape[0] - round(args.trim_end * mo["fps"])
            if b - a < 2:
                raise InputRejected(f"trim leaves fewer than 2 frames for {f}")
            mo["joints"] = mo["joints"][a:b]
            mo["root"] = mo["root"][a:b]
            if mo["contacts"] is not None:
                mo["contacts"] = mo["contacts"][a:b]
            mo["name"] += f"[{args.trim_start:g}:{args.trim_end:g}]"
        if mo["joints"].shape[0] < 2:
            raise InputRejected(f"{f} has {mo['joints'].shape[0]} frame(s); a review needs at least 2")
        mo["metrics"] = analyze(mo, tables, th, args.intent)
        mo["metrics"]["suggest"] = suggest(mo, args.min_speed)
        motions.append(mo)
    if args.sort:
        if args.sort not in motions[0]["metrics"]:
            raise UsageError(f"--sort {args.sort}: not a metric ({', '.join(k for k in motions[0]['metrics'] if k not in ('flags', 'suggest', 'speed_curve'))})")
        motions.sort(key=lambda m: m["metrics"].get(args.sort, 0))

    table = format_table(motions)
    print(table, flush=True)
    suggestions = None
    if args.suggest:
        suggestions = format_suggestions(motions)
        print("\n" + suggestions, flush=True)

    doc = metrics_document(motions)
    if metrics_path is not None:
        with open(metrics_path, "w", encoding="utf-8") as handle:
            json.dump(doc, handle, indent=2)
            handle.write("\n")

    if sheet is not None:
        if args.detail:
            detail_sheet(motions[0], str(sheet), tables, n_frames=max(args.frames, 8))
        else:
            contact_sheet(motions, str(sheet), tables, n_frames=args.frames, window=args.window, min_speed=args.min_speed)
        print(f"\nContact sheet: {sheet}", flush=True)

    return {
        "metrics": doc,
        "sheet": str(sheet) if sheet else None,
        "metrics_json": str(metrics_path) if metrics_path else None,
        "table": table,
        "suggestions": suggestions,
        "fake": False,
    }


if __name__ == "__main__":
    from forge_gen.motion import session

    if len(sys.argv) > 1 and sys.argv[1] == "--inner":
        sys.exit(session.inner_entry(main_inner, sys.argv[2:]))
    sys.stderr.write("run me through forge-gen: python3 python/forge_gen motion review ...\n")
    sys.exit(2)
