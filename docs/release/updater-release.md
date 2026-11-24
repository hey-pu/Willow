# Willow signed updater release

## One-time setup

1. Generate a Minisign key pair (already done on this machine, keys live outside the repository):
   `pnpm tauri signer generate -w ~/.tauri/willow.key --ci`.
2. Add two repository secrets in GitHub → Settings → Secrets and variables → Actions:
   - `TAURI_SIGNING_PRIVATE_KEY`: the full contents of `~/.tauri/willow.key`
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: omit for the current key, which was generated without a password
3. The public key is embedded in `src-tauri/tauri.conf.json` (`plugins.updater.pubkey`) and ships inside the app.

## Publishing a release

1. Bump the version in `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`.
2. Commit, tag `vX.Y.Z`, and push the tag. `.github/workflows/updater-release.yml` tests the release and builds the Windows x64 NSIS bundle, then uploads the installer, `.sig`, and `latest.json` to a draft GitHub Release.
3. Confirm the release assets contain `latest.json` and the signed `-setup.exe`/`.sig` pair.
4. Verify the manifest version, installer URL, signature, and installer SHA-256 before publishing the draft release.

## Recovery

- The signing key is the only way to publish updates the app will accept. Back up `~/.tauri/willow.key` and its password; losing it means shipping a new public key, which requires every existing installation to update manually once.
- If `latest.json` points to a broken URL, fix by re-running the workflow for the same tag or uploading corrected assets; the updater fetches `releases/latest/download/latest.json` on each check.

## Upgrade path for v0.5.0 users

v0.5.0 has no updater built in. Users must manually download and run the v0.5.1 installer once. From v0.5.1 onward, the app checks `latest.json`, verifies the Minisign signature against the embedded public key, and can download and install updates in-app (Willow exits and restarts during installation).
