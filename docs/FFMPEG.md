# FFmpeg

Video export (MP4/GIF) runs FFmpeg. The repo contains **no FFmpeg binaries**. Each
platform gets FFmpeg from its official publisher, pinned to a specific build and
checked by SHA-256 before use.

| platform | where FFmpeg comes from | verified by |
|---|---|---|
| Windows (installer) | Optional installer task "Download FFmpeg for video export" (on by default). Downloads gyan.dev's essentials build (`.7z`, ~33 MB) and installs `ffmpeg.exe` next to the app | Inno Setup's download check (`release/install.iss`) |
| Windows (portable / any) | Export › Video › **Download ffmpeg** fetches the same build (`.zip`, ~108 MB) and writes `ffmpeg.exe` next to the app. "Use system ffmpeg" uses the one in `PATH` instead | `src/export_modal.rs` (`fetch_ffmpeg`) |
| Linux | The system's ffmpeg, from the package manager | n/a |
| macOS | The system's ffmpeg, e.g. `brew install ffmpeg`. Homebrew and MacPorts locations are checked, because apps opened from Finder don't get the shell's `PATH` (`utils::system_ffmpeg`) | n/a |
| Web | Official ffmpeg.wasm from npm (`@ffmpeg/ffmpeg`, `@ffmpeg/core`) via jsDelivr, fetched by `web_build.py` into `dist/ffmpeg-wasm/` (cached in `target/ffmpeg-wasm/`) | `web_build.py` |

gyan.dev is one of the two Windows builders linked from
[ffmpeg.org/download.html](https://ffmpeg.org/download.html). Its builds are mirrored
as GitHub releases at [GyanD/codexffmpeg](https://github.com/GyanD/codexffmpeg/releases),
which keeps old versions available, so pinned URLs stay valid.

## Pinned versions

### Windows: gyan.dev `2026-02-09-git-9bfa1635ae`, essentials build

| asset | SHA-256 | used by |
|---|---|---|
| `ffmpeg-2026-02-09-git-9bfa1635ae-essentials_build.7z` | `e3c8fca4c28011b46e7ff8e215c034c1d7000a6726d055c98d4d3f9c4d0e4ff9` | installer |
| `ffmpeg-2026-02-09-git-9bfa1635ae-essentials_build.zip` | `170c57f56e116416ff09494f77bcee6cb4d6be34cc9c1170cbaa923b296616bf` | in-app download |
| `bin/ffmpeg.exe` inside either | `7f1ee3c1abf1d6b18e62d0006002ed26204e68f850429ae633e4dd530eaa1014` | reference |

### Web: ffmpeg.wasm

| file | package | SHA-256 |
|---|---|---|
| `ffmpeg.js` | `@ffmpeg/ffmpeg@0.12.15` `dist/umd/` | `ad4cfe957589995dea03fc8de1fd5e9f5cb4558a7282913172203082a65bbfaa` |
| `814.ffmpeg.js` | `@ffmpeg/ffmpeg@0.12.15` `dist/umd/` | `976f4174ae7da80c0d4f9523ee6dde3ecbce7dc2ee392b2a5322049abb9b8627` |
| `ffmpeg-core.js` | `@ffmpeg/core@0.12.10` `dist/umd/` | `b266ab5b952555881dd6310663986994a182acb2b7ff25cf10a25f7a37ac2b21` |
| `ffmpeg-core.wasm` | `@ffmpeg/core@0.12.10` `dist/umd/` | `9f57947a5bd530d8f00c5b3f2cb2a3492faa7e5d823315342d6a8656d0a6b7b7` |

## Updating

**Windows**
1. Pick a release at [GyanD/codexffmpeg](https://github.com/GyanD/codexffmpeg/releases).
   Each asset's SHA-256 is shown on the release page, and by
   `gh release view <tag> --repo GyanD/codexffmpeg --json assets`.
2. Update the URL and hash of the essentials `.7z` in `release/install.iss`. Also
   update the folder name (`FFmpegBuild`).
3. Update the URL, hash and size of the essentials `.zip` in `src/export_modal.rs`.
4. Update the table above.

**Web:** bump the versions in `web_build.py`, take the new hashes from the npm
packages (e.g. `curl -sL <jsdelivr url> | sha256sum`), and update the table above.

## History

Earlier, SkelForm kept FFmpeg binaries in the repo (`ffmpeg/`, partly in Git LFS).
When Keyternity forked, they were checked and then removed from the repo and its
history:
- **Windows `ffmpeg.exe`:** byte-identical to the gyan.dev build above.
- **Linux:** a dynamically linked distro binary. It only ran where the same
  `libav*` libraries were already installed.
- **macOS:** a copy out of a Homebrew install
  (`/opt/homebrew/Cellar/ffmpeg/8.0.1_2`). It only ran with that Homebrew FFmpeg
  installed.
- **Web:** identical to the npm files above, apart from one trailing newline.

## Licensing

These FFmpeg builds are GPL.
- **Desktop:** Keyternity doesn't redistribute them. The installer and the app
  fetch them from the publisher, whose release pages link the matching source.
- **Web:** the build output (`dist/ffmpeg-wasm/`) does serve ffmpeg.wasm. A hosted
  web build should link its source:
  [ffmpegwasm/ffmpeg.wasm](https://github.com/ffmpegwasm/ffmpeg.wasm) at the
  versions above.
