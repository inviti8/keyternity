import subprocess
import shutil
import sys
import argparse
import os
import hashlib
import urllib.request

RED = "\033[31m"
RESET = "\033[0m"

if not shutil.which("trunk"):
    print(f">>> {RED}!! trunk REQUIRED !!{RESET}")
    print(">>> Install trunk - https://trunkrs.dev/")
    print("")
    exit()

# build params (to be combined later)
features = '"'
generic = " --filehash false"
default_build = features + generic

parser = argparse.ArgumentParser(
    prog="Keyternity Web Builder",
    description="Build script for Keyternity's web (WASM) version.",
    epilog="Default build command:\ntrunk build " + default_build,
)

# yapf: disable
parser.add_argument("-s",   "--serve",   action="store_true", help="automatically run localhost:8000 after build",)
parser.add_argument("-r",   "--release", action="store_true", help="build for release/production")
parser.add_argument("-n",   "--nightly",    action="store_true", help="build for nightly/production")
parser.add_argument("-m",   "--mobile",  action="store_true", help="build for mobile")
parser.add_argument("-d",   "--debug",   action="store_true", help="build with debug flag. Ignored if --release is present",)
parser.add_argument("-u",   "--baseurl", action="store", help="Sets the base url. Overrides url from --release",)
parser.add_argument("-wg",   "--webgpu", action="store", help="Builds with webgpu feature instead of webgl",)

args = parser.parse_args()

if args.nightly:
    args.release = True

if not args.baseurl:
    args.baseurl = "\"./\""

if args.release and not args.mobile:
    generic += " --release"
if args.mobile:
    features += " mobile"
if args.debug and not args.release:
    features += " debug"
if args.webgpu:
    features += "webgpu"
else:
    features += "webgl"

features += '"'

build_command = "trunk build --features " + features + generic
build_command += " --public-url="+args.baseurl
print("\nBuild command:\n" + build_command + "\n")

# build /dist via trunk
subprocess.run(build_command, shell=True)

# copy assets over to /dist
shutil.copy("assets/skf_icon.ico", "dist/favicon.ico")
shutil.copy("samples/_skellington.skf", "dist/_skellington.skf")
shutil.copy("samples/_skellina.skf", "dist/_skellina.skf")

# official ffmpeg.wasm (npm @ffmpeg/ffmpeg + @ffmpeg/core), pinned and checked by SHA-256.
# See docs/FFMPEG.md to update.
FFMPEG_WASM = [
    ("https://cdn.jsdelivr.net/npm/@ffmpeg/ffmpeg@0.12.15/dist/umd/ffmpeg.js", "ffmpeg.js",
     "ad4cfe957589995dea03fc8de1fd5e9f5cb4558a7282913172203082a65bbfaa"),
    ("https://cdn.jsdelivr.net/npm/@ffmpeg/ffmpeg@0.12.15/dist/umd/814.ffmpeg.js", "814.ffmpeg.js",
     "976f4174ae7da80c0d4f9523ee6dde3ecbce7dc2ee392b2a5322049abb9b8627"),
    ("https://cdn.jsdelivr.net/npm/@ffmpeg/core@0.12.10/dist/umd/ffmpeg-core.js", "ffmpeg-core.js",
     "b266ab5b952555881dd6310663986994a182acb2b7ff25cf10a25f7a37ac2b21"),
    ("https://cdn.jsdelivr.net/npm/@ffmpeg/core@0.12.10/dist/umd/ffmpeg-core.wasm", "ffmpeg-core.wasm",
     "9f57947a5bd530d8f00c5b3f2cb2a3492faa7e5d823315342d6a8656d0a6b7b7"),
]
cache = os.path.join("target", "ffmpeg-wasm")
os.makedirs(cache, exist_ok=True)
os.makedirs("dist/ffmpeg-wasm", exist_ok=True)
for url, name, sha256 in FFMPEG_WASM:
    path = os.path.join(cache, name)
    if not os.path.exists(path) or hashlib.sha256(open(path, "rb").read()).hexdigest() != sha256:
        print(f">>> Downloading {url}")
        data = urllib.request.urlopen(url).read()
        if hashlib.sha256(data).hexdigest() != sha256:
            print(f">>> {RED}!! {name} doesn't match the pinned SHA-256; not using it !!{RESET}")
            exit(1)
        open(path, "wb").write(data)
    shutil.copy(path, os.path.join("dist/ffmpeg-wasm", name))

if args.serve:
    # automatically serve via python http
    subprocess.run("python3 -m http.server 8000 --directory dist".split())
