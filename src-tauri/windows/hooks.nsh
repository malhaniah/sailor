; Zeiler was called Sailor up to v0.1.2. A Sailor install's updater runs this
; installer in update mode, which installs next to Sailor (the product name changed)
; and creates no shortcuts. So when Sailor is present: give Zeiler its shortcuts and
; silently uninstall Sailor. A silent uninstall keeps app data, which Zeiler copies
; over on first launch (see migrate_legacy_data in src-tauri/src/state.rs).
!macro NSIS_HOOK_POSTINSTALL
  Push $R0
  Push $R1
  ReadRegStr $R0 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Sailor" "InstallLocation"
  ; the value is stored quoted: "C:\...\Sailor"
  StrCpy $R1 $R0 1
  ${If} $R1 == '"'
    StrCpy $R0 $R0 -1 1
  ${EndIf}
  ${If} $R0 != ""
  ${AndIf} $R0 != $INSTDIR
    ${IfNot} ${FileExists} "$SMPROGRAMS\${PRODUCTNAME}.lnk"
      CreateShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    ${EndIf}
    ${If} ${FileExists} "$DESKTOP\Sailor.lnk"
    ${AndIfNot} ${FileExists} "$DESKTOP\${PRODUCTNAME}.lnk"
      CreateShortcut "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    ${EndIf}
    ${If} ${FileExists} "$R0\uninstall.exe"
      ; _?= runs the uninstaller in place, so it cannot delete itself
      ExecWait '"$R0\uninstall.exe" /S _?=$R0'
      Delete "$R0\uninstall.exe"
      RMDir "$R0"
    ${EndIf}
  ${EndIf}
  Pop $R1
  Pop $R0
!macroend
