"""The rig profile, read as data: ``profile.toml`` and the three JSON files beside it.

Every number the pipeline holds a body, a prop or a clip to lives under
``rigs/<name>/`` and is read from there by Rust (``forge_rig``) and by these
modules alike; nothing here is a constant. The Blender steps read the fit
gate, the budgets and the finger layout; the motion review reads its
thresholds and the driven joint order; ``npz.write_take`` chains the contract
rest pose for a still figure.

Stdlib only (``tomllib`` is 3.11+, which ``forge_gen`` requires anyway).
"""

from __future__ import annotations

import json
import os
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

#: The profile a toolkit checkout ships, by directory name under ``rigs/``.
DEFAULT_NAME = "humanoid"


class ProfileError(Exception):
    """The profile directory is not one, or a file in it does not say what it must."""


def repo_root() -> Path:
    """The toolkit checkout this package lives in: the directory above ``python/``."""
    return Path(__file__).resolve().parent.parent.parent


def default_dir() -> Path:
    """The rig profile in force when none is named.

    ``$FORGE_RIG_PROFILE`` when set (what the Rust ``forge gen`` exports),
    else the *project's* profile — ``forge.toml``'s ``[paths] rigs`` and
    ``[project] rig``, from ``--project`` when it was given or the nearest
    ``forge.toml`` above the working directory — and the toolkit's own
    ``rigs/humanoid`` only as the final fallback. ``python3 python/forge_gen``
    run bare used to skip the project half and silently hold a body to the
    toolkit's contract instead of the project's.
    """
    override = os.environ.get("FORGE_RIG_PROFILE")
    if override:
        return Path(override).expanduser().resolve()
    root = _project_root()
    if root is not None:
        named = _project_profile_dir(root)
        if named is not None:
            return named
    return repo_root() / "rigs" / DEFAULT_NAME


def _project_root() -> Path | None:
    """The project in force: ``--project`` (via ``records.set_project``), else the nearest ``forge.toml`` above cwd."""
    from forge_gen import records

    known = records.project()
    if known is not None:
        return known
    current = Path.cwd().resolve()
    for candidate in (current, *current.parents):
        if (candidate / "forge.toml").is_file():
            return candidate
    return None


def _project_profile_dir(root: Path) -> Path | None:
    """``<root>/<paths.rigs>/<project.rig>`` from the project's ``forge.toml``, or ``None`` when it does not parse.

    Returned even when the directory is absent: a project that names a
    profile it does not have should fail loudly with that path in the
    message, not fall back to the toolkit's copy and pass the wrong gates.
    """
    try:
        with open(root / "forge.toml", "rb") as handle:
            data = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError):
        return None
    paths = data.get("paths") if isinstance(data.get("paths"), dict) else {}
    project = data.get("project") if isinstance(data.get("project"), dict) else {}
    rigs = str(paths.get("rigs", "rigs"))
    rig = str(project.get("rig", DEFAULT_NAME))
    return (root / rigs / rig).resolve()


@dataclass
class Profile:
    """One rig profile, every file parsed."""

    #: The directory the profile was read from.
    dir: Path
    #: ``profile.toml`` as a dict: ``[profile]``, ``[bones]``, ``[fit]``, ``[rig]``, ``[prop]``,
    #: ``[material]``, ``[export]``, ``[fingers]``, ``[review]``.
    toml: dict
    #: ``contract.json``: the bone table derived from ``rig.glb``.
    contract: dict
    #: ``sockets.json``: where a prop rides.
    sockets: dict
    #: ``motion_skeleton.json``: the driven layout, in take order.
    motion_skeleton: dict
    #: The skeleton as an artifact.
    rig_glb: Path
    #: The same skeleton as a Blender file; the auto-rig opens it.
    rig_blend: Path
    #: The driven-only clip ``rig-build`` rebuilds ``rig.blend`` from.
    fixture_clip: Path | None = None
    _by_name: dict = field(default_factory=dict, repr=False)

    @property
    def name(self) -> str:
        """The profile's name."""
        return str(self.toml["profile"]["name"])

    @property
    def bones(self) -> list[dict]:
        """Every bone, in contract order: ``{name, parent, driven, rest_translation, rest_rotation}``."""
        return self.contract["bones"]

    @property
    def root(self) -> str:
        """The only rootless bone."""
        return str(self.contract["root"])

    @property
    def joints(self) -> list[str]:
        """The driven joints, in the order a take writes them."""
        return list(self.motion_skeleton["joints"])

    @property
    def joint_parents(self) -> list[int | None]:
        """Parent index per driven joint, in take order; ``None`` for the root."""
        return list(self.motion_skeleton["parents"])

    def bone(self, name: str) -> dict:
        """The contract bone with this name."""
        try:
            return self._by_name[name]
        except KeyError:
            raise ProfileError(f"{self.dir}: contract.json has no bone {name!r}") from None

    def bone_index(self, name: str) -> int:
        """The contract index of a bone."""
        return self.bones.index(self.bone(name))

    def section(self, name: str) -> dict:
        """One ``[section]`` of profile.toml, refusing a missing one by name."""
        try:
            return self.toml[name]
        except KeyError:
            raise ProfileError(f"{self.dir}/profile.toml has no [{name}] section") from None

    def socket(self, name: str) -> dict:
        """One socket by name: ``{name, bone, translation, rotation, note}``."""
        for socket in self.sockets["sockets"]:
            if socket["name"] == name:
                return socket
        raise ProfileError(f"{self.dir}: sockets.json has no socket {name!r}")

    def rest_world(self) -> dict[str, tuple[tuple[float, float, float], tuple[float, float, float, float]]]:
        """Every bone's rest position and rotation in the armature's space.

        Chains ``rest_translation``/``rest_rotation`` (glTF node transforms,
        ``[x, y, z, w]``) from the root down. This is the T-pose the sockets
        are measured against and the still figure a fake take stands in.
        """
        out: dict[str, tuple] = {}
        bones = self.bones

        def world(index: int) -> tuple:
            bone = bones[index]
            if bone["name"] in out:
                return out[bone["name"]]
            translation = tuple(float(v) for v in bone["rest_translation"])
            rotation = tuple(float(v) for v in bone["rest_rotation"])
            parent = bone.get("parent")
            if parent is None:
                result = (translation, rotation)
            else:
                parent_pos, parent_rot = world(parent)
                result = (_add(parent_pos, _rotate(parent_rot, translation)), _qmul(parent_rot, rotation))
            out[bone["name"]] = result
            return result

        for index in range(len(bones)):
            world(index)
        return out


def _qmul(a: tuple, b: tuple) -> tuple:
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return (
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    )


def _rotate(q: tuple, v: tuple) -> tuple:
    x, y, z, w = q
    vx, vy, vz = v
    # v' = v + 2 * cross(q.xyz, cross(q.xyz, v) + w * v)
    tx = 2.0 * (y * vz - z * vy)
    ty = 2.0 * (z * vx - x * vz)
    tz = 2.0 * (x * vy - y * vx)
    return (
        vx + w * tx + (y * tz - z * ty),
        vy + w * ty + (z * tx - x * tz),
        vz + w * tz + (x * ty - y * tx),
    )


def _add(a: tuple, b: tuple) -> tuple:
    return (a[0] + b[0], a[1] + b[1], a[2] + b[2])


def _read_json(path: Path, what: str) -> dict:
    if not path.is_file():
        raise ProfileError(f"{path.parent}: no {what} ({path.name})")
    try:
        with open(path, encoding="utf-8") as handle:
            return json.load(handle)
    except json.JSONDecodeError as err:
        raise ProfileError(f"{path}: does not parse: {err}") from err


def load_profile(directory: str | os.PathLike | None = None) -> Profile:
    """Read a profile directory (default: :func:`default_dir`), checking the pieces agree.

    The checks are the cheap structural ones — the bone count matches
    ``[bones] count``, every driven joint is a contract bone, every socket
    names a contract bone, one root — so a command fails at its first line
    rather than in Blender twenty seconds later.
    """
    directory = Path(directory).expanduser().resolve() if directory else default_dir()
    toml_path = directory / "profile.toml"
    if not toml_path.is_file():
        raise ProfileError(f"{directory} is not a rig profile: no profile.toml")
    try:
        with open(toml_path, "rb") as handle:
            data = tomllib.load(handle)
    except tomllib.TOMLDecodeError as err:
        raise ProfileError(f"{toml_path}: does not parse: {err}") from err
    head = data.get("profile")
    if not isinstance(head, dict) or "name" not in head:
        raise ProfileError(f"{toml_path}: no [profile] name")

    def named(key: str, fallback: str) -> Path:
        return directory / str(head.get(key, fallback))

    contract = _read_json(named("contract", "contract.json"), "contract")
    sockets = _read_json(named("sockets", "sockets.json"), "sockets")
    motion = _read_json(named("motion_skeleton", "motion_skeleton.json"), "motion skeleton")
    bones = contract.get("bones")
    if not isinstance(bones, list) or not bones:
        raise ProfileError(f"{directory}: contract.json lists no bones")
    by_name = {bone["name"]: bone for bone in bones}
    if len(by_name) != len(bones):
        raise ProfileError(f"{directory}: contract.json names a bone twice")
    roots = [bone["name"] for bone in bones if bone.get("parent") is None]
    if roots != [contract.get("root")]:
        raise ProfileError(f"{directory}: contract.json roots are {roots}, root says {contract.get('root')!r}")
    declared = data.get("bones", {})
    if "count" in declared and int(declared["count"]) != len(bones):
        raise ProfileError(f"{directory}: profile.toml says {declared['count']} bones, contract.json has {len(bones)}")
    joints = motion.get("joints", [])
    if "driven" in declared and int(declared["driven"]) != len(joints):
        raise ProfileError(f"{directory}: profile.toml says {declared['driven']} driven, motion_skeleton.json lists {len(joints)}")
    for joint in joints:
        bone = by_name.get(joint)
        if bone is None:
            raise ProfileError(f"{directory}: driven joint {joint!r} is not a contract bone")
        if not bone.get("driven", False):
            raise ProfileError(f"{directory}: joint {joint!r} is driven by takes but not marked driven in the contract")
    for socket in sockets.get("sockets", []):
        if socket.get("bone") not in by_name:
            raise ProfileError(f"{directory}: socket {socket.get('name')!r} rides {socket.get('bone')!r}, not a contract bone")
    fixture = head.get("fixture_clip")
    return Profile(
        dir=directory,
        toml=data,
        contract=contract,
        sockets=sockets,
        motion_skeleton=motion,
        rig_glb=named("rig_glb", "rig.glb"),
        rig_blend=named("rig_blend", "rig.blend"),
        fixture_clip=directory / str(fixture) if fixture else None,
        _by_name=by_name,
    )
