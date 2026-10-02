# Packaging manifests

Templates for the third-party package repositories. The files ending in `.in`
contain `@@PLACEHOLDER@@` markers (version, tag, release date, SHA-256 of each
asset); `scripts/gen-packaging.py` renders them from a release tag and the
published assets, and `.github/workflows/distribute.yml` runs that after every
published release and pushes each channel whose token is configured. With no
tokens, the rendered files are only uploaded as a workflow artifact. Setup and
secrets: `docs/RELEASING.md` §6.

```sh
python scripts/gen-packaging.py --tag v0.6.0 --assets <dir with the assets> --out <out dir>
```

| Template | Rendered to |
|---|---|
| `winget/templates/*.yaml.in` | `winget/<version>/*.yaml` (version, installer, en-US and zh-CN locales) |
| `homebrew/ai-usage-sidebar.rb.in` | `homebrew/ai-usage-sidebar.rb` (cask, arm + intel) |
| `scoop/ai-usage-sidebar.json.in` | `scoop/ai-usage-sidebar.json` (experimental: 7-Zip unpack of the NSIS exe) |
| `aur/ai-usage-sidebar-bin/PKGBUILD.in` | `aur/ai-usage-sidebar-bin/PKGBUILD`; `.SRCINFO` comes from `makepkg --printsrcinfo` in CI |

`aur/ai-usage-sidebar/` (build from source) is a hand-maintained recipe and is
not touched by the pipeline.

Release asset names (GitHub turns the spaces of the product name into dots):

| Platform | Asset |
|---|---|
| Debian/Ubuntu (used by the AUR `-bin` package) | `AI.Usage.Sidebar_<version>_amd64.deb` |
| Fedora/openSUSE | `AI.Usage.Sidebar-<version>-1.x86_64.rpm` |
| Portable Linux | `AI.Usage.Sidebar_<version>_amd64.AppImage` |
| macOS Apple Silicon | `AI.Usage.Sidebar_<version>_aarch64.dmg` |
| macOS Intel | `AI.Usage.Sidebar_<version>_x64.dmg` |
| Windows (NSIS, per user) | `AI.Usage.Sidebar_<version>_x64-setup.exe` |
| Windows (MSI, per machine) | `AI.Usage.Sidebar_<version>_x64_en-US.msi` |

Whether the builds are signed depends on the release (`docs/RELEASING.md`
§6.1/§6.2). Do not claim a signature in a store listing unless that release
was built with the signing secrets, and do not paste Gatekeeper/SmartScreen
bypass instructions without telling the user what they do.

---

## AUR

`aur/ai-usage-sidebar-bin/` repackages the release `.deb` (x86_64, no
compiler). Dependencies: `webkit2gtk-4.1`, `libayatana-appindicator`,
`librsvg`, `gtk3`. `aur/ai-usage-sidebar/` builds from source; the two
`conflict`, and `-bin` `provides` the plain name. Manual validation of a
rendered recipe:

```sh
cd <out>/aur/ai-usage-sidebar-bin
makepkg --printsrcinfo > .SRCINFO
namcap PKGBUILD
makepkg -f
```

Known caveat for the source package: `build()` downloads npm and crates.io
dependencies, so a clean-chroot build needs network access.

## Homebrew

A **cask** (a prebuilt `.app`), not a formula, pushed to the tap
`harveyxiacn/homebrew-tap` (`Casks/`). Local check:
`brew audit --cask --new <out>/homebrew/ai-usage-sidebar.rb`. homebrew-cask
proper has rules about unsigned software and little usage history; a personal
tap is the friction-free route.

## winget

Rendered to the layout `microsoft/winget-pkgs` expects
(`manifests/h/harveyxiacn/AIUsageSidebar/<version>/`). Two installers: NSIS
(per user, matching `bundle.windows.nsis.installMode: currentUser`) and MSI
(per machine). Local check: `winget validate --manifest <out>\winget\<version>`.
`PackageIdentifier` (`harveyxiacn.AIUsageSidebar`) must never change.

## Scoop

Experimental and untested: the manifest unpacks the NSIS installer with 7-Zip
(`#/dl.7z`) instead of running it. Test with
`scoop install <out>\scoop\ai-usage-sidebar.json` before enabling the bucket.

## Not covered here

* **Flatpak / Snap**: both sandbox the filesystem and the app has to read
  `~/.claude/` and `~/.codex/`; not attempted.
* **Debian/Fedora repositories**: the release `.deb`/`.rpm` are installed
  directly (see `AGENTS.md` §2); no apt/dnf repository is hosted.
* **In-app updates** for package-manager installs are deliberately disabled:
  the app shows "update available" with a link instead of replacing files the
  package manager owns. See `docs/RELEASING.md`.
