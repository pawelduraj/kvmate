; The uninstaller removes the "start at sign-in" entry that tauri-plugin-autostart writes.
; The Tauri uninstaller already offers to delete the settings.

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "kvmate"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "kvmate"
!macroend
