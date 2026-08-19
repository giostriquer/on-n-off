; NSIS hooks for the on-n-off Windows installer.
; Wired in via `bundle.windows.nsis.installerHooks` in tauri.conf.json.

; The installer replaces `on-n-off.exe` in place, and Explorer keys its icon cache by
; that path. On Windows 11 the taskbar button of a running window shows the shell's
; icon for the executable, not the window's WM_SETICON icon, so a stale cache entry
; keeps showing the previous release's icon after an upgrade. SHCNE_ASSOCCHANGED is
; the documented way to make the shell drop cached icons; installers send it after
; changing an executable's icon.
!macro NSIS_HOOK_POSTINSTALL
  ; SHChangeNotify(SHCNE_ASSOCCHANGED = 0x08000000, SHCNF_IDLIST | SHCNF_FLUSHNOWAIT = 0x2000, NULL, NULL)
  System::Call "shell32::SHChangeNotify(i 0x08000000, i 0x2000, p 0, p 0)"
!macroend
