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

Before the bump, if this release changed the database schema or the settings
shape, the upgrade fixtures for the version being replaced must exist (see
"When the schema or the settings change" in [VALIDATION.md](VALIDATION.md)):

- [ ] new `src-tauri/tests/fixtures/db/v<previous>.sql` registered in `upgrade_tests/db.rs`
- [ ] new `src-tauri/tests/fixtures/settings/v<previous>-*.json` registered in `upgrade_tests/settings_files.rs`
- [ ] `cargo test --locked upgrade_tests` passes

Then:

```sh
pnpm check && pnpm check:i18n && pnpm check:agents && pnpm test && (cd src-tauri && cargo fmt --all -- --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked)
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

### 1.0 Schema changes (when `usage.db` changes shape)

The compatibility policy is in docs/ARCHITECTURE.md §8.1. For every release
that touches the schema:

1. Append a `Migration` to `MIGRATIONS` in `src-tauri/src/store/mod.rs`
   (`to: SCHEMA_VERSION + 1`, idempotent apply fn) and bump `SCHEMA_VERSION`.
   The pre-migration backup, the version stamps and the "newer database"
   handling come for free.
2. Decide `min_reader`. Additive (nullable/defaulted column, new table, new
   index): keep the previous value, so the previous release still opens the
   database. Breaking (drop, rename, changed meaning, rewritten rows): set it to
   the new version. When unsure, ask whether the previous release, left running
   against the new file, could misread or corrupt anything. Remember that
   releases older than v0.7 refuse any newer `schema_version` regardless.
3. Add the upgrade fixture: a database exactly as the previous release left it,
   migrated to the new schema by a test (see the WP1 upgrade tests), including
   a check that the pre-upgrade backup appears and holds the old schema.
4. Update the schema in ARCHITECTURE.md §8 and the release notes: say whether
   the release can be rolled back (`min_reader` unchanged) and mention
   `ai-usage-sidebar --restore-pre-upgrade` for anyone who needs the old data
   shape back.
5. Windows/macOS/Linux smoke: the `v0.5.0` upgrade legs must log the migration
   (`usage.db schema 2 -> N`, asserted through `SMOKE_EXPECT_MIGRATION`). The
   smoke scripts do not look at `<data dir>/backups/`; that the backup is
   written is covered by the Rust tests (step 3).

### 1.1 Never name the integration branch like the tag

`v0.2.0` was developed on a branch called `v0.2.0`. After the merge,
`git push origin v0.2.0` was refused ("cannot push some refs") because the name
matched both the branch and the new tag, and the release workflow checks out
`github.ref_name` by name, which would have been just as ambiguous. Use a
branch name such as `release/0.2.0`; if it happens anyway, delete the merged
branch (`git push origin --delete refs/heads/<name>`) and push the tag
explicitly (`git push origin refs/tags/<name>`).

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

This case is fully supported. The repository currently has the signing secret;
the maintainer should confirm it is still available before tagging a release.

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

## 3.1 Price table updates

The repository's `pricing.json` is the default published price source at its
GitHub raw URL. Keep it in sync with the bundled defaults when changing model
prices, and validate both copies before publishing. A user can select a
different `https://` source with `pricingUrl`; an empty value means the
project's raw file, not "disabled".

`autoPricingCheck` is independent of `autoUpdateCheck`. When enabled (the
default), the app checks the selected price source about 60 seconds after
startup and then every 24 hours. The background check is read-only: it only
records that a newer source table is available and reminds the user. It does
not replace the applied table, overwrite a saved custom table or reinstall the
application.

The user applies a source update through **Check pricing updates**, followed by
**Apply price update**. A saved table edited in Settings remains authoritative
until that action is confirmed; **Use source pricing** explicitly switches back
to source prices, confirms the change and backs up the saved table first.
The source download is size-limited and schema-validated, and a failed check
or failed apply keeps the current table. The backend contracts are
`get_price_update_status`, `check_for_price_updates`, `apply_price_update` and
`use_source_pricing`, with the `price-update-status` event. `refresh_pricing`
remains as a compatibility command that performs the legacy explicit
check-and-apply flow.

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

### Windows / macOS start-up smoke tests and upgrade smoke tests

`ci.yml` has a start-up smoke job per OS (`smoke`, `smoke-windows`,
`smoke-macos`), each consuming the binary its build job uploaded (`linux-binary`,
`windows-exe`, `macos-app`; one-day retention). Details of what they assert
are in `docs/VALIDATION.md`.

The `upgrade` matrix (3 OS x `v0.5.0` / latest release) runs only on pull
requests and `workflow_dispatch`, not on pushes to `main`, and, like everything
in `ci.yml`, not for docs-only changes. It downloads the previous release with
`gh release download` (default `GITHUB_TOKEN`, public assets), so:

* **Before a release PR**, expect the `latest` legs to test the *previous*
  version; after you publish, the next PR automatically tests the new one.
  The `latest` leg never asserts a migration line, even when one happens (while
  v0.6.0 is the latest release, schema 3 is migrated to 4 on that leg).
* If an asset naming scheme changes (`*_amd64.AppImage`, `*_aarch64.dmg` /
  `*_x64.dmg`, `*_x64-setup.exe`), update the patterns in `ci.yml`.
* When `SCHEMA_VERSION` changes again, add a matrix leg for the newest release
  that still holds the old schema and set `SMOKE_EXPECT_MIGRATION` for it.
* A failed leg uploads `upgrade-logs-<os>-<tag>`; the Windows/macOS scripts also
  print the whole log in the job output.

These jobs add roughly 3-6 minutes of wall time on the slowest runner, run in
parallel with each other and only after `build`.

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

`packaging/` holds the manifest **templates** (`*.in`); `scripts/gen-packaging.py`
renders them from a release tag and the published assets, and
`.github/workflows/distribute.yml` does that automatically after a release is
published. See §6 and `packaging/README.md`.

---

## 6. Signing & distribution

Everything here is **opt-in and secret-gated**: with none of the secrets or
variables below, `release.yml` produces exactly the unsigned release it always
has and `distribute.yml` only uploads the rendered manifests as a workflow
artifact. The updater signature (§2, minisign) is separate and unaffected by
any of this.

Secrets: Settings → Secrets and variables → Actions → *Secrets*. Variables
(non-secret): the *Variables* tab.

### 6.1 Windows (Authenticode)

`release.yml` step *Windows code signing setup* picks one method, in this order:

| Method | Needs | What the step does |
|---|---|---|
| (a) PFX certificate | secrets `WINDOWS_CERTIFICATE` (the .pfx, base64: `[Convert]::ToBase64String([IO.File]::ReadAllBytes('cert.pfx'))`) and `WINDOWS_CERTIFICATE_PASSWORD`; optional variable `WINDOWS_TIMESTAMP_URL` (default `http://timestamp.digicert.com`) | imports the PFX into the runner's `CurrentUser\My`, writes `src-tauri/windows-signing.conf.json` with `bundle.windows.certificateThumbprint` / `digestAlgorithm: sha256` / `timestampUrl`, and passes it as an extra `--config`. The NSIS installer, the MSI and the app exe are signed. |
| (b) Azure Trusted Signing (only if (a) is absent) | secrets `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`; variables `AZURE_SIGNING_ENDPOINT` (e.g. `https://wus2.codesigning.azure.net`), `AZURE_SIGNING_ACCOUNT`, `AZURE_SIGNING_PROFILE` | `cargo install artifact-signing-cli` and a `bundle.windows.signCommand` of `artifact-signing-cli -e ... -a ... -c ... -d AIUsageSidebar %1` in the same generated override |
| neither | - | no override; unsigned build, SmartScreen warns |

The generated file is never committed. If `cargo install artifact-signing-cli`
cannot find the crate (the tool was renamed from `trusted-signing-cli`; the
flags are identical), change the crate name in the step. An OV certificate
still builds SmartScreen reputation over time; EV and Trusted Signing remove
the warning much sooner.

### 6.2 macOS (Developer ID signing and notarization)

Step *macOS code signing setup* exports the Apple variables through
`GITHUB_ENV` only when complete (an *empty* `APPLE_CERTIFICATE` makes the Tauri
CLI try to import it and fail), then `tauri-action` imports the certificate
and signs/notarizes by itself.

| Stage | Secrets |
|---|---|
| Signing (all three required) | `APPLE_CERTIFICATE` (Developer ID Application .p12, base64), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY` (e.g. `Developer ID Application: Name (TEAMID)`) |
| Notarization, option 1 (Apple ID) | `APPLE_ID`, `APPLE_PASSWORD` (an app-specific password), `APPLE_TEAM_ID` |
| Notarization, option 2 (API key) | `APPLE_API_ISSUER`, `APPLE_API_KEY` (key id), `APPLE_API_PRIVATE_KEY` (the .p8 file's text; the step writes it to a temp file and sets `APPLE_API_KEY_PATH`) |

Signing set but no notarization credentials: the build is signed, not
notarized, and a warning annotation says so. No secrets: unchanged, as before.

### 6.3 Package managers (`distribute.yml`)

Runs on `release: published` (non-pre-releases) and via *Run workflow* with a
`tag` input (use that to retry or to backfill a tag). It downloads the
published assets, renders the templates with `scripts/gen-packaging.py`, and
uploads them as the artifact `packaging-<tag>` (always). Then, per channel:

| Channel | Gate | Action |
|---|---|---|
| AUR `ai-usage-sidebar-bin` | secret `AUR_SSH_PRIVATE_KEY` (private key whose public half is on the AUR account) | `.SRCINFO` is always generated in an Arch container (`makepkg --printsrcinfo`, artifact `aur-<tag>`); with the key it is pushed to `ssh://aur@aur.archlinux.org/ai-usage-sidebar-bin.git` |
| winget | secret `WINGET_TOKEN` (classic PAT, `public_repo`) | `wingetcreate submit` opens the PR against `microsoft/winget-pkgs` from the token owner's fork; manifests: version, installer (NSIS user + MSI machine), en-US and zh-CN locales |
| Homebrew cask | secret `HOMEBREW_TAP_TOKEN` (write access to the tap); optional variable `HOMEBREW_TAP_REPO` (default `harveyxiacn/homebrew-tap`) | commits `Casks/ai-usage-sidebar.rb` to the tap |
| Scoop | secret `SCOOP_BUCKET_TOKEN`; optional variable `SCOOP_BUCKET_REPO` (default `harveyxiacn/scoop-bucket`) | commits `bucket/ai-usage-sidebar.json`. The manifest unpacks the NSIS exe with 7-Zip and is **experimental and untested** |

Local run (e.g. to check a template change):

```sh
gh release download v0.6.0 --repo harveyxiacn/AI-usage-monitor-sidebar --dir /tmp/a -p '*.deb' -p '*.dmg' -p '*-setup.exe' -p '*.msi'
python scripts/gen-packaging.py --tag v0.6.0 --assets /tmp/a --out /tmp/out
```

The first winget submission is reviewed by humans (winget moderators); the
`PackageIdentifier` `harveyxiacn.AIUsageSidebar` must never change afterwards.
The tap and bucket repositories are created by the maintainer; the workflow
never creates one.

### 6.4 Turn-on checklist

1. **Windows**: get a code-signing certificate (PFX) or an Azure Trusted
   Signing account + certificate profile. Add the §6.1 secrets (and variables
   for Azure). Tag a release and confirm in the log: *Windows: signing with ...*;
   on a Windows machine check *Properties → Digital Signatures* of the
   `-setup.exe`.
2. **macOS**: enrol in the Apple Developer Program, create a *Developer ID
   Application* certificate, export it as .p12 and base64 it. Add the §6.2
   secrets. After the release, run `spctl -a -vv -t install` on the .dmg and
   `xcrun stapler validate` on the app.
3. **Homebrew**: create the repo `harveyxiacn/homebrew-tap`, create a
   fine-grained token with *Contents: write* on it, store it as
   `HOMEBREW_TAP_TOKEN`.
4. **Scoop**: create `harveyxiacn/scoop-bucket` (with a `bucket/` directory),
   token as `SCOOP_BUCKET_TOKEN`.
5. **AUR**: register at aur.archlinux.org, add an SSH public key, store the
   private key as `AUR_SSH_PRIVATE_KEY`.
6. **winget**: create a classic PAT with `public_repo` as `WINGET_TOKEN`.
7. Publish the next (draft) release, or run *Distribute* manually with that
   tag. Each channel with a token pushes; the others leave the artifact.
8. Once a channel is live, drop the "once published" wording for it in
   README.md; drop the SmartScreen/Gatekeeper note once both signing paths
   work.

### 6.5 Sources

* Tauri v2, Windows code signing (`certificateThumbprint`, `digestAlgorithm`,
  `timestampUrl`, `signCommand`, Azure signing CLI and its three `AZURE_*`
  variables): <https://v2.tauri.app/distribute/sign/windows/>
* Tauri v2, macOS code signing (`APPLE_CERTIFICATE`,
  `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`,
  `APPLE_PASSWORD`, `APPLE_TEAM_ID`, `APPLE_API_ISSUER`, `APPLE_API_KEY`,
  `APPLE_API_KEY_PATH`): <https://v2.tauri.app/distribute/sign/macos/>
* wingetcreate: <https://github.com/microsoft/winget-create>
