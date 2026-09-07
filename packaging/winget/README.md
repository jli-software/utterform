# winget package

Manifests for the [Windows Package Manager](https://github.com/microsoft/winget-pkgs)
community repository, so Windows users can install Utterform with:

```
winget install Utterform
```

winget needs no code signing certificate and costs nothing. It does not make the
Windows SmartScreen prompt disappear — the installer is still unsigned — but it
removes the browser download warning, because nothing is downloaded by hand.

## Status

**Not submitted yet.** The manifests below are prepared and validated, but the
package does not exist in `microsoft/winget-pkgs`. Do not advertise the
`winget install` command in the README until the pull request is merged.

## What is here

`0.4.2/` holds the three manifests winget expects, matching the published
release assets:

| File | Purpose |
| --- | --- |
| `JliSoftware.Utterform.yaml` | Version manifest — ties the other two together |
| `JliSoftware.Utterform.installer.yaml` | The NSIS installer, its URL and SHA-256 |
| `JliSoftware.Utterform.locale.en-US.yaml` | Name, publisher, licence, description |

Facts these were built from, verified against the published `v0.4.2` assets on
2026-09-07 rather than assumed:

- `InstallerSha256` matches `SHA256SUMS.txt` **and** an independent `sha256sum`
  of the downloaded `utterform-windows-x86_64-setup.exe`.
- `Scope: user` and no elevation requirement: the installer's embedded manifest
  requests `asInvoker`, which matches Tauri's per-user NSIS default.
- `InstallerType: nullsoft` — the installer reports Nullsoft Install System
  v3.11. winget supplies the `/S` silent switch for that type itself.

## Submitting

The pull request has to come from your own GitHub account. On a Windows machine:

```
winget install Microsoft.WingetCreate
wingetcreate submit --token <your-github-token> path\to\0.4.2
```

`wingetcreate` forks `microsoft/winget-pkgs` for you, copies the manifests to
`manifests/j/JliSoftware/Utterform/0.4.2/` and opens the pull request.

To check the manifests before submitting:

```
winget validate --manifest path\to\0.4.2
winget install --manifest path\to\0.4.2
```

The second one actually installs from the local manifest, which is the honest
test that the package works.

After submitting, an automated pipeline validates the manifest and virus-scans
the installer. Unsigned installers usually pass, but a first submission can wait
on a human moderator for a few days.

## Later versions

`wingetcreate update JliSoftware.Utterform --version <new> --urls <installer url> --submit`
takes the new installer, computes its hash and opens the follow-up pull request.
Automating that in the release workflow is the intended next step; it is not
wired up yet.

## Open point

`AppsAndFeaturesEntries` / `ProductCode` is deliberately absent. It would let
`winget upgrade` match an already-installed Utterform precisely, and the correct
value can only be read from a real Windows installation:

```
Get-ItemProperty HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\* |
  Where-Object DisplayName -like "*Utterform*" | Select-Object PSChildName, DisplayName, DisplayVersion
```

Guessing it would be worse than leaving it out, so it is left out until someone
runs that on Windows.
