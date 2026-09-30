# WinGet package

The three manifests in `0.7.11/` match the published Utterform v0.7.11 Windows
NSIS installer. Their active first-package submission is
[`microsoft/winget-pkgs#431040`](https://github.com/microsoft/winget-pkgs/pull/431040).

## Status

The package is **not yet available in the public WinGet catalog**. The
submission passed validation for v0.7.2 and was updated to v0.7.11 on
2026-09-30. The new automated validation and required community-moderator
approval must finish before Microsoft merges it. Catalog synchronization follows
the merge. Do not advertise an installation command in the root README until
`winget show --id JliSoftware.Utterform --exact` resolves the package after
`winget source update`.

## Manifest evidence

- The immutable v0.7.11 installer URL points to the published NSIS release asset.
  Its independently computed SHA-256 matches the public `SHA256SUMS.txt`.
- The installer is a per-user Nullsoft package; WinGet supplies its silent switch.
- The published application imports `MSVCP140.dll`, so the manifest declares
  `Microsoft.VCRedist.2015+.x64`. This addresses the earlier upstream
  `STATUS_DLL_NOT_FOUND` installation-validation failure.
- The Windows per-user uninstall entry has `ProductCode: Utterform`; the manifest
  carries the same value for package correlation and upgrades.
- `winget validate --manifest` succeeded on Windows 11 for the v0.7.11 manifests.

The Windows installer remains unsigned. WinGet does not remove a possible
SmartScreen prompt in interactive use.

## Local validation

```powershell
winget validate --manifest path\to\0.7.11
winget install --manifest path\to\0.7.11
```

The second command installs the package locally; use an appropriate Windows test
environment. After upstream merge and catalog synchronization, install with
`winget install --id JliSoftware.Utterform --exact`. Each later Utterform release
requires a separate WinGet manifest submission and merge before
`winget upgrade --id JliSoftware.Utterform --exact` can offer it. GitHub releases
are not automatically ingested into WinGet.
