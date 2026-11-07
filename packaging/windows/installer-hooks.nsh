; Ask Explorer to refresh its icon/association view after files and shortcuts
; change. SHCNE_ASSOCCHANGED + SHCNF_IDLIST is a supported shell notification:
; no cache files are deleted and Explorer is never killed or restarted.
; Tauri's stock NSIS installer retains ownership of upgrade, running-app checks,
; shortcut removal and the explicit opt-in checkbox for deleting app data.
; "Open with > Utterform" for voice messages and recordings (0.13.0), per user.
; Only the Open-with list is extended: each type gains an OpenWithProgids value
; and Utterform an Applications entry. The type's default program is never
; read or written, unlike Tauri's own fileAssociations, which would make
; Utterform the default player for MP3 and WAV.
!define UTTERFORM_PROGID "Utterform.AudioFile"
!macro UTTERFORM_OPEN_WITH EXT
  WriteRegStr HKCU "Software\Classes\.${EXT}\OpenWithProgids" "${UTTERFORM_PROGID}" ""
  WriteRegStr HKCU "Software\Classes\Applications\utterform.exe\SupportedTypes" ".${EXT}" ""
!macroend
!macro UTTERFORM_OPEN_WITH_REMOVE EXT
  DeleteRegValue HKCU "Software\Classes\.${EXT}\OpenWithProgids" "${UTTERFORM_PROGID}"
!macroend
!macro UTTERFORM_REGISTER_OPEN_WITH
  WriteRegStr HKCU "Software\Classes\${UTTERFORM_PROGID}" "" "Audio file"
  WriteRegStr HKCU "Software\Classes\${UTTERFORM_PROGID}\DefaultIcon" "" '"$INSTDIR\utterform.exe",0'
  WriteRegStr HKCU "Software\Classes\${UTTERFORM_PROGID}\shell\open" "FriendlyAppName" "Utterform"
  WriteRegStr HKCU "Software\Classes\${UTTERFORM_PROGID}\shell\open\command" "" '"$INSTDIR\utterform.exe" --file "%1"'
  WriteRegStr HKCU "Software\Classes\Applications\utterform.exe" "FriendlyAppName" "Utterform"
  WriteRegStr HKCU "Software\Classes\Applications\utterform.exe\shell\open\command" "" '"$INSTDIR\utterform.exe" --file "%1"'
  !insertmacro UTTERFORM_OPEN_WITH "opus"
  !insertmacro UTTERFORM_OPEN_WITH "ogg"
  !insertmacro UTTERFORM_OPEN_WITH "oga"
  !insertmacro UTTERFORM_OPEN_WITH "m4a"
  !insertmacro UTTERFORM_OPEN_WITH "mp3"
  !insertmacro UTTERFORM_OPEN_WITH "wav"
  !insertmacro UTTERFORM_OPEN_WITH "flac"
  !insertmacro UTTERFORM_OPEN_WITH "aac"
  !insertmacro UTTERFORM_OPEN_WITH "webm"
  !insertmacro UTTERFORM_OPEN_WITH "mka"
  !insertmacro UTTERFORM_OPEN_WITH "aiff"
!macroend
!macro UTTERFORM_UNREGISTER_OPEN_WITH
  !insertmacro UTTERFORM_OPEN_WITH_REMOVE "opus"
  !insertmacro UTTERFORM_OPEN_WITH_REMOVE "ogg"
  !insertmacro UTTERFORM_OPEN_WITH_REMOVE "oga"
  !insertmacro UTTERFORM_OPEN_WITH_REMOVE "m4a"
  !insertmacro UTTERFORM_OPEN_WITH_REMOVE "mp3"
  !insertmacro UTTERFORM_OPEN_WITH_REMOVE "wav"
  !insertmacro UTTERFORM_OPEN_WITH_REMOVE "flac"
  !insertmacro UTTERFORM_OPEN_WITH_REMOVE "aac"
  !insertmacro UTTERFORM_OPEN_WITH_REMOVE "webm"
  !insertmacro UTTERFORM_OPEN_WITH_REMOVE "mka"
  !insertmacro UTTERFORM_OPEN_WITH_REMOVE "aiff"
  DeleteRegKey HKCU "Software\Classes\${UTTERFORM_PROGID}"
  DeleteRegKey HKCU "Software\Classes\Applications\utterform.exe"
!macroend

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
  !insertmacro UTTERFORM_REGISTER_OPEN_WITH
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; The stock installer also runs this hook while replacing a previous version.
  ; Retain both values then so POSTINSTALL can repair an enabled command.
  ; Only a real uninstall removes the operating system's startup state.
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Utterform"
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "Utterform"
    !insertmacro UTTERFORM_UNREGISTER_OPEN_WITH
  ${EndIf}
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend
