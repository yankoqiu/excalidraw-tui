# Releasing excalidraw-tui

Releases publish six npm packages at the same version:

- `excalidraw-tui` — Node launcher, bundled renderer, README, license
- `excalidraw-tui-linux-x64`
- `excalidraw-tui-linux-arm64`
- `excalidraw-tui-darwin-x64`
- `excalidraw-tui-darwin-arm64`
- `excalidraw-tui-win32-x64`

The version comes from `[package].version` in `Cargo.toml`. The tag and all six
packages must use that exact version.

## Prerequisites

- A public GitHub repository matching `Cargo.toml` `repository`; npm provenance
  requires a public source repository.
- An npm account or organization that controls all six package names. The
  platform packages are resolved by the main package's optional dependencies.
- GitHub Actions enabled, with an environment named `npm` in **Settings →
  Environments**. The publish job targets this environment, and trusted
  publishing is configured with the same environment name. Add approvals or
  environment-scoped secrets as needed.
- Trusted publishing configured on npmjs.com for all six packages: provider
  GitHub Actions, workflow filename `release.yml`, environment `npm`, and
  **Allow npm publish** checked (configurations created after Sep 3, 2026 only
  allow `npm stage publish` by default). The publish job intentionally does not
  accept an `NPM_TOKEN` secret; it fails fast if the OIDC token endpoint is
  unavailable so misconfiguration is not masked by a fallback token.
- Node.js 22.22.2+ locally for packaging. CI uses Node 24.

The build matrix is:

| Runner | Rust target | npm package |
| --- | --- | --- |
| `ubuntu-22.04` | `x86_64-unknown-linux-musl` | `excalidraw-tui-linux-x64` |
| `ubuntu-24.04-arm` | `aarch64-unknown-linux-musl` | `excalidraw-tui-linux-arm64` |
| `macos-15-intel` | `x86_64-apple-darwin` | `excalidraw-tui-darwin-x64` |
| `macos-15` | `aarch64-apple-darwin` | `excalidraw-tui-darwin-arm64` |
| `windows-latest` | `x86_64-pc-windows-msvc` | `excalidraw-tui-win32-x64` |

Linux binaries are checked for dynamic dependencies before packaging.

## First publication of a brand-new package

Trusted publishing can only be configured after a package exists on npmjs.com,
and the release workflow publishes exclusively through OIDC. A brand-new package
name therefore needs one manual publish first:

1. Confirm the six names are still available:

   ```sh
   npm view excalidraw-tui version
   npm view excalidraw-tui-linux-x64 version
   ```

   A 404 means the name is available.

2. Build the artifacts (see [Dry run](#dry-run)) and publish from a workstation
   with an npm token that has publish permission, in the order listed under
   [Manual publishing](#manual-publishing).
3. Configure trusted publishing for all six packages (see
   [Trusted publishing](#trusted-publishing)), then use tag releases for every
   subsequent version.
4. Verify the result:

   ```sh
   npm view excalidraw-tui@0.1.0 version
   npx --yes excalidraw-tui@0.1.0 --help
   ```

For tag releases, the workflow validates the tag, builds every target, assembles
npm tarballs, uploads them as the `npm-tarballs` artifact, publishes the five
platform packages first, and publishes `excalidraw-tui` last. The publish step
skips package versions that already exist, so a failed release can be retried
after the underlying problem is fixed.

## Trusted publishing

Publishing runs through GitHub OIDC; no npm token secret is used. Configure this
for each of the six packages after the first manual publish:

1. Open the package settings on npmjs.com.
2. Add a Trusted Publisher:
   - Provider: GitHub Actions
   - Organization or user: `yankoqiu`
   - Repository: `excalidraw-tui`
   - Workflow filename: `release.yml`
   - Environment: `npm`
   - Allowed actions: check **Allow npm publish**
3. Repeat for every platform package and the main package.
4. Delete any leftover `NPM_TOKEN` repository secret; it is ignored by the
   workflow.
5. The workflow installs npm `^11.5.1`, which is required for trusted
   publishing.

The publish job declares `environment: npm`, so every trusted publisher
configuration must use the environment name `npm` exactly; a blank or different
environment fails the OIDC check. npm stores only one trusted publisher per
package, so correcting a field means revoking and recreating the configuration.

With npm 11.15+ you can inspect and recreate configurations from a local
terminal (requires `npm login` with 2FA):

```sh
npm trust list excalidraw-tui --json
npm trust revoke excalidraw-tui --id <id>
npm trust github excalidraw-tui \
  --repo yankoqiu/excalidraw-tui --file release.yml --env npm \
  --allow-publish --yes
```

Repeat for all six packages; npm offers a 5-minute 2FA skip window for bulk
commands, and a 2-second sleep between calls avoids rate limiting.

### Diagnosing publish failures

Trusted publishing failures surface as generic errors, often `ENEEDAUTH`, even
when the real cause is a field mismatch. When a publish fails:

- Check the failing run's log for the step output
  `Publishing with npm trusted publishing (OIDC)`. If it is missing, the OIDC
  endpoint guard failed or npm is not running with trusted publishing support.
- Re-check on npmjs.com for every package: workflow filename must be exactly
  `release.yml` (case-sensitive, with extension), environment must be `npm`,
  and **Allow npm publish** must be checked. npm does not validate these when
  the configuration is saved.
- The `repository.url` in each generated `package.json` must resolve to
  `github.com/yankoqiu/excalidraw-tui`.
- Use GitHub-hosted runners only; self-hosted runners are not supported.
- Verify the npm CLI is `>= 11.5.1`; the workflow installs the latest 11.x.

After fixing configuration, re-run the failed publish job or start the workflow
manually with `publish` enabled. No new tag is needed while versions remain
unpublished; the publish helper skips any package version that already exists.

## Dry run

Run the Release workflow manually with `publish` disabled:

1. Open **Actions → Release → Run workflow**.
2. Leave **Publish packages to npm** unchecked.
3. Download the `npm-tarballs` artifact from the run.

This builds all five platforms and validates package contents without publishing.
The tag/version check is skipped for manual runs, so make sure `Cargo.toml` is on
the version you intend to release.

Local packaging:

```sh
npm ci --prefix renderer
npm run bundle --prefix renderer
cargo build --release --locked
node scripts/prepare-npm.mjs --local --pack
```

The script uses `npm pack --dry-run` to reject unexpected files such as
`node_modules`, build output, repository data, or tarballs. It prints the publish
order and, for `--local`, an install command for the two local tarballs.

Test a local installation without touching your global installation:

```sh
tmpdir="$(mktemp -d)"
cd "$tmpdir"
npm init -y >/dev/null
npm install \
  /absolute/path/to/dist/npm/tarballs/excalidraw-tui-win32-x64-0.1.0.tgz \
  /absolute/path/to/dist/npm/tarballs/excalidraw-tui-0.1.0.tgz
./node_modules/.bin/exv --help
```

Use the platform tarball matching your machine; on Windows run
`node_modules\.bin\exv.cmd`. The main tarball alone is not enough for a local
test because its platform package is not on the registry yet.

## Manual publishing

Prefer the GitHub workflow. If CI publishing is unavailable, download the
`npm-tarballs` artifact and publish in this order:

```sh
npm publish excalidraw-tui-linux-x64-0.1.0.tgz --access public
npm publish excalidraw-tui-linux-arm64-0.1.0.tgz --access public
npm publish excalidraw-tui-darwin-x64-0.1.0.tgz --access public
npm publish excalidraw-tui-darwin-arm64-0.1.0.tgz --access public
npm publish excalidraw-tui-win32-x64-0.1.0.tgz --access public
npm publish excalidraw-tui-0.1.0.tgz --access public
```

Manual publishing from a workstation cannot attach npm provenance; publishing
from GitHub Actions with `--provenance` is preferred. Never publish the main
package before all five platform packages exist.

## Rollback and failed releases

npm versions are immutable. If a release is wrong:

- Deprecate the bad version and publish a patch:

  ```sh
  npm deprecate excalidraw-tui@0.1.0 "broken release; use 0.1.1"
  ```

- Do not unpublish only some platform packages. The main package pins exact
  versions, so all six packages must remain available together.
- npm only allows unpublishing within its published policy window and only when
  dependents permit it. Prefer a forward fix.

If the workflow fails after publishing some packages, push a fix or re-run the
same tag. The publish helper skips versions that already exist and continues with
the rest.
