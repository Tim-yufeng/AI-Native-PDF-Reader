; Project-owned bounded candidate template, not an upstream template fork.
; This script is compiled only. Installation/uninstallation needs new approval.
Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "WordFunc.nsh"
!include "x64.nsh"
!define PRODUCT "{{product_name}}"
!define BINARY "{{main_binary_name}}"
!define APPID "{{bundle_id}}"
!define PROGID "${APPID}.PDF"
!define UNINSTALL "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPID}"
!define MARKER ".${APPID}.installation"
; WebView2 runtime supply, selected at compile time by the Tauri bundle config.
; The baseline config keeps `skip`, so INSTALLWEBVIEW2MODE is "skip" there and
; every line below that depends on it compiles away: that package still installs
; no system runtime. The offline validation config selects `offlineInstaller`,
; which makes the bundler embed the signed Microsoft offline installer and pass
; its source path below. Nothing here downloads at install time.
!define WEBVIEW2APPGUID "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
!define INSTALLWEBVIEW2MODE "{{install_webview2_mode}}"
!define WEBVIEW2INSTALLERARGS "{{webview2_installer_args}}"
!define WEBVIEW2INSTALLERPATH "{{webview2_installer_path}}"
Var RetainedWriter
Var DirectoryGuards
Var RootGuards
Var LeafGuard
Var GuardPath
Var GuardTarget
Var GuardPrefix
Var GuardRemainder
Var GuardSegment
Var GuardCreate
Var GuardReserve
Var GuardHandle
Var GuardBuffer
Var GuardError
Var InstallStage
Var InstallPath
Var RemoveChatData
!define MUI_UNCHECKBOXBLURB "同时删除本应用的本地聊天、设置与缓存数据"
!define MUI_UNCHECKBOXBLURBTEXT "仅删除应用自己的数据目录（聊天记录、恢复材料、设置、缓存）。$\r$\n源 PDF、批注和您自己的文件不会被删除。$\r$\n不勾选则全部保留，便于以后继续使用或升级。"
Var WebView2Version
Var WebView2Location
Var WebView2Present
Var WebView2ExitCode
Name "${PRODUCT}"
OutFile "{{out_file}}"
RequestExecutionLevel user
SetCompressor /SOLID lzma
InstallDir "$LOCALAPPDATA\Programs\${PRODUCT}"
InstallDirRegKey HKCU "${UNINSTALL}" "InstallLocation"
VIProductVersion "{{version_with_build}}"
VIAddVersionKey "ProductName" "${PRODUCT}"
VIAddVersionKey "FileDescription" "${PRODUCT}"
VIAddVersionKey "FileVersion" "{{version}}"
!define MUI_ABORTWARNING
!define MUI_CUSTOMFUNCTION_ABORT ReleaseAllGuards
!define MUI_CUSTOMFUNCTION_UNABORT un.ReleaseAllGuards
!define MUI_ICON "{{no-escape installer_icon}}"
!define MUI_UNICON "{{no-escape installer_icon}}"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_COMPONENTS
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"
!insertmacro MUI_LANGUAGE "English"

!macro RequireInstallSuccess
  ${If} ${Errors}
    DetailPrint "Installation failed: $InstallStage; path=$InstallPath; NSIS operation reported failure (no reliable Win32 code supplied)."
    MessageBox MB_ICONSTOP "Installation did not finish.$\r$\nStep: $InstallStage$\r$\nPath: $InstallPath$\r$\nThe NSIS operation reported failure; an exact Windows error code is unavailable.$\r$\nPartial program materials were retained; no user data was removed."
    Call ReleaseAllGuards
    SetErrorLevel 1
    Abort
  ${EndIf}
!macroend
; Native status distinguishes an absent optional entry (2/3) from denial (5).
; Keys/names below are fixed application-owned literals, never user input.
!macro RemoveOwnedKey KEY
  System::Call 'advapi32::RegDeleteTreeW(p 0x80000001, w "${KEY}") i .r2'
  ${If} $2 != 0
  ${AndIf} $2 != 2
  ${AndIf} $2 != 3
    SetErrors
    !insertmacro RequireUninstallSuccess
  ${EndIf}
!macroend
!macro RemoveOwnedValue KEY VALUE
  System::Call 'advapi32::RegOpenKeyExW(p 0x80000001, w "${KEY}", i 0, i 2, *p .r2) i .r3'
  ${If} $3 == 0
    System::Call 'advapi32::RegDeleteValueW(p r2, w "${VALUE}") i .r3'
    System::Call 'advapi32::RegCloseKey(p r2) i .r2'
    ${If} $2 != 0
      SetErrors
      !insertmacro RequireUninstallSuccess
    ${EndIf}
  ${EndIf}
  ${If} $3 != 0
  ${AndIf} $3 != 2
  ${AndIf} $3 != 3
    SetErrors
    !insertmacro RequireUninstallSuccess
  ${EndIf}
!macroend
!macro RequireUninstallSuccess
  ${If} ${Errors}
    MessageBox MB_ICONSTOP "Removal did not finish. Remaining program materials and all user data were retained."
    SetErrorLevel 1
    Abort
  ${EndIf}
!macroend

!macro RejectInstallDirectory OPERATION CODE REASON
  DetailPrint "Directory check failed: ${OPERATION}; path=$INSTDIR; Win32=${CODE}; ${REASON}"
  MessageBox MB_ICONSTOP "The installation directory cannot be checked. Nothing will be overwritten.$\r$\nPath: $INSTDIR$\r$\nOperation: ${OPERATION}$\r$\nWindows error: ${CODE}$\r$\n${REASON}"
  SetErrorLevel 1
  Abort
!macroend

; READ_DATA/LIST_DIRECTORY plus READ_ATTRIBUTES participates in sharing checks;
; READ_ATTRIBUTES alone does not prevent DeleteFile. Read handles remain
; compatible with NSIS's CREATE_ALWAYS/GENERIC_WRITE and FILE_SHARE_READ.
; These bounded checks are not an atomic installation transaction and do not
; claim immunity from an adversary already holding equal-user write access.
!macro GuardFailure PREFIX REASON
  DetailPrint "Program path check failed: $GuardPath; Win32=$GuardError; ${REASON}"
  MessageBox MB_ICONSTOP "Program path cannot be used safely.$\r$\nPath: $GuardPath$\r$\nWindows error: $GuardError$\r$\n${REASON}$\r$\nPartial program materials and all user data were retained."
  Call ${PREFIX}ReleaseAllGuards
  SetErrorLevel 1
  Abort
!macroend

!macro GuardFunctions PREFIX
Function ${PREFIX}ReleaseLeafGuard
  ${If} $LeafGuard P<> 0
    System::Call 'kernel32::CloseHandle(p "$LeafGuard") i .r0'
    StrCpy $LeafGuard "0"
    ${If} $0 == 0
      SetErrors
    ${EndIf}
  ${EndIf}
FunctionEnd

Function ${PREFIX}ReleaseDirectoryGuards
  ${DoWhile} $DirectoryGuards P<> 0
    StrCpy $8 $DirectoryGuards
    System::Call '*$8(p .r6, p .r9)'
    StrCpy $DirectoryGuards $9
    System::Call 'kernel32::CloseHandle(p r6) i .r0'
    System::Free $8
    ${If} $0 == 0
      SetErrors
    ${EndIf}
  ${Loop}
FunctionEnd

Function ${PREFIX}ReleaseAllGuards
  Call ${PREFIX}ReleaseLeafGuard
  Call ${PREFIX}ReleaseDirectoryGuards
  StrCpy $DirectoryGuards $RootGuards
  StrCpy $RootGuards "0"
  Call ${PREFIX}ReleaseDirectoryGuards
FunctionEnd

Function ${PREFIX}PinDirectory
  System::Call 'kernel32::CreateFileW(w "$GuardPath", i 0x81, i 3, p 0, i 3, i 0x02200000, p 0) p .r6 ?e'
  Pop $GuardError
  ${If} $6 P= -1
    ${If} $GuardError == 2
    ${OrIf} $GuardError == 3
      ${If} $GuardCreate == "0"
        Return
      ${EndIf}
      ; Only one segment is created; its entire parent chain is already pinned.
      System::Call 'kernel32::CreateDirectoryW(w "$GuardPath", p 0) i .r0 ?e'
      Pop $GuardError
      ${If} $0 == 0
      ${AndIf} $GuardError != 183
        !insertmacro GuardFailure "${PREFIX}" "An ordinary program directory could not be created."
      ${EndIf}
      System::Call 'kernel32::CreateFileW(w "$GuardPath", i 0x81, i 3, p 0, i 3, i 0x02200000, p 0) p .r6 ?e'
      Pop $GuardError
    ${EndIf}
  ${EndIf}
  ${If} $6 P= -1
    !insertmacro GuardFailure "${PREFIX}" "The program directory cannot be opened without following links."
  ${EndIf}
  StrCpy $GuardHandle $6
  System::Alloc 52
  Pop $GuardBuffer
  ${If} $GuardBuffer P= 0
    System::Call 'kernel32::CloseHandle(p "$GuardHandle") i .r0'
    StrCpy $GuardError "8"
    !insertmacro GuardFailure "${PREFIX}" "Directory verification memory is unavailable."
  ${EndIf}
  System::Call 'kernel32::GetFileInformationByHandle(p "$GuardHandle", p "$GuardBuffer") i .r0 ?e'
  Pop $GuardError
  ${If} $0 == 0
    System::Free $GuardBuffer
    System::Call 'kernel32::CloseHandle(p "$GuardHandle") i .r0'
    !insertmacro GuardFailure "${PREFIX}" "The opened directory could not be verified."
  ${EndIf}
  System::Call '*$GuardBuffer(i .r0)'
  System::Free $GuardBuffer
  IntOp $0 $0 & 0x410
  ; IntOp emits decimal text (16). LogicLib != is string comparison and would
  ; reject it against "0x10". <> expands to IntCmp and compares numeric values.
  ${If} $0 <> 0x10
    System::Call 'kernel32::CloseHandle(p "$GuardHandle") i .r0'
    StrCpy $GuardError "n/a"
    !insertmacro GuardFailure "${PREFIX}" "Linked or non-directory program paths are refused."
  ${EndIf}
  System::Alloc 16
  Pop $8
  ${If} $8 P= 0
    System::Call 'kernel32::CloseHandle(p "$GuardHandle") i .r0'
    StrCpy $GuardError "8"
    !insertmacro GuardFailure "${PREFIX}" "Directory guard memory is unavailable."
  ${EndIf}
  StrCpy $6 $GuardHandle
  StrCpy $9 $DirectoryGuards
  System::Call '*$8(p r6, p r9)'
  StrCpy $DirectoryGuards $8
FunctionEnd

Function ${PREFIX}PinDirectoryTree
  ; Only normalized local drive paths are accepted, never UNC/device/ADS paths.
  ; NSIS GetFullPathName also probes the final leaf and clears its result when
  ; that leaf does not exist. It cannot be used before creating a fresh target.
  ; Win32 normalization below does not assert existence or grant any access.
  StrCpy $0 $GuardTarget 1 1
  StrCpy $1 $GuardTarget 1 2
  ${If} $0 != ":"
  ${OrIf} $1 != "\"
    StrCpy $GuardPath $GuardTarget
    StrCpy $GuardError "n/a"
    !insertmacro GuardFailure "${PREFIX}" "An absolute local drive directory is required; relative, UNC and device paths are refused."
  ${EndIf}
  System::Call 'kernel32::GetFullPathNameW(w "$GuardTarget", i ${NSIS_MAX_STRLEN}, w .r7, p 0) i .r5 ?e'
  Pop $GuardError
  ${If} $5 == 0
    StrCpy $GuardPath $GuardTarget
    !insertmacro GuardFailure "${PREFIX}" "The program directory could not be normalized."
  ${EndIf}
  ${If} $5 >= ${NSIS_MAX_STRLEN}
  ${OrIf} $7 == ""
    StrCpy $GuardPath $GuardTarget
    StrCpy $GuardError "206"
    !insertmacro GuardFailure "${PREFIX}" "The normalized program directory exceeds the installer path limit."
  ${EndIf}
  StrCpy $GuardTarget $7
  StrCpy $0 $GuardTarget 1 1
  StrCpy $1 $GuardTarget 1 2
  ${If} $0 != ":"
  ${OrIf} $1 != "\"
    StrCpy $GuardPath $GuardTarget
    StrCpy $GuardError "n/a"
    !insertmacro GuardFailure "${PREFIX}" "UNC and device program directories are refused."
  ${EndIf}
  StrCpy $GuardPrefix $GuardTarget 3
  StrCpy $GuardRemainder $GuardTarget "" 3
  StrCpy $GuardPath $GuardPrefix
  Call ${PREFIX}PinDirectory
  StrCpy $GuardSegment ""
  ${Do}
    StrCpy $0 $GuardRemainder 1
    StrCpy $GuardRemainder $GuardRemainder "" 1
    ${If} $0 == ":"
      StrCpy $GuardPath $GuardTarget
      StrCpy $GuardError "n/a"
      !insertmacro GuardFailure "${PREFIX}" "Alternate data streams are refused."
    ${EndIf}
    ${If} $0 == "\"
    ${OrIf} $0 == ""
      ${If} $GuardSegment != ""
        StrCpy $1 $GuardSegment 1 -1
        ${If} $1 == "."
        ${OrIf} $1 == " "
          StrCpy $GuardPath $GuardTarget
          StrCpy $GuardError "n/a"
          !insertmacro GuardFailure "${PREFIX}" "Ambiguous trailing dot or space path segments are refused."
        ${EndIf}
        StrCpy $GuardPath "$GuardPrefix$GuardSegment"
        Call ${PREFIX}PinDirectory
        StrCpy $GuardPrefix "$GuardPath\"
        StrCpy $GuardSegment ""
      ${EndIf}
      ${If} $GuardRemainder == ""
        Return
      ${EndIf}
    ${Else}
      StrCpy $GuardSegment "$GuardSegment$0"
    ${EndIf}
  ${Loop}
FunctionEnd

Function ${PREFIX}PinLeaf
  ${If} $LeafGuard P<> 0
    StrCpy $GuardError "n/a"
    !insertmacro GuardFailure "${PREFIX}" "A previous program file guard is still active."
  ${EndIf}
  System::Call 'kernel32::CreateFileW(w "$GuardPath", i 0x81, i 3, p 0, i 3, i 0x00200000, p 0) p .r6 ?e'
  Pop $GuardError
  ${If} $6 P= -1
    ${If} $GuardError == 2
    ${OrIf} $GuardError == 3
      ${If} $GuardReserve == "0"
        Return
      ${EndIf}
      ; CREATE_NEW reserves only an absent leaf; never truncates existing data.
      System::Call 'kernel32::CreateFileW(w "$GuardPath", i 0x81, i 3, p 0, i 1, i 0x00200000, p 0) p .r6 ?e'
      Pop $GuardError
    ${EndIf}
  ${EndIf}
  ${If} $6 P= -1
    !insertmacro GuardFailure "${PREFIX}" "A program file could not be safely reserved or opened."
  ${EndIf}
  StrCpy $LeafGuard $6
  System::Alloc 52
  Pop $GuardBuffer
  ${If} $GuardBuffer P= 0
    StrCpy $GuardError "8"
    !insertmacro GuardFailure "${PREFIX}" "Program file verification memory is unavailable."
  ${EndIf}
  System::Call 'kernel32::GetFileInformationByHandle(p "$LeafGuard", p "$GuardBuffer") i .r0 ?e'
  Pop $GuardError
  ${If} $0 == 0
    System::Free $GuardBuffer
    !insertmacro GuardFailure "${PREFIX}" "The opened program file could not be verified."
  ${EndIf}
  System::Call '*$GuardBuffer(i .r0, &v36, i .r1)'
  System::Free $GuardBuffer
  IntOp $0 $0 & 0x410
  ${If} $0 != 0
  ${OrIf} $1 != 1
    StrCpy $GuardError "n/a"
    !insertmacro GuardFailure "${PREFIX}" "Linked, multiply linked or directory program files are refused."
  ${EndIf}
FunctionEnd
!macroend
!insertmacro GuardFunctions ""
!insertmacro GuardFunctions "un."

!macro BeginGuardedWrite PATH
  StrCpy $InstallStage "write-program-file"
  StrCpy $InstallPath "${PATH}"
  StrCpy $GuardPath "${PATH}"
  StrCpy $GuardReserve "1"
  Call PinLeaf
  ClearErrors
!macroend
!macro EndGuardedWrite
  !insertmacro RequireInstallSuccess
  StrCpy $InstallStage "release-file-guard"
  Call ReleaseLeafGuard
  !insertmacro RequireInstallSuccess
!macroend

; NSIS Delete adjusts readonly attributes before deletion. Refuse pre-existing
; leaf links first, then release the leaf handle so Delete is not self-blocked.
; Parent directory handles continue to pin the route. This is not an atomic
; check/delete protocol against an equal-user leaf replacement race.
!macro CheckUninstallLeaf PATH
  StrCpy $GuardPath "${PATH}"
  StrCpy $GuardReserve "0"
  Call un.PinLeaf
  Call un.ReleaseLeafGuard
  !insertmacro RequireUninstallSuccess
!macroend

Function .onInstFailed
  Call ReleaseAllGuards
FunctionEnd
Function .onGUIEnd
  Call ReleaseAllGuards
FunctionEnd
Function un.onUninstFailed
  Call un.ReleaseAllGuards
FunctionEnd
Function un.onGUIEnd
  Call un.ReleaseAllGuards
FunctionEnd

Function .onInit
  SetShellVarContext current
  StrCpy $DirectoryGuards "0"
  StrCpy $RootGuards "0"
  StrCpy $LeafGuard "0"
  ReadRegStr $0 HKCU "${UNINSTALL}" "DisplayVersion"
  ${If} $0 != ""
    ${VersionCompare} $0 "{{version}}" $1
    ${If} $1 == 1
      MessageBox MB_ICONSTOP "A newer reader is already installed. This candidate will not downgrade it."
      Abort
    ${EndIf}
  ${EndIf}
FunctionEnd

Function un.onInit
  SetShellVarContext current
  StrCpy $DirectoryGuards "0"
  StrCpy $RootGuards "0"
  StrCpy $LeafGuard "0"
  StrCpy $RemoveChatData "0"
  StrCpy $GuardTarget $INSTDIR
  StrCpy $GuardCreate "0"
  Call un.PinDirectoryTree
  StrCpy $GuardTarget $SMPROGRAMS
  Call un.PinDirectoryTree
  StrCpy $RootGuards $DirectoryGuards
  StrCpy $DirectoryGuards "0"
  StrCpy $GuardPath "$INSTDIR\${MARKER}"
  StrCpy $GuardReserve "0"
  Call un.PinLeaf
  ClearErrors
  FileOpen $0 "$INSTDIR\${MARKER}" r
  ${If} ${Errors}
    MessageBox MB_ICONSTOP "Installation identity is unavailable. No files or user data will be removed."
    Abort
  ${EndIf}
  FileRead $0 $1
  FileClose $0
  !insertmacro RequireUninstallSuccess
  Call un.ReleaseLeafGuard
  !insertmacro RequireUninstallSuccess
  ${If} $1 != "${APPID}"
    MessageBox MB_ICONSTOP "This directory does not belong to the reader. No files will be removed."
    Abort
  ${EndIf}
FunctionEnd

; Do not overwrite a generic uninstall.exe or a foreign application directory.
; Only a new/empty directory or our registered, marked directory is accepted.
Function CheckInstallDir
  ; ?e saves the target API's last error before returning to NSIS/plugin code.
  System::Call 'kernel32::GetFileAttributesW(w "$INSTDIR") i .r3 ?e'
  Pop $4
  ${If} $3 == -1
    ${If} $4 == 2
    ${OrIf} $4 == 3
      Return
    ${EndIf}
    !insertmacro RejectInstallDirectory "GetFileAttributesW" "$4" "Only file/path-not-found (2/3) is accepted as a new directory."
  ${EndIf}
  IntOp $4 $3 & 0x400
  ${If} $4 != 0
    !insertmacro RejectInstallDirectory "directory-policy" "n/a" "Choose a directory without a reparse point."
  ${EndIf}
  IntOp $4 $3 & 0x10
  ${If} $4 == 0
    !insertmacro RejectInstallDirectory "directory-policy" "n/a" "The selected path is not a directory."
  ${EndIf}
  ReadRegStr $0 HKCU "${UNINSTALL}" "InstallLocation"
  ${If} $0 == "$INSTDIR"
    StrCpy $GuardPath "$INSTDIR\${MARKER}"
    StrCpy $GuardReserve "0"
    Call PinLeaf
    ClearErrors
    FileOpen $1 "$INSTDIR\${MARKER}" r
    ${IfNot} ${Errors}
      FileRead $1 $2
      FileClose $1
      !insertmacro RequireInstallSuccess
      Call ReleaseLeafGuard
      !insertmacro RequireInstallSuccess
      ${If} $2 == "${APPID}"
        Return
      ${EndIf}
    ${EndIf}
    Call ReleaseLeafGuard
  ${EndIf}
  ; A removed candidate can retain only its app-owned Writer identity. The
  ; ordinary uninstall list is gone; require both the exact retained locator
  ; and the original installation marker before treating this as our directory.
  ReadRegStr $0 HKCU "Software\${APPID}" "RetainedWriterInstallLocation"
  ${If} $0 == "$INSTDIR"
    StrCpy $GuardPath "$INSTDIR\${MARKER}"
    StrCpy $GuardReserve "0"
    Call PinLeaf
    ClearErrors
    FileOpen $1 "$INSTDIR\${MARKER}" r
    ${IfNot} ${Errors}
      FileRead $1 $2
      FileClose $1
      !insertmacro RequireInstallSuccess
      Call ReleaseLeafGuard
      !insertmacro RequireInstallSuccess
      ${If} $2 == "${APPID}"
        Return
      ${EndIf}
    ${EndIf}
    Call ReleaseLeafGuard
  ${EndIf}
  ; Native enumeration preserves the exact error too. WIN32_FIND_DATAW is
  ; 592 bytes; cFileName starts at byte 44 and holds 260 UTF-16 characters.
  System::Alloc 592
  Pop $8
  ${If} $8 P= 0
    !insertmacro RejectInstallDirectory "allocate-find-data" "8" "Directory verification buffer is unavailable."
  ${EndIf}
  System::Call 'kernel32::FindFirstFileW(w "$INSTDIR\*", p r8) p .r0 ?e'
  Pop $2
  ${If} $0 P= -1
    System::Free $8
    ; Do not infer an empty directory from an unsuccessful enumeration.
    !insertmacro RejectInstallDirectory "FindFirstFileW" "$2" "The existing directory cannot be enumerated. Choose a new folder."
  ${EndIf}
  ${Do}
    System::Call '*$8(&v44, &w260 .r1)'
    ${If} $1 != "."
    ${AndIf} $1 != ".."
      System::Call 'kernel32::FindClose(p r0) i .r3'
      System::Free $8
      !insertmacro RejectInstallDirectory "directory-policy" "n/a" "Choose a new or empty folder; this directory contains unverified materials."
    ${EndIf}
    System::Call 'kernel32::FindNextFileW(p r0, p r8) i .r3 ?e'
    Pop $2
    ${If} $3 == 0
      ; Preserve the enumeration status across cleanup calls.
      System::Call 'kernel32::FindClose(p r0) i .r3 ?e'
      Pop $4
      System::Free $8
      ${If} $2 != 18
        !insertmacro RejectInstallDirectory "FindNextFileW" "$2" "Directory verification did not finish."
      ${EndIf}
      ${If} $3 == 0
        !insertmacro RejectInstallDirectory "FindClose" "$4" "Directory verification handle could not be released."
      ${EndIf}
      Return
    ${EndIf}
  ${Loop}
FunctionEnd

; Pin every ancestor of each fixed bundled-resource directory before copying or
; deleting. Creation is segment-by-segment under an already pinned parent.
Function CheckResourceDirectory
  Pop $GuardTarget
  StrCpy $InstallStage "prepare-resource-directory"
  StrCpy $InstallPath $GuardTarget
  StrCpy $GuardCreate "1"
  Call PinDirectoryTree
FunctionEnd

Function un.CheckResourceDirectory
  Pop $GuardTarget
  StrCpy $GuardCreate "0"
  Call un.PinDirectoryTree
FunctionEnd

; Removes one owned data tree only when its leaf is exactly the application
; identifier, so a renamed or redirected path is skipped instead of followed.
; The source PDF and its annotations live outside these directories by design.
!macro RemoveOwnedDataDirectory NAME LABEL
  StrCpy $1 "${NAME}"
  ${If} $1 == "${APPID}"
    DetailPrint "Removing owned application data: $$LOCALAPPDATA\${NAME}"
    ClearErrors
    RMDir /r "$$LOCALAPPDATA\${NAME}"
    ${If} ${Errors}
      DetailPrint "Application data removal did not finish: ${LABEL}; remaining files were kept."
    ${EndIf}
  ${Else}
    DetailPrint "Skipped unexpected data directory name for ${LABEL}."
  ${EndIf}
!macroend

; The data-removal decision is its own uninstall component, default OFF, with
; its own text. Cancelling or leaving it unchecked keeps every local record and
; never touches the PDFs the user opened.
Function un.RemoveSelectedChatData
  ${If} $RemoveChatData != "1"
    DetailPrint "Local chat, settings and cache were retained by user choice."
    Return
  ${EndIf}
  MessageBox MB_YESNO|MB_ICONEXCLAMATION "Delete this reader's local data?$\r$\n$\r$\n$(^NameDA) will remove only its own data folders under:$\r$\n$LOCALAPPDATA and $APPDATA$\r$\n$\r$\nThis permanently removes local chat history, unsaved annotation recovery material, reader settings and rebuildable cache.$\r$\nSource PDF files and annotations already saved into them are NOT deleted.$\r$\n$\r$\nDelete local reader data now?" IDYES +2
  StrCpy $RemoveChatData "0"
  Return
  StrCpy $1 $LOCALAPPDATA
  StrCpy $2 $1 "" -36
  StrCpy $1 $APPDATA
  StrCpy $3 $1 "" -36
  ${If} $2 == "\${APPID}"
  ${AndIf} $3 == "\${APPID}"
    !insertmacro RemoveOwnedDataDirectory "${APPID}" "chat, drafts, settings and cache"
    !insertmacro RemoveOwnedDataDirectory "${APPID}.personal" "personal build data, if present"
  ${Else}
    DetailPrint "Unexpected data root; no application data was removed."
  ${EndIf}
FunctionEnd

; The whole WebView2 supply is inside one compile-time gate: with the baseline
; `skip` config this block produces no section, no string and no output at all.
!if "${INSTALLWEBVIEW2MODE}" == "offlineInstaller"
; Evergreen runtime registration, following Microsoft's documented discovery: the
; `pv` value under HKLM (WOW6432Node on x64 Windows) or HKCU must be present and
; non-zero, and `location` names the directory that holds the version subfolders.
!macro ReadWebView2Registration
  StrCpy $WebView2Version ""
  StrCpy $WebView2Location ""
  ${If} ${RunningX64}
    ReadRegStr $WebView2Version HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
    ReadRegStr $WebView2Location HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "location"
  ${Else}
    ReadRegStr $WebView2Version HKLM "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
    ReadRegStr $WebView2Location HKLM "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "location"
  ${EndIf}
  ${If} $WebView2Version == ""
    ReadRegStr $WebView2Version HKCU "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
    ReadRegStr $WebView2Location HKCU "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "location"
  ${EndIf}
  ${If} $WebView2Version == "0.0.0.0"
    StrCpy $WebView2Version ""
  ${EndIf}
!macroend

; A registration alone is not proof that the runtime is usable. Sandbox and
; restored system images can carry a `pv` value whose version directory is gone;
; the loader then fails with the very error this package exists to remove. The
; engine binary of the registered version must exist before it counts as present.
!macro ResolveWebView2Presence OUTPRESENT
  StrCpy ${OUTPRESENT} "0"
  ${If} $WebView2Version != ""
    ${If} $WebView2Location == ""
      StrCpy $WebView2Location "C:\Program Files (x86)\Microsoft\EdgeWebView\Application"
    ${EndIf}
    ${If} ${FileExists} "$WebView2Location\$WebView2Version\msedgewebview2.exe"
      StrCpy ${OUTPRESENT} "1"
    ${EndIf}
  ${EndIf}
!macroend

; Supplies the runtime the reader needs before any program file is written, so a
; missing or unusable runtime fails closed without leaving a half-installed
; application. The embedded installer is the only source; the exit code is
; recorded, and the registration plus the engine binary decide success or failure.
Section "WebView2"
  !insertmacro ReadWebView2Registration
  !insertmacro ResolveWebView2Presence $WebView2Present
  ${If} $WebView2Present == "1"
    DetailPrint "WebView2 runtime is already usable: $WebView2Version"
  ${Else}
    StrCpy $InstallStage "supply-webview2-runtime"
    StrCpy $InstallPath "Microsoft Edge WebView2 Runtime (embedded offline installer)"
    DetailPrint "WebView2 runtime not usable; registered='$WebView2Version' location='$WebView2Location'"
    Delete "$TEMP\MicrosoftEdgeWebView2RuntimeInstaller.exe"
    ClearErrors
    SetOutPath "$TEMP"
    File "/oname=$TEMP\MicrosoftEdgeWebView2RuntimeInstaller.exe" "${WEBVIEW2INSTALLERPATH}"
    !insertmacro RequireInstallSuccess
    ExecWait '"$TEMP\MicrosoftEdgeWebView2RuntimeInstaller.exe" ${WEBVIEW2INSTALLERARGS} /install' $WebView2ExitCode
    ${If} ${Errors}
      StrCpy $WebView2ExitCode "not-started"
    ${EndIf}
    !insertmacro ReadWebView2Registration
    !insertmacro ResolveWebView2Presence $WebView2Present
    DetailPrint "WebView2 offline installer exit code: $WebView2ExitCode; registered version: $WebView2Version; engine present: $WebView2Present"
    ${If} $WebView2Present != "1"
      MessageBox MB_ICONSTOP "The embedded WebView2 runtime could not be installed or registered.$\r$\nInstaller exit code: $WebView2ExitCode$\r$\nRegistered version: $WebView2Version$\r$\nNo program files were written. The reader cannot run without this runtime."
      SetErrorLevel 1
      Abort
    ${EndIf}
  ${EndIf}
SectionEnd
!endif

Section "Reader"
  StrCpy $InstallStage "prepare-install-directory"
  StrCpy $InstallPath $INSTDIR
  StrCpy $GuardTarget $INSTDIR
  StrCpy $GuardCreate "1"
  Call PinDirectoryTree
  StrCpy $INSTDIR $GuardTarget
  Call CheckInstallDir
  SetShellVarContext current
  StrCpy $GuardTarget $SMPROGRAMS
  Call PinDirectoryTree
  StrCpy $RootGuards $DirectoryGuards
  StrCpy $DirectoryGuards "0"
  {{#each resources_dirs}}
  Push "$INSTDIR\\{{this}}"
  Call CheckResourceDirectory
  {{/each}}
  SetShellVarContext current
  StrCpy $InstallStage "set-output-directory"
  StrCpy $InstallPath $INSTDIR
  ClearErrors
  SetOutPath "$INSTDIR"
  !insertmacro RequireInstallSuccess
  !insertmacro BeginGuardedWrite "$INSTDIR\${BINARY}.exe"
  File /oname=${BINARY}.exe "{{no-escape main_binary_path}}"
  !insertmacro EndGuardedWrite
  ; Tauri supplies a finite, validated resource inventory from the pinned cache.
  ; CheckResourceDirectory already created and pinned all required directories.
  {{#each resources}}
  !insertmacro BeginGuardedWrite "$INSTDIR\\{{this.[1]}}"
  File /a "/oname={{this.[1]}}" "{{no-escape @key}}"
  !insertmacro EndGuardedWrite
  {{/each}}
  !insertmacro BeginGuardedWrite "$INSTDIR\uninstall.exe"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  !insertmacro EndGuardedWrite
  !insertmacro BeginGuardedWrite "$INSTDIR\${MARKER}"
  FileOpen $0 "$INSTDIR\${MARKER}" w
  !insertmacro RequireInstallSuccess
  FileWrite $0 "${APPID}"
  FileClose $0
  !insertmacro EndGuardedWrite
  StrCpy $GuardPath "$INSTDIR\${MARKER}"
  StrCpy $GuardReserve "0"
  Call PinLeaf
  StrCpy $InstallStage "verify-installation-marker"
  StrCpy $InstallPath $GuardPath
  ClearErrors
  FileOpen $0 "$INSTDIR\${MARKER}" r
  !insertmacro RequireInstallSuccess
  FileRead $0 $1
  FileClose $0
  !insertmacro RequireInstallSuccess
  Call ReleaseLeafGuard
  !insertmacro RequireInstallSuccess
  ${If} $1 != "${APPID}"
    MessageBox MB_ICONSTOP "Installation identity could not be verified. Partial materials were retained."
    SetErrorLevel 1
    Abort
  ${EndIf}
  !insertmacro BeginGuardedWrite "$SMPROGRAMS\${PRODUCT}.lnk"
  CreateShortcut "$SMPROGRAMS\${PRODUCT}.lnk" "$INSTDIR\${BINARY}.exe"
  !insertmacro EndGuardedWrite
  ; Register a candidate only. Never set .pdf's default or UserChoice.
  StrCpy $InstallStage "register-current-user-candidate"
  StrCpy $InstallPath "HKCU application candidate and uninstall metadata"
  ClearErrors
  WriteRegStr HKCU "Software\Classes\${PROGID}" "" "${PRODUCT} PDF"
  WriteRegStr HKCU "Software\Classes\${PROGID}\DefaultIcon" "" '$"$INSTDIR\${BINARY}.exe$",0'
  WriteRegStr HKCU "Software\Classes\${PROGID}\shell\open\command" "" '$"$INSTDIR\${BINARY}.exe$" $"%1$"'
  WriteRegStr HKCU "Software\Classes\.pdf\OpenWithProgids" "${PROGID}" ""
  WriteRegStr HKCU "Software\Classes\Applications\${BINARY}.exe\SupportedTypes" ".pdf" ""
  WriteRegStr HKCU "Software\Classes\Applications\${BINARY}.exe\shell\open\command" "" '$"$INSTDIR\${BINARY}.exe$" $"%1$"'
  WriteRegStr HKCU "Software\${APPID}\Capabilities" "ApplicationName" "${PRODUCT}"
  WriteRegStr HKCU "Software\${APPID}\Capabilities\FileAssociations" ".pdf" "${PROGID}"
  WriteRegStr HKCU "Software\RegisteredApplications" "${PRODUCT}" "Software\${APPID}\Capabilities"
  WriteRegStr HKCU "${UNINSTALL}" "DisplayName" "${PRODUCT}"
  WriteRegStr HKCU "${UNINSTALL}" "DisplayVersion" "{{version}}"
  WriteRegStr HKCU "${UNINSTALL}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL}" "UninstallString" '$"$INSTDIR\uninstall.exe$"'
  WriteRegDWORD HKCU "${UNINSTALL}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL}" "NoRepair" 1
  !insertmacro RequireInstallSuccess
  ReadRegStr $0 HKCU "${UNINSTALL}" "InstallLocation"
  ${If} $0 != "$INSTDIR"
    MessageBox MB_ICONSTOP "Installation location could not be verified. Partial program materials were retained."
    SetErrorLevel 1
    Abort
  ${EndIf}
  ReadRegStr $0 HKCU "Software\${APPID}" "RetainedWriterInstallLocation"
  ${If} $0 == "$INSTDIR"
    StrCpy $InstallStage "retire-reinstall-locator"
    StrCpy $InstallPath "HKCU\Software\${APPID}\RetainedWriterInstallLocation"
    System::Call 'advapi32::RegOpenKeyExW(p 0x80000001, w "Software\${APPID}", i 0, i 2, *p .r2) i .r3'
    ${If} $3 == 0
      System::Call 'advapi32::RegDeleteValueW(p r2, w "RetainedWriterInstallLocation") i .r3'
      System::Call 'advapi32::RegCloseKey(p r2) i .r2'
      ${If} $2 != 0
        SetErrors
        !insertmacro RequireInstallSuccess
      ${EndIf}
    ${EndIf}
    ${If} $3 != 0
    ${AndIf} $3 != 2
    ${AndIf} $3 != 3
      SetErrors
      !insertmacro RequireInstallSuccess
    ${EndIf}
  ${EndIf}
  ClearErrors
  StrCpy $InstallStage "release-installation-guards"
  StrCpy $InstallPath $INSTDIR
  Call ReleaseAllGuards
  !insertmacro RequireInstallSuccess
SectionEnd

Section "Uninstall"
  SetShellVarContext current
  {{#each resources_dirs}}
  Push "$INSTDIR\\{{this}}"
  Call un.CheckResourceDirectory
  {{/each}}
  ; Only the exact application-owned data directory is ever removed, and only
  ; after the user ticks the separate uninstall choice. The source PDF, its
  ; annotations and every other file are never touched.
  Call un.RemoveSelectedChatData
  ; The byte writer is now the renderer's MuPDF.js/WASM module, so this build no
  ; longer ships or depends on the Python worker. No writer marker is retained.
  StrCpy $RetainedWriter "0"
  ; Fail before unregistering when an occupied/denied program cannot be removed.
  !insertmacro CheckUninstallLeaf "$INSTDIR\${BINARY}.exe"
  ClearErrors
  Delete "$INSTDIR\${BINARY}.exe"
  !insertmacro RequireUninstallSuccess
  ; Remove only exact program resources, never walk data or unknown files.
  {{#each resources}}
  !insertmacro CheckUninstallLeaf "$INSTDIR\\{{this.[1]}}"
  ClearErrors
  Delete "$INSTDIR\\{{this.[1]}}"
  !insertmacro RequireUninstallSuccess
  {{/each}}
  ; Release resource ancestors only after all exact resource deletes. Root and
  ; menu ancestor guards remain active until the remaining owned files are gone.
  ClearErrors
  Call un.ReleaseDirectoryGuards
  !insertmacro RequireUninstallSuccess
  {{#each resources_ancestors}}
  RMDir "$INSTDIR\\{{this}}"
  {{/each}}
  StrCpy $1 '$"$INSTDIR\${BINARY}.exe$" $"%1$"'
  ; Remove only this binary and its own shortcut/registry candidate. Never scan
  ; locators, SQLite, settings, drafts, backups, custom data directories or PDFs.
  ReadRegStr $0 HKCU "Software\Classes\${PROGID}\shell\open\command" ""
  ${If} $0 == $1
    ClearErrors
    !insertmacro RemoveOwnedValue "Software\Classes\.pdf\OpenWithProgids" "${PROGID}"
    !insertmacro RemoveOwnedKey "Software\${APPID}\Capabilities"
    !insertmacro RemoveOwnedValue "Software\RegisteredApplications" "${PRODUCT}"
    !insertmacro RemoveOwnedKey "Software\Classes\${PROGID}"
    !insertmacro RequireUninstallSuccess
  ${EndIf}
  ReadRegStr $0 HKCU "Software\Classes\Applications\${BINARY}.exe\shell\open\command" ""
  ${If} $0 == $1
    ClearErrors
    !insertmacro RemoveOwnedKey "Software\Classes\Applications\${BINARY}.exe"
    !insertmacro RequireUninstallSuccess
  ${EndIf}
  ReadRegStr $0 HKCU "${UNINSTALL}" "InstallLocation"
  ${If} $0 == "$INSTDIR"
    !insertmacro CheckUninstallLeaf "$SMPROGRAMS\${PRODUCT}.lnk"
    ClearErrors
    Delete "$SMPROGRAMS\${PRODUCT}.lnk"
    !insertmacro RequireUninstallSuccess
    !insertmacro RemoveOwnedKey "${UNINSTALL}"
  ${EndIf}
  ; Preserve the root identity only for the explicitly retained app-owned state.
  ; Do this before deleting the uninstaller so registry failure remains retryable.
  ${If} $RetainedWriter == "1"
    ClearErrors
    WriteRegStr HKCU "Software\${APPID}" "RetainedWriterInstallLocation" "$INSTDIR"
    !insertmacro RequireUninstallSuccess
    ReadRegStr $0 HKCU "Software\${APPID}" "RetainedWriterInstallLocation"
    ${If} $0 != "$INSTDIR"
      MessageBox MB_ICONSTOP "Retained Writer location could not be verified. Remaining materials were preserved."
      SetErrorLevel 1
      Abort
    ${EndIf}
  ${EndIf}
  ; Retain the uninstaller and identity until earlier retryable cleanup succeeds.
  !insertmacro CheckUninstallLeaf "$INSTDIR\uninstall.exe"
  ClearErrors
  Delete "$INSTDIR\uninstall.exe"
  !insertmacro RequireUninstallSuccess
  ${If} $RetainedWriter == "0"
    !insertmacro CheckUninstallLeaf "$INSTDIR\${MARKER}"
    ClearErrors
    Delete "$INSTDIR\${MARKER}"
    !insertmacro RequireUninstallSuccess
    ClearErrors
    Call un.ReleaseAllGuards
    !insertmacro RequireUninstallSuccess
    RMDir "$INSTDIR"
  ${Else}
    DetailPrint "Writer isolation identity and its location were retained for controlled reinstallation; all user data remains unchanged."
  ${EndIf}
  ClearErrors
  Call un.ReleaseAllGuards
  !insertmacro RequireUninstallSuccess
  ; A non-empty directory is deliberately retained, never recursively removed.
  ; No recursive deletion and no data-deletion checkbox in this candidate.
  ; Separate explicitly-authorized data removal is still a later acceptance item.
SectionEnd
