; Ask Explorer to refresh its icon/association view after files and shortcuts
; change. SHCNE_ASSOCCHANGED + SHCNF_IDLIST is a supported shell notification:
; no cache files are deleted and Explorer is never killed or restarted.
; Tauri's stock NSIS installer retains ownership of upgrade, running-app checks,
; shortcut removal and the explicit opt-in checkbox for deleting app data.
!macro NSIS_HOOK_POSTINSTALL
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend
