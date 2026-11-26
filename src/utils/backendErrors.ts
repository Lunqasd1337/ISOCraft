import i18n from "../i18n";

const EXACT_BACKEND_ERRORS: Record<
  string,
  () => string
> = {
  "Operation cancelled by user.": () => i18n.t("backendErrors.operationCancelled"),
  "Another image operation is currently running.": () =>
    i18n.t("backendErrors.anotherImageOpRunning"),
  "Another image or download operation is currently running.": () =>
    i18n.t("backendErrors.anotherImageOrDownloadRunning"),
  "Another build or image operation is already running.": () =>
    i18n.t("backendErrors.anotherBuildRunning"),
  "Another build or image operation is currently running.": () =>
    i18n.t("backendErrors.anotherBuildRunning"),
  "Cannot recover workspace while an image operation is running.": () =>
    i18n.t("backendErrors.cannotRecoverWhileBusy"),
  "Cannot clear workspace cache while an image operation is running.": () =>
    i18n.t("backendErrors.cannotClearCacheWhileBusy"),
  "Cannot clear workspace storage while mounts or registry hives are active. Run workspace recovery first.": () =>
    i18n.t("backendErrors.cannotClearStorageDirtyWorkspace"),
  "Cannot install application update while mounts or registry hives are active. Run workspace recovery first.": () =>
    i18n.t("backendErrors.cannotUpdateDirtyWorkspace"),
  "Workspace has active WIM mounts or loaded registry hives. Run workspace recovery before starting a build.": () =>
    i18n.t("backendErrors.buildDirtyWorkspace"),
  "Cannot change workspace directory while an image operation is running.": () =>
    i18n.t("backendErrors.cannotChangeWorkspaceWhileBusy"),
  "Selected oscdimg.exe must be a valid Microsoft-signed binary inside a protected Program Files directory.": () =>
    i18n.t("backendErrors.oscdimgUntrusted"),
  "Timed out waiting for active operation to stop; retry cancellation or workspace recovery.": () =>
    i18n.t("backendErrors.cancelTimeout"),
  "No application update is currently available.": () =>
    i18n.t("backendErrors.noAppUpdateAvailable"),
  "explorer.exe not found in Windows directory.": () =>
    i18n.t("backendErrors.explorerNotFound"),
  "Settings lock poisoned": () => i18n.t("backendErrors.settingsLockPoisoned"),
  "Unsupported external URL.": () => i18n.t("backendErrors.unsupportedExternalUrl"),
  "Selected edition index must be 1 or greater.": () =>
    i18n.t("backendErrors.invalidEditionIndex"),
  "Source image path must not be empty.": () =>
    i18n.t("backendErrors.sourcePathEmpty"),
  "Output path must not be empty.": () => i18n.t("backendErrors.outputPathEmpty"),
  "Output path cannot be placed inside temporary workspace session_current directory.": () =>
    i18n.t("backendErrors.outputInsideSessionCurrent"),
  "Output path must not overwrite the source image file.": () =>
    i18n.t("backendErrors.outputOverwritesSource"),
  "Invalid workspace directory: path cannot be empty.": () =>
    i18n.t("backendErrors.workspaceDirEmpty"),
  "Appx package path cannot be empty.": () =>
    i18n.t("backendErrors.appxPathEmpty"),
  "dism.exe not found.": () => i18n.t("backendErrors.dismNotFound"),
  "pnputil.exe not found in Windows directory.": () =>
    i18n.t("backendErrors.pnputilNotFound"),
  "wimlib-imagex.exe is missing.": () => i18n.t("backendErrors.wimlibMissing"),
  "ISO output requires an .iso source image with bootloaders.": () =>
    i18n.t("backendErrors.isoRequiresIsoSource"),
  "Neither oscdimg.exe nor xorriso.exe is available for ISO creation.": () =>
    i18n.t("backendErrors.noIsoBuilderAvailable"),
  "Neither oscdimg.exe nor xorriso.exe is available to build the ISO.": () =>
    i18n.t("backendErrors.noIsoBuilderAvailable"),
  "sources\\boot.wim not found in ISO; cannot inject Setup drivers.": () =>
    i18n.t("backendErrors.bootWimNotFound"),
  "No image indices found in WIM/ESD metadata.": () =>
    i18n.t("backendErrors.noImageIndices"),
  "No .inf driver files found in the selected folder.": () =>
    i18n.t("backendErrors.driverNoInfFound"),
  "No host drivers selected for export.": () =>
    i18n.t("backendErrors.driverExportNoneSelected"),
  "Driver export destination path must not be empty.": () =>
    i18n.t("backendErrors.driverExportDestEmpty"),
  "Failed exporting any selected host drivers.": () =>
    i18n.t("backendErrors.driverExportAllFailed"),
  "Selected folder contains no .msu or .cab update packages.": () =>
    i18n.t("backendErrors.updateNoPackagesFound"),
  "Search query must not be empty.": () =>
    i18n.t("backendErrors.catalogQueryEmpty"),
  "UNC network paths are not allowed as a destination path.": () =>
    i18n.t("backendErrors.fileUncNotAllowed"),
  "Drive letters or ':' characters are not allowed in destination path.": () =>
    i18n.t("backendErrors.fileDriveLetterNotAllowed"),
  "Destination path contains an empty path segment.": () =>
    i18n.t("backendErrors.fileEmptySegment"),
  "Custom file or folder path must not be empty.": () =>
    i18n.t("backendErrors.filePathEmpty"),
  "Winget package ID must not be empty.": () =>
    i18n.t("backendErrors.postSetupWingetEmpty"),
  "Post-Setup arguments must not contain newline or NUL characters.": () =>
    i18n.t("backendErrors.postSetupArgsUnsafe"),
  "Post-Setup file path must not be empty.": () =>
    i18n.t("backendErrors.postSetupPathEmpty"),
  "Post-Setup local file path must not be empty.": () =>
    i18n.t("backendErrors.postSetupPathEmpty"),
  "Winget packages can only run during the First Logon (User) phase.": () =>
    i18n.t("backendErrors.postSetupWingetPhase"),
  "Post-Setup FirstLogonCommands (autounattend.xml) requires ISO output target.": () =>
    i18n.t("backendErrors.postSetupAutounattendRequiresIso"),
};

const EXACT_DISM_HINTS: Record<string, () => string> = {
  "Windows 11 24H2+ Checkpoint Cumulative Update prerequisite missing (0x800f0993 / 0x80073712): place the baseline Checkpoint .msu and target cumulative .msu in the same folder or queue them together so DISM can resolve the update chain.":
    () => i18n.t("backendErrors.dismHintCheckpoint"),
  "Update package is not applicable to this Windows 11 image (0x800f081e): verify build, edition, and x64 architecture.":
    () => i18n.t("backendErrors.dismHintNotApplicable"),
  "Update package requires a newer Servicing Stack Update (SSU) to be integrated first (0x800f0823).":
    () => i18n.t("backendErrors.dismHintSsuRequired"),
  "The offline image has pending CBS servicing operations (0x800f0806) that will complete on first boot.":
    () => i18n.t("backendErrors.dismHintPendingCbs"),
};

const REGEX_BACKEND_ERRORS: Array<[RegExp, (m: RegExpMatchArray) => string]> = [
  [
    /^Failed to open browser: ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.failedOpenBrowser", { details: m[1] }),
  ],
  [
    /^Failed to spawn '([^']+)': ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.failedSpawnProcess", { program: m[1], details: m[2] }),
  ],
  [
    /^Failed waiting for '([^']+)': ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.failedWaitProcess", { program: m[1], details: m[2] }),
  ],
  [
    /^Failed creating session directory: ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.failedCreateSessionDir", { details: m[1] }),
  ],
  [
    /^Image file not found: ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.imageFileNotFound", { path: m[1] }),
  ],
  [
    /^Failed to parse WIM XML metadata: ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.failedParseWimXml", { details: m[1] }),
  ],
  [
    /^Failed to execute wimlib-imagex: ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.failedRunWimlib", { details: m[1] }),
  ],
  [
    /^Driver path does not exist: ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.driverPathNotFound", { path: m[1] }),
  ],
  [
    /^Symbolic links and junctions are not supported for driver paths: ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.driverSymlinkUnsupported", { path: m[1] }),
  ],
  [
    /^Unsupported driver file '([^']+)'\. Expected \.inf file\.$/,
    (m) => i18n.t("backendErrors.driverUnsupportedFile", { path: m[1] }),
  ],
  [
    /^Failed reading driver folder '([^']+)': ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.driverReadFolderFailed", { path: m[1], details: m[2] }),
  ],
  [
    /^Invalid OEM driver published name '([^']*)'\. Expected format like oem12\.inf\.$/,
    (m) => i18n.t("backendErrors.driverInvalidPublishedName", { name: m[1] }),
  ],
  [
    /^Network\/UNC or Windows device paths are not allowed for driver export destination: '([^']+)'$/,
    (m) => i18n.t("backendErrors.driverExportUncNotAllowed", { path: m[1] }),
  ],
  [
    /^Driver export destination must be an absolute path: '([^']+)'$/,
    (m) => i18n.t("backendErrors.driverExportNotAbsolute", { path: m[1] }),
  ],
  [
    /^Symbolic links and junctions are not supported for driver export destination: '([^']+)'$/,
    (m) => i18n.t("backendErrors.driverExportSymlinkUnsupported", { path: m[1] }),
  ],
  [
    /^Driver export destination '([^']+)' points to an existing file\.$/,
    (m) => i18n.t("backendErrors.driverExportIsFile", { path: m[1] }),
  ],
  [
    /^Driver export destination cannot reside inside temporary workspace session_current: '([^']+)'$/,
    (m) => i18n.t("backendErrors.driverExportInsideSession", { path: m[1] }),
  ],
  [
    /^Drive root cannot be used as driver export destination: '([^']+)'$/,
    (m) => i18n.t("backendErrors.driverExportDriveRoot", { path: m[1] }),
  ],
  [
    /^Windows directory cannot be used as driver export destination: '([^']+)'$/,
    (m) => i18n.t("backendErrors.driverExportWindowsDir", { path: m[1] }),
  ],
  [
    /^Invalid Winget package ID '([^']*)': ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.postSetupWingetInvalid", { id: m[1] }),
  ],
  [
    /^Winget package ID '([^']*)' is too long\.$/,
    (m) => i18n.t("backendErrors.postSetupWingetInvalid", { id: m[1] }),
  ],
  [
    /^Post-Setup file does not exist: '([^']+)'$/,
    (m) => i18n.t("backendErrors.postSetupFileNotFound", { path: m[1] }),
  ],
  [
    /^Symbolic links and junctions are not supported for Post-Setup files: '([^']+)'$/,
    (m) => i18n.t("backendErrors.postSetupSymlinkUnsupported", { path: m[1] }),
  ],
  [
    /^Post-Setup item must be a regular file: '([^']+)'$/,
    (m) => i18n.t("backendErrors.postSetupNotRegularFile", { path: m[1] }),
  ],
  [
    /^Unsupported Post-Setup file '([^']+)'\. Expected \.exe, \.msi, \.ps1, \.cmd, or \.bat\.$/,
    (m) => i18n.t("backendErrors.postSetupUnsupportedFile", { path: m[1] }),
  ],
  [
    /^Update path does not exist: ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.updatePathNotFound", { path: m[1] }),
  ],
  [
    /^Symbolic links and junctions are not supported for update paths: ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.updateSymlinkUnsupported", { path: m[1] }),
  ],
  [
    /^Unsupported update file '([^']+)'\. Expected \.msu or \.cab\.$/,
    (m) => i18n.t("backendErrors.updateUnsupportedFile", { path: m[1] }),
  ],
  [
    /^Failed reading update folder '([^']+)': ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.updateReadFolderFailed", { path: m[1], details: m[2] }),
  ],
  [
    /^Microsoft Update Catalog search failed \(exit (-?\d+)\): ([\s\S]*)$/,
    (m) => i18n.t("backendErrors.catalogSearchFailed", { code: m[1], details: m[2] }),
  ],
  [
    /^Failed resolving catalog download links for ([^ ]+) \(exit (-?\d+)\): ([\s\S]*)$/,
    (m) =>
      i18n.t("backendErrors.catalogResolveLinksFailed", {
        guid: m[1],
        code: m[2],
        details: m[3],
      }),
  ],
  [
    /^No downloadable \.msu or \.cab links were returned by Microsoft Update Catalog for ([^.]+)\.$/,
    (m) => i18n.t("backendErrors.catalogNoLinksReturned", { guid: m[1] }),
  ],
  [
    /^Destination path contains invalid Windows path character: '([\s\S])'\.$/,
    (m) => i18n.t("backendErrors.fileInvalidChar", { char: m[1] }),
  ],
  [
    /^Path traversal or invalid segment '([^']+)' is not allowed in destination path\.$/,
    (m) => i18n.t("backendErrors.fileInvalidSegment", { segment: m[1] }),
  ],
  [
    /^Invalid non-relative component in destination path '([^']+)'\.$/,
    (m) => i18n.t("backendErrors.fileNonRelativeComponent", { path: m[1] }),
  ],
  [
    /^Protected system path cannot be modified via file copy: '([^']+)'\. Use the Registry screen instead\.$/,
    (m) => i18n.t("backendErrors.fileProtectedSystemHive", { path: m[1] }),
  ],
  [
    /^Protected Default user hive cannot be modified via file copy: '([^']+)'\. Use the Registry screen instead\.$/,
    (m) => i18n.t("backendErrors.fileProtectedDefaultUserHive", { path: m[1] }),
  ],
  [
    /^Protected ISO boot\/image file cannot be overwritten: '([^']+)'\.$/,
    (m) => i18n.t("backendErrors.fileProtectedIsoFile", { path: m[1] }),
  ],
  [
    /^Protected ISO split image file cannot be overwritten: '([^']+)'\.$/,
    (m) => i18n.t("backendErrors.fileProtectedIsoSwm", { path: m[1] }),
  ],
  [
    /^Path does not exist: ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.filePathNotFound", { path: m[1] }),
  ],
  [
    /^Failed inspecting '([^']+)': ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.fileInspectFailed", { path: m[1], details: m[2] }),
  ],
  [
    /^Failed reading folder '([^']+)': ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.fileReadFolderFailed", { path: m[1], details: m[2] }),
  ],
  [
    /^Failed reading preset '([^']+)': ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.presetReadFailed", { path: m[1], details: m[2] }),
  ],
  [
    /^Invalid preset JSON '([^']+)': ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.presetInvalidJson", { path: m[1], details: m[2] }),
  ],
  [
    /^Unsupported preset schemaVersion '([^']*)'; expected '([^']*)'\.$/,
    (m) =>
      i18n.t("backendErrors.presetUnsupportedSchema", {
        actual: m[1],
        expected: m[2],
      }),
  ],
  [
    /^Failed creating preset folder '([^']+)': ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.presetCreateDirFailed", { path: m[1], details: m[2] }),
  ],
  [
    /^Failed serializing JSON preset: ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.presetSerializeFailed", { details: m[1] }),
  ],
  [
    /^Failed writing preset '([^']+)': ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.presetWriteFailed", { path: m[1], details: m[2] }),
  ],
  [
    /^Failed to create settings directory: ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.settingsCreateDirFailed", { details: m[1] }),
  ],
  [
    /^Failed to initialize workspace directory: ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.settingsInitWorkspaceFailed", { details: m[1] }),
  ],
  [
    /^Failed to serialize settings: ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.settingsSerializeFailed", { details: m[1] }),
  ],
  [
    /^Failed to write settings file: ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.settingsWriteFailed", { details: m[1] }),
  ],
  [
    /^Failed to create destination folder: ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.diagCreateDirFailed", { details: m[1] }),
  ],
  [
    /^Failed to write diagnostics file: ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.diagWriteFailed", { details: m[1] }),
  ],
  [
    /^Invalid workspace directory '([^']+)': symbolic links and reparse points are not allowed\.$/,
    (m) => i18n.t("backendErrors.workspaceSymlinkNotAllowed", { path: m[1] }),
  ],
  [
    /^Invalid workspace directory '([^']+)': ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.workspaceDirInvalid", { path: m[1], details: m[2] }),
  ],
  [
    /^Network\/UNC or Windows device paths are not allowed for ([^:]+): '([^']+)'$/,
    (m) => i18n.t("backendErrors.uncPathNotAllowedFor", { label: m[1], path: m[2] }),
  ],
  [
    /^UNC network or device paths are not allowed for '([^']+)'\.?$/,
    (m) => i18n.t("backendErrors.uncPathNotAllowed", { path: m[1] }),
  ],
  [
    /^(.+?) cannot reside inside temporary workspace session_current: '([^']+)'$/,
    (m) => i18n.t("backendErrors.inputInsideSessionCurrent", { label: m[1], path: m[2] }),
  ],
  [
    /^(.+?) directory does not exist: '([^']+)'$/,
    (m) => i18n.t("backendErrors.dirNotFound", { label: m[1], path: m[2] }),
  ],
  [
    /^(.+?) must be a regular non-symlink directory: '([^']+)'$/,
    (m) => i18n.t("backendErrors.regularDirRequired", { label: m[1], path: m[2] }),
  ],
  [
    /^Unsafe (?:downloads directory|updates download folder) '([^']+)': symbolic links and reparse points are not allowed\.$/,
    (m) => i18n.t("backendErrors.unsafeDownloadsDir", { path: m[1] }),
  ],
  [
    /^Downloaded update '([^']+)' failed Microsoft Authenticode signature verification\.$/,
    (m) => i18n.t("backendErrors.updateAuthenticodeFailed", { file: m[1] }),
  ],
  [
    /^HTTP (\d+) redirect missing Location header from '([^']+)'\.$/,
    (m) => i18n.t("backendErrors.redirectMissingLocation", { status: m[1], url: m[2] }),
  ],
  [
    /^Too many redirects resolving download URL '([^']+)'\.$/,
    (m) => i18n.t("backendErrors.tooManyRedirects", { url: m[1] }),
  ],
  [
    /^Source image does not exist: '([^']+)'$/,
    (m) => i18n.t("backendErrors.imageFileNotFound", { path: m[1] }),
  ],
  [
    /^Source image must be a regular non-symlink file: '([^']+)'$/,
    (m) => i18n.t("backendErrors.sourceSymlinkUnsupported", { path: m[1] }),
  ],
  [
    /^Refusing to overwrite symlink or reparse point at '([^']+)'\.?$/,
    (m) => i18n.t("backendErrors.symlinkOverwriteBlocked", { path: m[1] }),
  ],
  [
    /^Output path '([^']+)' points to an existing directory\.$/,
    (m) => i18n.t("backendErrors.outputIsDirectory", { path: m[1] }),
  ],
  [
    /^Invalid autounattend\.xml: ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.invalidAutounattend", { details: m[1] }),
  ],
  [
    /^Symbolic links and reparse points are not allowed for '([^']+)'\.$/,
    (m) => i18n.t("backendErrors.symlinkNotAllowed", { path: m[1] }),
  ],
  [
    /^File '([^']+)' \((\d+) bytes\) exceeds maximum allowed size of (\d+) bytes\.$/,
    (m) => i18n.t("backendErrors.fileTooLarge", { path: m[1], maxBytes: m[3] }),
  ],
  [
    /^Blocked download redirect to untrusted host: '([^']+)'\.$/,
    (m) => i18n.t("backendErrors.blockedUntrustedRedirect", { url: m[1] }),
  ],
  [
    /^Appx package file does not exist: '([^']+)'$/,
    (m) => i18n.t("backendErrors.appxPackageNotFound", { path: m[1] }),
  ],
  [
    /^Invalid Appx\/MSIX package file: '([^']+)'$/,
    (m) => i18n.t("backendErrors.appxPackageInvalid", { path: m[1] }),
  ],
  [
    /^Appx dependency file does not exist: '([^']+)'$/,
    (m) => i18n.t("backendErrors.appxDependencyNotFound", { path: m[1] }),
  ],
  [
    /^Invalid Appx dependency file: '([^']+)'$/,
    (m) => i18n.t("backendErrors.appxDependencyInvalid", { path: m[1] }),
  ],
  [
    /^Appx license file does not exist: '([^']+)'$/,
    (m) => i18n.t("backendErrors.appxLicenseNotFound", { path: m[1] }),
  ],
  [
    /^Invalid Appx XML license file: '([^']+)'$/,
    (m) => i18n.t("backendErrors.appxLicenseInvalid", { path: m[1] }),
  ],
  [
    /^Syntax error\(s\) in '([^']+)': ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.regSyntaxErrors", { path: m[1], details: m[2] }),
  ],
  [
    /^Failed initializing updater: ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.updaterInitFailed", { details: m[1] }),
  ],
  [
    /^Failed checking for updates: ([\s\S]+)$/,
    (m) => i18n.t("backendErrors.updaterCheckFailed", { details: m[1] }),
  ],
  [
    /^Failed reading '([^']+)': ([\s\S]+)$/,
    (m) => i18n.t("regMessages.failedReadFile", { path: m[1], details: m[2] }),
  ],
];

function translateMainBackendError(main: string): string {
  const exact = EXACT_BACKEND_ERRORS[main];
  if (exact) return exact();

  for (const [pattern, format] of REGEX_BACKEND_ERRORS) {
    const match = main.match(pattern);
    if (match) return format(match);
  }
  return main;
}

export function localizeBackendError(raw: string): string {
  const hintMarker = "\nHint: ";
  const hintIdx = raw.indexOf(hintMarker);
  if (hintIdx === -1) {
    return translateMainBackendError(raw);
  }

  const mainPart = raw.slice(0, hintIdx);
  const rawHint = raw.slice(hintIdx + hintMarker.length);
  const hintFn = EXACT_DISM_HINTS[rawHint];
  const translatedMain = translateMainBackendError(mainPart);
  const translatedHint = hintFn ? hintFn() : rawHint;

  return `${translatedMain}\n${i18n.t("backendErrors.hintPrefix", {
    hint: translatedHint,
  })}`;
}

export function localizeXmlError(raw: string): string {
  if (raw === "XML file contains no root element.") {
    return i18n.t("xmlErrors.noRootElement");
  }

  let match = raw.match(/^Failed to read file: ([\s\S]+)$/);
  if (match) return i18n.t("xmlErrors.readFailed", { details: match[1] });

  match = raw.match(/^Expected root XML element <unattend>, found <([^>]+)>\.$/);
  if (match) return i18n.t("xmlErrors.expectedUnattendRoot", { tag: match[1] });

  match = raw.match(/^Mismatched closing tag <\/([^>]+)>, expected <\/([^>]+)>\.$/);
  if (match) {
    return i18n.t("xmlErrors.mismatchedClosingTag", {
      actual: match[1],
      expected: match[2],
    });
  }

  match = raw.match(/^Unexpected closing tag <\/([^>]+)>\.$/);
  if (match) return i18n.t("xmlErrors.unexpectedClosingTag", { tag: match[1] });

  match = raw.match(/^Invalid XML syntax: ([\s\S]+)$/);
  if (match) return i18n.t("xmlErrors.invalidSyntax", { details: match[1] });

  match = raw.match(/^Unclosed XML element\(s\): ([\s\S]+)$/);
  if (match) return i18n.t("xmlErrors.unclosedElements", { tags: match[1] });

  return translateMainBackendError(raw);
}

const EXACT_REG_MESSAGES: Record<string, () => string> = {
  "REG_LINK (hex(6)) values are not allowed in offline registry files.": () =>
    i18n.t("regMessages.regLinkNotAllowed"),
  "Registry file is empty or has no valid header.": () =>
    i18n.t("regMessages.emptyOrNoHeader"),
  "Missing or unsupported .reg header (expected 'Windows Registry Editor Version 5.00').":
    () => i18n.t("regMessages.missingHeader"),
  "Registry key path contains an embedded NUL character": () =>
    i18n.t("regMessages.keyContainsNul"),
  "Empty registry key path": () => i18n.t("regMessages.emptyKeyPath"),
  "Registry value defined outside of any [Key] section.": () =>
    i18n.t("regMessages.valueOutsideSection"),
  "Expected '=' after '@' default value.": () =>
    i18n.t("regMessages.expectedEqualsAfterAt"),
  "Registry value name contains an embedded NUL character.": () =>
    i18n.t("regMessages.valueNameContainsNul"),
  "Dangling escape '\\' inside quoted string": () =>
    i18n.t("regMessages.danglingEscape"),
  "Expected opening quote '\"'": () => i18n.t("regMessages.expectedOpeningQuote"),
};

const REGEX_REG_MESSAGES: Array<[RegExp, (m: RegExpMatchArray) => string]> = [
  [
    /^Unclosed registry section header '([\s\S]+)'\.$/,
    (m) => i18n.t("regMessages.unclosedSection", { header: m[1] }),
  ],
  [
    /^Skipped unsupported offline root '([\s\S]+)'\.$/,
    (m) => i18n.t("regMessages.skippedUnsupportedRoot", { root: m[1] }),
  ],
  [
    /^Expected '=' after value name "([\s\S]*)"\.$/,
    (m) => i18n.t("regMessages.expectedEqualsAfterName", { name: m[1] }),
  ],
  [
    /^Invalid registry entry '([\s\S]+)'\.$/,
    (m) => i18n.t("regMessages.invalidEntry", { entry: m[1] }),
  ],
  [
    /^Invalid hex byte '([^']*)' in hex list$/,
    (m) => i18n.t("regMessages.invalidHexByte", { token: m[1] }),
  ],
  [
    /^Unexpected trailing characters after string value: '([\s\S]*)'$/,
    (m) => i18n.t("regMessages.unexpectedTrailingChars", { rest: m[1] }),
  ],
  [
    /^Invalid DWORD hex value '([^']*)'$/,
    (m) => i18n.t("regMessages.invalidDwordHex", { value: m[1] }),
  ],
  [
    /^Invalid hex\(<type>\) specifier '([^']*)'$/,
    (m) => i18n.t("regMessages.invalidHexType", { spec: m[1] }),
  ],
  [
    /^REG_EXPAND_SZ \(hex\(2\)\) byte count must be even, got (\d+)$/,
    (m) => i18n.t("regMessages.expandSzOddBytes", { count: Number(m[1]) }),
  ],
  [
    /^REG_DWORD \(hex\(4\)\) requires 4 bytes, got (\d+)$/,
    (m) => i18n.t("regMessages.dwordByteCount", { count: Number(m[1]) }),
  ],
  [
    /^REG_MULTI_SZ \(hex\(7\)\) byte count must be even, got (\d+)$/,
    (m) => i18n.t("regMessages.multiSzOddBytes", { count: Number(m[1]) }),
  ],
  [
    /^REG_QWORD \(hex\(b\)\) requires 8 bytes, got (\d+)$/,
    (m) => i18n.t("regMessages.qwordByteCount", { count: Number(m[1]) }),
  ],
  [
    /^Unsupported registry value syntax: '([\s\S]+)'$/,
    (m) => i18n.t("regMessages.unsupportedValueSyntax", { syntax: m[1] }),
  ],
  [
    /^Failed reading '([^']+)': ([\s\S]+)$/,
    (m) => i18n.t("regMessages.failedReadFile", { path: m[1], details: m[2] }),
  ],
  [
    /^Preview truncated to first (\d+) operations \(total: (\d+)\)\.$/,
    (m) => i18n.t("regMessages.previewTruncated", { limit: m[1], total: m[2] }),
  ],
];

function translateRegInner(msg: string): string {
  const exact = EXACT_REG_MESSAGES[msg];
  if (exact) return exact();

  for (const [pattern, format] of REGEX_REG_MESSAGES) {
    const match = msg.match(pattern);
    if (match) return format(match);
  }
  return msg;
}

export function localizeRegMessage(raw: string): string {
  const lineMatch = raw.match(/^Line (\d+): ([\s\S]+)$/);
  if (lineMatch) {
    return i18n.t("regMessages.linePrefix", {
      line: lineMatch[1],
      message: translateRegInner(lineMatch[2]),
    });
  }
  return translateRegInner(raw);
}
