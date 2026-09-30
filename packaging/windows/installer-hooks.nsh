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
  ; The stock installer also runs this hook while replacing a previous version.
  ; Retain both values then so POSTINSTALL can repair an enabled command.
  ; Only a real uninstall removes the operating system's startup state.
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Utterform"
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "Utterform"
  ${EndIf}
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend
