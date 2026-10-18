; Extra steps for the Nexdrop installer/uninstaller (NSIS hooks).

; Set when Nexdrop was already installed, so an update never switches back on
; something the user turned off.
Var NexdropWasInstalled

!macro NSIS_HOOK_PREINSTALL
  StrCpy $NexdropWasInstalled "0"
  ${If} ${FileExists} "$INSTDIR\nexdrop.exe"
    StrCpy $NexdropWasInstalled "1"
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Start with Windows, quietly in the tray, on a fresh install. This is the same
  ; entry the "Start Nexdrop when I sign in" switch in Settings controls.
  ${If} $NexdropWasInstalled == "0"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Nexdrop" '"$INSTDIR\nexdrop.exe" --minimized'
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Remove the "start with Windows" entry the installer or the app created.
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Nexdrop"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "nexdrop"

  ; When the user ticks "Delete the application data", also remove Nexdrop's
  ; database and settings. Downloaded files are never touched.
  ${If} $DeleteAppDataCheckboxState == 1
    RMDir /r "$LOCALAPPDATA\Nexdrop"
  ${EndIf}
!macroend
