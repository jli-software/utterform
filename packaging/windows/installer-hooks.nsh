; Ask Explorer to refresh its icon/association view after files and shortcuts
; change. SHCNE_ASSOCCHANGED + SHCNF_IDLIST is a supported shell notification:
; no cache files are deleted and Explorer is never killed or restarted.
; Tauri's stock NSIS installer retains ownership of upgrade, running-app checks,
; shortcut removal and the explicit opt-in checkbox for deleting app data.
!macro NSIS_HOOK_POSTINSTALL
  ; Preserve an existing opt-in across upgrades, but repair the command from
  ; auto-launch 0.5.0: an unquoted per-user path can look registered while
  ; CreateProcess cannot start it. Keep Windows' StartupApproved choice.
  Push $0
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Utterform"
  ${If} $0 != ""
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Utterform" '"$INSTDIR\utterform.exe" --autostart'
  ${EndIf}
  Pop $0
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Startup is OS-owned rather than app data. Never leave a command pointing
  ; at the removed executable or an orphaned Explorer approval value.
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Utterform"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "Utterform"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend
