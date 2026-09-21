# Releasing AI Usage Sidebar

Everything a release needs: the version checklist, the updater signing key
(including what happens when it does not exist), and the CI facts that bite.

Audience: the maintainer. Contributors only need §1.

---

## 1. Version bump checklist

Four files carry the version and **all four must agree**, because the build
runs with `--locked` and a `Cargo.lock` that still holds the old version fails
the build instead of quietly updating itself.

| File | Field |
|---|---|
| `package.json` | `"version"` |
| `src-tauri/tauri.conf.json` | `"version"` (this is what the app reports and what `latest.json` compares against) |
| `src-tauri/Cargo.toml` | `[package] version` |
| `src-tauri/Cargo.lock` | the `[[package]] name = "ai-usage-sidebar"` entry |

```sh
NEW=0.2.0
sed -i "s/\"version\": \"[0-9].*\"/\"version\": \"$NEW\"/" package.json
sed -i "s/\"version\": \"[0-9].*\"/\"version\": \"$NEW\"/" src-tauri/tauri.conf.json
sed -i "0,/^version = \".*\"/s//version = \"$NEW\"/" src-tauri/Cargo.toml
(cd src-tauri && cargo update --workspace --offline)   # rewrites Cargo.lock only
git diff --stat            # exactly those four files
```

Then:

```sh
pnpm check && pnpm test && (cd src-tauri && cargo fmt --all -- --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked)
git commit -am "Bump version to $NEW"
git tag "v$NEW"
git push origin main "v$NEW"
```

Pushing the tag starts `.github/workflows/release.yml`. It builds four
matrix jobs (macOS arm64, macOS x64, ubuntu-24.04, windows) and creates a
**draft** release. Check the assets, edit the notes, then publish it.

`workflow_dispatch` can re-run the workflow for an existing tag, which is the
way to retry a single failed platform.

> The updater endpoint is `releases/latest/download/latest.json`. A **draft**
> release is not "latest", so no user is offered the update until the draft is
> published. That is the intended safety net — nothing else has to be gated.

---

## 2. The updater signing key

### 2.1 What is already in the repository

* `src-tauri/tauri.conf.json` has `bundle.createUpdaterArtifacts: true`, the
  `plugins.updater.endpoints` entry and the **public** key
  (`plugins.updater.pubkey`). A public key is public by design; it is in git
  on purpose so every build verifies signatures against the same key.
* The **private** key is not here and must never be. It lives in the
  maintainer's `~/.tauri/` and, for CI, in a GitHub Actions secret.

### 2.2 Generating a key (once, on the maintainer's machine)

```sh
pnpm tauri signer generate -w ~/.tauri/ai-usage-sidebar.key
```

It prints the public key and writes two files:

* `~/.tauri/ai-usage-sidebar.key` — the private key. Back it up somewhere
  safe. **Losing it means no existing installation can ever be updated
  in place again**: the pubkey baked into shipped binaries would no longer
  match, and users would have to reinstall by hand.
* `~/.tauri/ai-usage-sidebar.key.pub` — the public key, which belongs in
  `plugins.updater.pubkey` (base64, one line).

If you generate a *new* key, the `pubkey` in `tauri.conf.json` must be
replaced in the same commit, and every user still running an older build has
to reinstall manually.

### 2.3 The GitHub secrets

Repository → Settings → Secrets and variables → Actions → *New repository
secret*:

| Secret | Value |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | the whole content of `~/.tauri/ai-usage-sidebar.key` (or the key string itself) |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | the password chosen during `signer generate`; **create it even if empty** if you set one |

Nothing else needs to change: `release.yml` already passes both to
`tauri-action`.

### 2.4 What happens when the secret does **not** exist

This is the current state, and it is fully supported.

`tauri build` refuses to run when `createUpdaterArtifacts` is on, a `pubkey`
is configured and `TAURI_SIGNING_PRIVATE_KEY` is missing:

```
A public key has been found, but no private key.
Make sure to set `TAURI_SIGNING_PRIVATE_KEY` environment variable.
```

So `release.yml` has a step, *Decide whether this release carries updater
artifacts*, that runs before the build:

* **Secret present** → normal build. `.AppImage.tar.gz`, `.app.tar.gz` and the
  Windows installer get `.sig` files, and `tauri-action` uploads `latest.json`
  (each of the four matrix jobs merges its platforms into the file already
  attached to the release, so one `latest.json` ends up describing all of
  them).
* **Secret absent** → the build gets
  `--config src-tauri/updater-disabled.conf.json`, which merges
  `bundle.createUpdaterArtifacts: false` over `tauri.conf.json`;
  `uploadUpdaterJson` is set to `false`; and the run prints a **warning
  annotation** at the top of the summary page. The release is otherwise
  completely normal — same `.deb`, `.rpm`, `.AppImage`, `.dmg`, `.exe`,
  `.msi`, same names.

The in-app consequence of "secret absent": every installed copy that checks
for updates gets an HTTP 404 from the endpoint, records that as an error in
its update status and shows nothing to the user beyond "Check for updates"
still being available. Nobody is pestered and nothing breaks; the app simply
never offers an in-place update until a release carries `latest.json`.

`ci.yml` always passes the same `--config` override: pull requests from forks
never see secrets, and CI has no reason to produce signed updater artifacts.

---

## 3. How the in-app updater behaves

Implementation: `src-tauri/src/updater.rs`, `src/lib/stores/update.svelte.ts`.

* Setting `autoUpdateCheck` (default **on**) controls the automatic check
  only. It runs 30 s after start-up and then once every 24 h. Turning it off
  never removes the manual "Check for updates" action.
* A check reads `latest.json`. It **never** downloads or installs anything.
* A found version is surfaced in two unobtrusive places: the tray menu item
  becomes "Update x.y.z available…", and the dashboard gets a one-line banner
  above the tabs (dismissable) plus an "Updates" row in Settings → About with
  the release notes link.
* Installing is always an explicit click, and it restarts the app.
* **Which installs may replace themselves**: AppImage, macOS `.app`, Windows
  NSIS/MSI. A `.deb`/`.rpm` belongs to the package manager and
  `scripts/install-linux.sh` owns `~/.local/bin/ai-usage-sidebar`; for those,
  `UpdateStatus.canInstall` is `false` and the UI offers the release page
  instead of an install button. The detection is the `APPIMAGE` environment
  variable, which only an AppImage runtime sets.

---

## 4. CI facts worth knowing

### Docs-only changes

`ci.yml` has `paths-ignore` for `**.md`, `docs/**`, `LICENSE` and
`packaging/**`, on both `push` and `pull_request`. A README typo therefore
does not spend 45 minutes on three runners.

**The catch**: `paths-ignore` makes the workflow *not run at all*, so its
checks never report — they are not "skipped", they are absent. If the `build`
or `smoke` checks are ever marked **required** in branch protection, a
docs-only PR will hang forever waiting for a check that will never arrive.
Two ways out, pick one before enabling branch protection:

1. Do not mark them required (rely on the PR review instead), or
2. add a tiny always-runs job with the same name that exits 0 when only docs
   changed (the "dummy required job" pattern), and make *that* the required
   check.

### The Linux start-up smoke test

`scripts/smoke-linux.sh` launches the real binary under `xvfb-run` +
`dbus-run-session`, with `WEBKIT_DISABLE_DMABUF_RENDERER=1` and a throw-away
`XDG_*` home, and waits (default 60 s) for `sidebar revealed` to appear in
`~/.local/share/io.github.harveyxiacn.ai-usage-sidebar/logs/`. It then kills
the process group and exits.

This catches what nothing else can: a panic in `setup()`, a missing runtime
library in the bundle, a window that is created but never shown. The CI job
reuses the binary the Linux build job already produced instead of rebuilding.

Run it locally with `scripts/smoke-linux.sh` after `pnpm tauri build`
(it needs the `xvfb` and `dbus-x11` packages; without `xvfb-run` it refuses
to start rather than opening a window on your desktop).

### `native-smoke`

The `native-smoke` cargo feature dlopens `libgtk-layer-shell.so.0` and checks
that every symbol the Wayland docking path calls resolves. It runs on the
Ubuntu leg only, after `apt-get install libgtk-layer-shell0`:

```sh
cargo test --locked --features native-smoke 'smoke::'
```

It fails on machines without the library — that is the point, and why it is
not part of the default test run.

---

## 5. Third-party packages

`packaging/` holds draft AUR, Homebrew and winget manifests with checksum
placeholders. Nothing is submitted automatically; see `packaging/README.md`
for how to fill them in, validate them and (if the maintainer wants to)
submit them.
