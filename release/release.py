# Script for building a complete release distribution.
#
# The full distribution requires, but is not limited to:
# - main binary (release version)
# - user documentation (built/distributed, not source)
#
# Run from this folder. CI runs it from .github/workflows/release.yml with
# --version set from the pushed tag; see the README's "Releasing" section.

import subprocess
import os
import sys
import glob
import platform
import plistlib
import shutil
import argparse

RED = "\033[31m"
BLUE = "\033[34m"
CYAN = "\033[36m"
RESET = "\033[0m"

# yapf: disable
parser = argparse.ArgumentParser(prog="Keyternity Release Builder", description="Build script for Keyternity release distributions.", formatter_class=argparse.RawTextHelpFormatter)
parser.add_argument("-v", "--verbose", action="store_true", help="Print output of everything")
parser.add_argument("-dmg", "--dmg", action="store_true", help="Attempt to create Mac dmg (requires create-dmg)")
parser.add_argument("-d", "--debug", action="store_true", help="Create debug build")
parser.add_argument("-nd", "--nodocs", action="store_true", help="Skip user docs & dev docs")
parser.add_argument("-up", "--ubuntudeps", action="store_true", help="Install glib2 and gtk3 for Ubuntu (used for CI/CD)")
parser.add_argument("--version", help="Version stamped into the installer and Mac bundle (default: Cargo.toml's)")
args = parser.parse_args()

# `&>` would background the command under sh, so redirect explicitly
stdout = "" if args.verbose else " > /dev/null 2>&1"


def run(cmd):
    """Run a shell command and stop the release if it fails."""
    print(f"{CYAN}$ {cmd}{RESET}", flush=True)
    if subprocess.run(cmd, shell=True).returncode != 0:
        print(f"{RED}>>> Failed: {cmd}{RESET}")
        sys.exit(1)


def cargo_version():
    with open("../Cargo.toml", encoding="utf-8") as f:
        for line in f:
            if line.startswith("version"):
                return line.split("=", 1)[1].strip().strip('"')
    sys.exit(f"{RED}>>> No version in Cargo.toml{RESET}")


version = args.version or cargo_version()
print(f">>> Building Keyternity {version}")

if not args.nodocs:
    shutil.rmtree("skelform_dev_docs", ignore_errors=True)
    shutil.rmtree("skelform_user_docs", ignore_errors=True)
    shutil.rmtree("user_docs", ignore_errors=True)
    shutil.rmtree("dev_docs", ignore_errors=True)
    run("cargo install mdbook@0.5.1")
    run("git clone https://github.com/Retropaint/skelform_dev_docs")
    run("git clone https://github.com/Retropaint/skelform_user_docs")
    run("mdbook build skelform_dev_docs")
    run("mdbook build skelform_user_docs")
    shutil.copytree("skelform_dev_docs/book", "./dev-docs", dirs_exist_ok = True)
    shutil.copytree("skelform_user_docs/book", "./user-docs", dirs_exist_ok = True)

# Require create-dmg on mac
if platform.system() == "Darwin" and args.dmg:
    if not shutil.which("create-dmg"):
        run("brew install create-dmg")

binExt = ".exe" if platform.system() == "Windows" else ""

platform_name = ""
match platform.system():
    case "Windows":
        platform_name = "windows"
    case "Darwin":
        platform_name = "mac"
    case "Linux":
        platform_name = "linux"

dirname = "keyternity_" + platform_name

# create clean release folder
if os.path.exists(dirname):
    shutil.rmtree(dirname)
os.mkdir(dirname)

mode = "--release"
path = "release"
if args.debug:
    mode = ""
    path = "debug"

# download dependencies for Ubuntu
if args.ubuntudeps:
    run("sudo apt-get -y update")
    run("sudo apt-get -y install libglib2.0-dev libgtk-3-dev")

# yapf: disable
run            (f"cargo build {mode}")
shutil.copy    (f"../target/{path}/Keyternity{binExt}", f"./{dirname}")
if not args.nodocs:
    shutil.copytree("./user-docs", f"./{dirname}/user-docs")
    shutil.copytree("./dev-docs",  f"./{dirname}/dev-docs")
shutil.copytree("../assets",      f"./{dirname}/assets")
shutil.copytree("../samples",     f"./{dirname}/samples")

# Platform-specific distribution

def find_iscc():
    """Inno Setup's compiler: on PATH, or in its default install folders."""
    found = shutil.which("ISCC.exe") or shutil.which("iscc")
    if found:
        return found
    roots = [os.environ.get("ProgramFiles(x86)", ""), os.environ.get("ProgramFiles", "")]
    for root in roots:
        matches = sorted(glob.glob(os.path.join(root, "Inno Setup *", "ISCC.exe")))
        if matches:
            return matches[-1]
    sys.exit(f"{RED}>>> Inno Setup (ISCC.exe) not found{RESET}")


def darwin():
    print(">>> Preparing Mac app...")
    bin_path = "./Keyternity.app/Contents/MacOS/"
    if os.path.exists(bin_path):
        shutil.rmtree(bin_path)
    shutil.copytree(dirname, bin_path)

    # stamp the version into the bundle (Finder's Get Info, About)
    plist_path = "./Keyternity.app/Contents/Info.plist"
    with open(plist_path, "rb") as f:
        plist = plistlib.load(f)
    plist["CFBundleShortVersionString"] = version
    plist["CFBundleVersion"] = version
    plist["CFBundlePackageType"] = "APPL"
    with open(plist_path, "wb") as f:
        plistlib.dump(plist, f)

    # sign the app in any way, so the OS doesn't show 'this app is damaged'
    run("codesign --force --deep --sign - Keyternity.app")

    shutil.make_archive("Keyternity.app", "zip", ".", "Keyternity.app")

    if not args.dmg:
        print(f">>> Mac release complete. Please look for {BLUE}Keyternity.app{RESET}.")
        return
    print(">>> Preparing Mac dmg...")
    run("./create-dmg.sh" + stdout)
    print(f">>> Mac release complete. Please look for {BLUE}Keyternity.dmg{RESET}.")

match platform.system():
    case "Windows":
        shutil.copy(f"../target/{path}/Keyternity.pdb", f"./{dirname}")
        shutil.make_archive(dirname, 'zip', ".", dirname)

        # create installer (Inno Setup)
        run(f'"{find_iscc()}" /DMyAppVersion={version} install.iss')
    case "Darwin":
        darwin()
    case "Linux":
        shutil.make_archive(dirname, 'zip', ".", dirname)
