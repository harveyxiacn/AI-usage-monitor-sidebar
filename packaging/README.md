# Packaging manifests

Ready-made manifests for the three third-party package repositories people ask
for most. **Nothing here has been submitted anywhere** — they are drafts kept
in the repository so a release can be published without writing them from
scratch, and so reviewers can see exactly what would be submitted.

Every file has a checksum placeholder that must be replaced before submission.
Checksums cannot be filled in here because they are computed from the actual
release assets, which only exist after the release workflow ran.

Release asset names (they contain the product name with spaces replaced by
dots, which is what GitHub does):

| Platform | Asset |
|---|---|
| Debian/Ubuntu | `AI.Usage.Sidebar_<version>_amd64.deb` |
| Fedora/openSUSE | `AI.Usage.Sidebar-<version>-1.x86_64.rpm` |
| Portable Linux | `AI.Usage.Sidebar_<version>_amd64.AppImage` |
| macOS Apple Silicon | `AI.Usage.Sidebar_<version>_aarch64.dmg` |
| macOS Intel | `AI.Usage.Sidebar_<version>_x64.dmg` |
| Windows (NSIS) | `AI.Usage.Sidebar_<version>_x64-setup.exe` |
| Windows (MSI) | `AI.Usage.Sidebar_<version>_x64_en-US.msi` |

All builds are **unsigned and un-notarised**. Say so in every store listing;
do not paste Gatekeeper/SmartScreen bypass instructions without telling the
user what they do.

---

## AUR (`aur/`)

Two packages, deliberately:

| Directory | Package | What it does |
|---|---|---|
| `aur/ai-usage-sidebar-bin/` | `ai-usage-sidebar-bin` | Repackages the release `.deb`. No compiler needed, x86_64 only. |
| `aur/ai-usage-sidebar/` | `ai-usage-sidebar` | Builds from the tagged source, doing what `scripts/install-linux.sh` does. x86_64 + aarch64. |

They `conflict` with each other, and `-bin` `provides` the plain name.

**Filling in and validating**

```sh
cd packaging/aur/ai-usage-sidebar-bin
# bump pkgver first, then:
updpkgsums                       # pacman-contrib; replaces the SKIP checksums
makepkg --printsrcinfo > .SRCINFO
namcap PKGBUILD                  # namcap package (lints the recipe)
makepkg -f                       # optional: build it once
namcap ./*.pkg.tar.zst           # lints the built package
```

The `.SRCINFO` files checked in here were generated with
`makepkg --printsrcinfo` and are therefore correct for the current PKGBUILDs —
but they still carry the `SKIP` checksums, so regenerate them after
`updpkgsums`.

**Submitting** (only when the maintainer decides to): clone
`ssh://aur@aur.archlinux.org/<pkgname>.git`, copy `PKGBUILD` and `.SRCINFO`
into it, commit, push. The AUR only accepts those two files plus any
`.install` script; it does not accept this README.

Known caveat for the source package: `build()` downloads the npm and crates.io
dependencies, so a clean-chroot build (`extra-x86_64-build`) needs network
access or a pre-populated cache. This is common for Node/Rust AUR packages but
worth knowing before a reviewer asks.

---

## Homebrew (`homebrew/ai-usage-sidebar.rb`)

A **cask** (a prebuilt `.app`), not a formula. Both architectures are covered
with `on_arm` / `on_intel` blocks.

**Filling in**

```sh
shasum -a 256 AI.Usage.Sidebar_<version>_aarch64.dmg
shasum -a 256 AI.Usage.Sidebar_<version>_x64.dmg
# paste both into the two `sha256` lines
brew audit --cask --new ./packaging/homebrew/ai-usage-sidebar.rb
brew install --cask ./packaging/homebrew/ai-usage-sidebar.rb   # local test
```

**Submitting**: fork `Homebrew/homebrew-cask`, drop the file in
`Casks/a/ai-usage-sidebar.rb`, open a PR. Note that homebrew-cask has rules
about software with no code signature and little usage history; a personal tap
(`brew tap harveyxiacn/tap`) is the friction-free alternative and needs the
identical file in a `Casks/` directory of that tap repository.

Later releases: `brew bump-cask-pr --version <new> ai-usage-sidebar`.

---

## winget (`winget/<version>/`)

Three manifests, the layout the community repository expects. In
`microsoft/winget-pkgs` they go to
`manifests/h/harveyxiacn/AIUsageSidebar/<version>/`.

Two installers are listed: the NSIS `-setup.exe` (per **user**, matching
`bundle.windows.nsis.installMode: currentUser`) and the MSI (per **machine**,
for managed fleets).

**Filling in and validating**

```powershell
(Get-FileHash .\AI.Usage.Sidebar_<version>_x64-setup.exe -Algorithm SHA256).Hash
(Get-FileHash .\AI.Usage.Sidebar_<version>_x64_en-US.msi -Algorithm SHA256).Hash
# paste both into harveyxiacn.AIUsageSidebar.installer.yaml
winget validate --manifest .\packaging\winget\<version>\
winget install --manifest .\packaging\winget\<version>\   # local test
```

**Submitting**: fork `microsoft/winget-pkgs`, copy the three files into the
path above, open a PR. `wingetcreate` automates all of it:

```powershell
wingetcreate update harveyxiacn.AIUsageSidebar --version <new> `
  --urls <setup-exe-url> <msi-url> --submit
```

`PackageIdentifier` (`harveyxiacn.AIUsageSidebar`) must never change once the
package is accepted — the whole update history hangs off it.

---

## Not covered here

* **Flatpak / Snap** — both sandbox the filesystem, and the app has to read
  `~/.claude/` and `~/.codex/`. That needs `--filesystem=home` style holes
  which defeat the point; not attempted.
* **Debian/Fedora repositories** — the release `.deb`/`.rpm` are installed
  directly (see `AGENTS.md` §2); no apt/dnf repository is hosted.
* **In-app updates** for packages installed from any of these repositories are
  deliberately disabled: the app shows "update available" with a link instead
  of replacing files the package manager owns. See `docs/RELEASING.md`.
