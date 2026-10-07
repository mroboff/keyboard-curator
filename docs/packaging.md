# Packaging for macOS

How Keyboard Curator becomes a `.app` and a `.dmg`, locally and in CI.
The pieces: `ci/bundle-macos.sh` (the packager), `crates/kc-app/Info.plist`
(the bundle manifest), `crates/kc-app/entitlements.plist` (hardened
runtime, deliberately empty), `crates/kc-app/assets/icon/` (the icon's SVG
source and the `.icns` made from it by `ci/make-icon.sh`) and
`.github/workflows/release.yml` (the release build).

## Building a release locally

```sh
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer   # GPUI's Metal shader step needs Xcode
cargo build --release --locked -p kc-app --target aarch64-apple-darwin
ci/bundle-macos.sh aarch64-apple-darwin
```

This writes `dist/Keyboard Curator.app` and
`dist/Keyboard-Curator-<version>-macos.dmg`, with the version from
`[workspace.package]` in `Cargo.toml`. For a universal build, add the Intel
target (`rustup target add x86_64-apple-darwin`), build it too, and pass
both triples; the script joins them with `lipo`.

Without a signing identity the app is ad-hoc signed. It runs on the Mac
that built it; on another Mac, Gatekeeper refuses it until the user allows
it in System Settings › Privacy & Security (or right-click › Open). That is
the state of every release until the secrets below exist.

To check a bundle:

```sh
codesign --verify --deep --strict --verbose=2 "dist/Keyboard Curator.app"
plutil -lint "dist/Keyboard Curator.app/Contents/Info.plist"
hdiutil verify dist/Keyboard-Curator-*-macos.dmg
spctl --assess --type execute "dist/Keyboard Curator.app"   # passes only once signed and notarized
```

## The release workflow

`release.yml` runs on a push of a tag `v*`, or by hand from the Actions
tab. It builds both Darwin targets on `macos-14`, runs the bundle script,
uploads the DMG as a workflow artifact and, on a tag, attaches it to the
GitHub release for that tag (created as a draft if it does not exist, so
the notes can be written before publishing).

Every signing and notarization step is guarded, so the workflow passes
with no secrets configured and produces an ad-hoc signed DMG. Once the
secrets exist, the same workflow signs and notarizes without any change.

### Secrets

Set these under the repository's Settings › Secrets and variables › Actions.

| Secret | What it is |
|---|---|
| `MACOS_CERTIFICATE_P12` | The Developer ID Application certificate and its private key, as a base64-encoded `.p12` |
| `MACOS_CERTIFICATE_PASSWORD` | The password chosen when exporting the `.p12` |
| `APPLE_ID` | The Apple ID of the developer account |
| `APPLE_TEAM_ID` | The ten-character team ID shown in the Apple Developer account |
| `APPLE_APP_PASSWORD` | An app-specific password for that Apple ID, used by `notarytool` |

The first two enable signing; all five together enable notarization.

**Exporting the certificate.** The account needs the Apple Developer
Program (paid). In Xcode › Settings › Accounts › Manage Certificates, add a
"Developer ID Application" certificate (or create it at
developer.apple.com/account/resources/certificates). Then in Keychain
Access, find the certificate under "My Certificates", expand it so the
private key shows, select both, and File › Export Items as a `.p12` with a
password. Encode it for the secret:

```sh
base64 -i DeveloperID.p12 | pbcopy
```

**App-specific password.** At account.apple.com › Sign-In and Security ›
App-Specific Passwords, generate one named for this workflow. It is shown
once; paste it into `APPLE_APP_PASSWORD`.

**Local signing.** The script reads the same names from the environment:

```sh
MACOS_SIGNING_IDENTITY="Developer ID Application: Name (TEAMID)" \
APPLE_ID=... APPLE_TEAM_ID=... APPLE_APP_PASSWORD=... \
ci/bundle-macos.sh aarch64-apple-darwin
```

`security find-identity -v -p codesigning` lists the identities in the
keychain.

## What the bundle contains, and what it does not

- Fonts, themes and board data are compiled into the binary
  (`include_bytes!` and `include_str!` in `crates/kc-app/src/theme.rs`), so
  `Contents/Resources` holds only the icon. Nothing is read from the bundle
  at run time; the app's own files live under the user's config and
  Documents folders.
- The app is **not sandboxed**. It talks to keyboards over USB and serial
  ports and reads and writes files wherever the user chooses, which the App
  Sandbox would make awkward, and it is distributed outside the Mac App
  Store, where the sandbox is optional. The hardened runtime is enabled
  (notarization requires it) with no exceptions: the app has no JIT and
  loads no unsigned code.
- `Info.plist` carries `NSBluetoothAlwaysUsageDescription` so that a future
  Bluetooth path prompts with a sensible message. Today's device access
  goes over USB and needs no usage description.
- The bundle identifier is `com.mroboff.keyboard-curator`. Changing it
  later is a new app as far as macOS permissions are concerned, so it is
  worth settling before the first signed release.

## Later

- **Homebrew cask.** Once releases are signed and notarized, a cask in a
  personal tap (`brew tap mroboff/keyboard-curator`) can point at the DMG
  with its SHA-256; casks for unsigned apps are a poor experience because
  of the Gatekeeper prompt.
- **Sparkle or another updater** is out of scope for v1; releases are
  downloaded from GitHub.
