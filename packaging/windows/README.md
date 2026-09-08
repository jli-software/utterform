# Windows installer branding

The executable, NSIS setup and uninstaller use the generated Signal `icon.ico`.
Run `npm run icons` after changing `src-tauri/icons/app-icon.svg`.

The two supported NSIS hooks notify the Windows shell with `SHChangeNotify`
(`SHCNE_ASSOCCHANGED`, `SHCNF_IDLIST`) after install/remove so Explorer can
refresh its displayed icons. They do not delete cache files, restart Explorer,
change privileges, or touch application data.

Keep Tauri's stock installer template and existing product name, identifier,
install mode and shortcut locations. It already checks for a running app,
handles existing installations, removes its shortcuts and Add/Remove Programs
entry, and offers an explicit **Delete app data** checkbox on uninstall. Data
deletion is not enabled by these hooks; the default keeps settings and models.

## Native verification before release

On Windows, install over 0.4.6 and verify the app, Start menu shortcut, setup
and uninstaller show the Signal mark. Confirm settings and downloaded models
remain available. Remove with **Delete app data** unchecked, including once
while the app is running, and verify the stock close-app prompt, shortcut
cleanup and retained data. Reinstall and confirm the retained settings load.
Shell notification delivery and Explorer's rendering require a real Windows
session; Linux-side asset/config checks cannot verify them.

References:

- [Tauri NSIS configuration](https://v2.tauri.app/reference/config/#nsisconfig)
- [Stock installer for CLI 2.11.4](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.11.4/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi)
- [Microsoft SHChangeNotify](https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/nf-shlobj_core-shchangenotify)
