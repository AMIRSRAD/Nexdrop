; Extra steps for the Nexdrop installer/uninstaller (NSIS hooks).

!macro NSIS_HOOK_PREINSTALL
!macroend

!macro NSIS_HOOK_POSTINSTALL
!macroend

!macro NSIS_HOOK_PREUNINSTALL
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Remove the "start with Windows" entry the app may have created.
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Nexdrop"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "nexdrop"

  ; When the user ticks "Delete the application data", also remove Nexdrop's
  ; database and settings. Downloaded files are never touched.
  ${If} $DeleteAppDataCheckboxState == 1
    RMDir /r "$LOCALAPPDATA\Nexdrop"
  ${EndIf}
!macroend
