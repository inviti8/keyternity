#!/usr/bin/env python3
# /// script
# dependencies = ["icnsutil", "resvg-py"]
# ///
"""
Regenerate Keyternity's app icons from the master SVG (assets/icon_SRC.svg).

Adapted from Inkternity's scripts/regen_icons.py. Two stages, so line weights
match across sizes:
  1. Rasterize icon_SRC.svg once at 2048x2048 (alpha kept) in a temp dir.
  2. Lanczos-resample every derivative from that master.

The SVG is rasterized with resvg, not magick: magick's SVG coders (its own
MSVG, or a bundled librsvg 2.40) ignore `paint-order`, which the icon relies
on to tuck strokes behind fills, so strokes came out on top and too heavy.

The ICO is a true multi-image container, built by handing magick all the
per-size PNGs. The ICNS is packed with icnsutil, because magick's ICNS coder
writes a single frame; OSTypes are declared explicitly since 1x and 2x
variants share pixel sizes. The PEP 723 header lets `uv run` install both.

Outputs (relative to the repo root):
  assets/icon.png                                      256x256, window icon (src/lib.rs)
  assets/icon.ico                                      16-256, exe resource (build.rs)
                                                       and web favicon (web_build.py)
  release/Keyternity.app/Contents/Resources/keyternity.icns
                                                       16-1024 with retina variants

Requires ImageMagick (`magick`) on PATH. Idempotent.

Usage (from the repo root):
  uv run tools/regen_icons.py
  uv run tools/regen_icons.py --check        # just verify magick is installed
  uv run tools/regen_icons.py --keep-master  # keep the 2048 px master for inspection
"""

import argparse
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
MASTER_SVG = REPO_ROOT / "assets" / "icon_SRC.svg"

# Every derivative (up to the 1024 px retina ICNS frame) is a downsample of this.
MASTER_RASTER_SIZE = 2048

PNG_TARGETS: list[tuple[str, int]] = [
    ("assets/icon.png", 256),
]

ICO_TARGET = "assets/icon.ico"
ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]

ICNS_TARGET = "release/Keyternity.app/Contents/Resources/keyternity.icns"
#   icp4 = 16x16        | ic11 = 16x16 @2x = 32x32 px
#   icp5 = 32x32        | ic12 = 32x32 @2x = 64x64 px
#   ic07 = 128x128      | ic13 = 128x128 @2x = 256x256 px
#   ic08 = 256x256      | ic14 = 256x256 @2x = 512x512 px
#   ic09 = 512x512      | ic10 = 512x512 @2x = 1024x1024 px
ICNS_ENTRIES: list[tuple[str, int]] = [
    ("icp4", 16),
    ("icp5", 32),
    ("ic07", 128),
    ("ic08", 256),
    ("ic09", 512),
    ("ic10", 1024),
    ("ic11", 32),
    ("ic12", 64),
    ("ic13", 256),
    ("ic14", 512),
]


def have_magick() -> bool:
    return shutil.which("magick") is not None


def run_magick(args: list[str], label: str) -> bool:
    cmd = ["magick", *args]
    print(f"  {label}")
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode != 0:
        print(
            f"    ERROR (exit {result.returncode}): "
            f"{result.stderr.strip() or result.stdout.strip()}",
            file=sys.stderr,
        )
        return False
    return True


def display(p: Path) -> str:
    try:
        return str(p.relative_to(REPO_ROOT))
    except ValueError:
        return str(p)


def rasterize_master(dest: Path) -> bool:
    """SVG -> 2048 px PNG with alpha, rendered by resvg."""
    import resvg_py  # only needed past --check

    print(f"  -> master ({MASTER_RASTER_SIZE}x{MASTER_RASTER_SIZE}, transparent)")
    try:
        png = resvg_py.svg_to_bytes(
            svg_path=str(MASTER_SVG),
            width=MASTER_RASTER_SIZE,
            height=MASTER_RASTER_SIZE,
        )
    except Exception as e:
        print(f"    ERROR: resvg failed: {e}", file=sys.stderr)
        return False
    dest.write_bytes(bytes(png))
    return True


def resample_png(master: Path, size: int, dest: Path) -> bool:
    dest.parent.mkdir(parents=True, exist_ok=True)
    return run_magick(
        [
            str(master),
            "-background", "none",
            "-filter", "Lanczos",
            "-resize", f"{size}x{size}",
            f"PNG32:{dest}",
        ],
        f"-> {display(dest)}  ({size}x{size})",
    )


def build_ico(master: Path, sizes: list[int], dest: Path, tmp: Path) -> bool:
    parts = []
    for size in sizes:
        part = tmp / f"ico_{size}.png"
        if not resample_png(master, size, part):
            return False
        parts.append(str(part))
    if dest.exists():
        dest.unlink()
    return run_magick([*parts, str(dest)], f"-> {display(dest)}  (sizes {sizes})")


def build_icns(master: Path, entries: list[tuple[str, int]], dest: Path, tmp: Path) -> bool:
    import icnsutil  # only needed past --check

    dest.parent.mkdir(parents=True, exist_ok=True)
    by_size: dict[int, Path] = {}
    for _, size in entries:
        if size not in by_size:
            part = tmp / f"icns_{size}.png"
            if not resample_png(master, size, part):
                return False
            by_size[size] = part
    icns = icnsutil.IcnsFile()
    for ostype, size in entries:
        icns.add_media(ostype, file=str(by_size[size]))
    if dest.exists():
        dest.unlink()
    icns.write(str(dest))
    print(f"  -> {display(dest)}  ({len(entries)} frames)")
    return True


def main() -> int:
    parser = argparse.ArgumentParser(description="Regenerate app icons from icon_SRC.svg")
    parser.add_argument("--check", action="store_true", help="Verify magick is on PATH and exit")
    parser.add_argument("--keep-master", action="store_true",
                        help="Keep the master raster next to icon_SRC.svg")
    args = parser.parse_args()

    if not have_magick():
        print("ERROR: ImageMagick `magick` not on PATH.", file=sys.stderr)
        return 1
    if args.check:
        print("magick OK")
        return 0
    if not MASTER_SVG.exists():
        print(f"ERROR: master SVG not found at {MASTER_SVG}", file=sys.stderr)
        return 1

    print(f"Master SVG: {display(MASTER_SVG)}")
    failures: list[str] = []
    with tempfile.TemporaryDirectory(prefix="regen_icons_") as tmp_str:
        tmp = Path(tmp_str)
        master = tmp / "icon_master.png"
        if not rasterize_master(master):
            print("FATAL: master rasterization failed.", file=sys.stderr)
            return 1

        for rel, size in PNG_TARGETS:
            if not resample_png(master, size, REPO_ROOT / rel):
                failures.append(rel)
        if not build_ico(master, ICO_SIZES, REPO_ROOT / ICO_TARGET, tmp):
            failures.append(ICO_TARGET)
        if not build_icns(master, ICNS_ENTRIES, REPO_ROOT / ICNS_TARGET, tmp):
            failures.append(ICNS_TARGET)

        if args.keep_master:
            keep = MASTER_SVG.parent / f"icon_master_{MASTER_RASTER_SIZE}.png"
            shutil.copy2(master, keep)
            print(f"Kept master raster at {display(keep)}")

    if failures:
        print(f"FAILED: {failures}", file=sys.stderr)
        return 1
    print("All icons regenerated. Review with `git status` before committing.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
