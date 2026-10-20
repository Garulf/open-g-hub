; Per-user installer for open-g-hub. Build with:
;   makensis -DVERSION=<version> -DSTAGE=<dir with the exes> -DOUTFILE=<setup.exe> installer.nsi
; Silent install: setup.exe /S [/NOAUTOSTART] [/D=<dir>]

!ifndef VERSION
  !error "pass -DVERSION=<version>"
!endif
!ifndef STAGE
  !error "pass -DSTAGE=<dir with the exes>"
!endif
!ifndef OUTFILE
  !error "pass -DOUTFILE=<setup.exe>"
!endif

Unicode true
Target amd64-unicode
SetCompressor /SOLID lzma

!define APP "open-g-hub"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP}"
!define APP_KEY "Software\${APP}"

Name "${APP}"
OutFile "${OUTFILE}"
RequestExecutionLevel user
InstallDir "$LOCALAPPDATA\Programs\${APP}"
InstallDirRegKey HKCU "${APP_KEY}" "InstallDir"

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "Sections.nsh"
!include "FileFunc.nsh"

!define MUI_ICON "${NSISDIR}\Contrib\Graphics\Icons\modern-install.ico"
!define MUI_UNICON "${NSISDIR}\Contrib\Graphics\Icons\modern-uninstall.ico"
!define MUI_COMPONENTSPAGE_NODESC
!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_TEXT "Start ${APP} now"
!define MUI_FINISHPAGE_RUN_FUNCTION StartApp

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

!macro StopRunningCopies
  DetailPrint "Stopping running copies of ${APP}"
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM ogh-gui.exe /IM ogh-tray.exe /IM ogh-daemon.exe'
  Pop $0
  Sleep 1000
!macroend

!macro RunAutostart action
  DetailPrint "ogh autostart ${action}"
  nsExec::ExecToLog '"$INSTDIR\ogh.exe" autostart ${action}'
  Pop $0
  ${If} $0 != 0
    MessageBox MB_ICONEXCLAMATION|MB_OK "ogh autostart ${action} failed ($0). Run it from a terminal in $INSTDIR to see why." /SD IDOK
  ${EndIf}
!macroend

Section "${APP}" SEC_APP
  SectionIn RO
  !insertmacro StopRunningCopies

  SetOutPath "$INSTDIR"
  File "${STAGE}\ogh.exe"
  File "${STAGE}\ogh-daemon.exe"
  File "${STAGE}\ogh-gui.exe"
  File "${STAGE}\ogh-tray.exe"
  File "${STAGE}\README.txt"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  CreateDirectory "$SMPROGRAMS\${APP}"
  CreateShortcut "$SMPROGRAMS\${APP}\${APP}.lnk" "$INSTDIR\ogh-gui.exe"
  CreateShortcut "$SMPROGRAMS\${APP}\Uninstall ${APP}.lnk" "$INSTDIR\uninstall.exe"

  WriteRegStr HKCU "${APP_KEY}" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${APP}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "${APP}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "URLInfoAbout" "https://github.com/Garulf/open-g-hub"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\ogh-gui.exe"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

Section "Start ${APP} when I sign in" SEC_AUTOSTART
SectionEnd

Section "-autostart"
  ${If} ${SectionIsSelected} ${SEC_AUTOSTART}
    !insertmacro RunAutostart on
  ${Else}
    !insertmacro RunAutostart off
  ${EndIf}
SectionEnd

Function .onInit
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/NOAUTOSTART" $1
  ${IfNot} ${Errors}
    !insertmacro UnselectSection ${SEC_AUTOSTART}
  ${EndIf}
FunctionEnd

Function StartApp
  Exec '"$INSTDIR\ogh-daemon.exe"'
  Exec '"$INSTDIR\ogh-tray.exe"'
FunctionEnd

Section "Uninstall"
  !insertmacro StopRunningCopies
  !insertmacro RunAutostart off

  Delete "$INSTDIR\ogh.exe"
  Delete "$INSTDIR\ogh-daemon.exe"
  Delete "$INSTDIR\ogh-gui.exe"
  Delete "$INSTDIR\ogh-tray.exe"
  Delete "$INSTDIR\README.txt"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\${APP}\${APP}.lnk"
  Delete "$SMPROGRAMS\${APP}\Uninstall ${APP}.lnk"
  RMDir "$SMPROGRAMS\${APP}"

  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  DeleteRegKey HKCU "${APP_KEY}"
SectionEnd
