"""Packs the mod into argv[1] (default dist/): steam_controller.hnmod, the native code, as
docs/api/mod-files.md describes it. Steam Controller has no content mod, so there is no .hmod.
The packs already there move first to past-backups/<their version>/<when they were made>/."""
import json
import os
import re
import shutil
import sys
import time
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
MOD = os.path.dirname(HERE)
OUT = sys.argv[1] if len(sys.argv) > 1 else os.path.join(MOD, "dist")
BACKUPS = os.path.join(MOD, "past-backups")

GAME = "26.3"
# The horizon-mixins commit the launcher built this Horizon with (native-mods.json's horizon_mixins).
MIXINS = "5a64af938ec86b00d53d16a408aabc0038fb3e57"


def files(root):
    for base, dirs, names in os.walk(root):
        dirs[:] = sorted(d for d in dirs if d not in ("target", "__pycache__"))
        for name in sorted(names):
            path = os.path.join(base, name)
            yield path, os.path.relpath(path, root).replace(os.sep, "/")


def toml_value(text, section, key):
    block = re.search(rf"^\[{section}\]\n(.*?)(?=^\[|\Z)", text, re.M | re.S).group(1)
    return re.search(rf'^{key}\s*=\s*"([^"]+)"', block, re.M).group(1)


def back_up_old_packs():
    """Moves the packs in OUT to past-backups/<version>/<YYYY-MM-DD_HH-MM-SS>/, the version read
    from the native pack's manifest and the time from when it was made."""
    old = [os.path.join(OUT, n) for n in sorted(os.listdir(OUT)) if n.endswith((".hmod", ".hnmod"))]
    if not old:
        return
    version = "unknown"
    for path in old:
        if path.endswith(".hnmod"):
            try:
                with zipfile.ZipFile(path) as z:
                    version = json.loads(z.read("horizon-native.json"))["version"]
            except (KeyError, ValueError, zipfile.BadZipFile):
                pass
    made = time.strftime("%Y-%m-%d_%H-%M-%S", time.localtime(max(os.path.getmtime(p) for p in old)))
    folder = os.path.join(BACKUPS, version, made)
    os.makedirs(folder, exist_ok=True)
    for path in old:
        shutil.move(path, os.path.join(folder, os.path.basename(path)))
    print(f"moved the old packs to {folder}")


def native_entry(rel):
    """What mod-files.md lets a bundle hold under mixins/<id>/."""
    name = rel.rsplit("/", 1)[-1]
    top = rel.count("/") <= 1
    return (rel.endswith((".rs", ".toml"))
            or rel.startswith("data/") or re.match(r"[^/]+/data/", rel)
            or (top and re.match(r"(README|LICEN[CS]E|COPYING|NOTICE)|\.gitignore$", name)))


os.makedirs(OUT, exist_ok=True)
back_up_old_packs()

# The native mod, with the licence and readme beside its sources.
native = os.path.join(MOD, "native")
with open(os.path.join(native, "horizon-mixin.toml"), encoding="utf-8") as f:
    mixin = f.read()
native_id = toml_value(mixin, "mod", "id")
manifest = {
    "schema": 1,
    "id": native_id,
    "version": toml_value(mixin, "mod", "version"),
    "game": GAME,
    "horizon": toml_value(mixin, "compatibility", "horizon_revision"),
    "mixins": MIXINS,
}
hnmod = os.path.join(OUT, f"{native_id}.hnmod")
with zipfile.ZipFile(hnmod, "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("horizon-native.json", json.dumps(manifest, indent=2) + "\n")
    for path, rel in files(native):
        if native_entry(rel):
            z.write(path, f"mixins/{native_id}/{rel}")
    z.write(os.path.join(MOD, "LICENSE"), f"mixins/{native_id}/LICENSE")
    z.write(os.path.join(MOD, "README.md"), f"mixins/{native_id}/README.md")
print(hnmod)
