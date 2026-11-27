import assert from "node:assert/strict";
import fs from "node:fs";
import { createRequire } from "node:module";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  bumpSemver,
  checkVersionFiles,
  normalizeVersion,
  resolveTargetVersion,
  syncVersionFiles,
} from "./sync-version.mjs";

let esbuildBuild;
try {
  ({ build: esbuildBuild } = await import("esbuild"));
} catch {
  console.error(
    "Missing 'esbuild' dependency. Run 'npm ci' or 'npm install' before running frontend tests."
  );
  process.exit(1);
}

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const rootDir = path.resolve(__dirname, "..");

async function loadFrontendModules() {
  const result = await esbuildBuild({
    stdin: {
      contents: `
        export * as destinationValidation from "./src/utils/destinationValidation";
        export * as win11Version from "./src/utils/win11Version";
        export * as format from "./src/utils/format";
        export * as backendErrors from "./src/utils/backendErrors";
        export * as contextMenu from "./src/utils/contextMenu";
        export { default as i18n } from "./src/i18n";
      `,
      resolveDir: rootDir,
      loader: "ts",
    },
    bundle: true,
    platform: "node",
    format: "cjs",
    write: false,
    logLevel: "silent",
  });

  const code = result.outputFiles[0].text;
  const moduleObj = { exports: {} };
  const storage = new Map();
  const mockLocalStorage = {
    getItem(key) {
      return storage.has(key) ? storage.get(key) : null;
    },
    setItem(key, value) {
      storage.set(key, String(value));
    },
    removeItem(key) {
      storage.delete(key);
    },
  };
  const mockWindow = { localStorage: mockLocalStorage };
  const mockDocument = { documentElement: { lang: "en" } };
  const mockNavigator = { language: "en-US", languages: ["en-US"] };
  const cjsRequire = createRequire(import.meta.url);

  const runner = new Function(
    "exports",
    "require",
    "module",
    "__filename",
    "__dirname",
    "window",
    "document",
    "navigator",
    "localStorage",
    code
  );
  runner(
    moduleObj.exports,
    cjsRequire,
    moduleObj,
    path.join(rootDir, "virtual-frontend-tests.cjs"),
    rootDir,
    mockWindow,
    mockDocument,
    mockNavigator,
    mockLocalStorage
  );

  return { mod: moduleObj.exports, mockDocument };
}

const { mod, mockDocument } = await loadFrontendModules();
const {
  destinationValidation,
  win11Version,
  format,
  backendErrors,
  contextMenu,
  i18n,
} = mod;

let passed = 0;
function test(name, fn) {
  fn();
  passed += 1;
  console.log(`  ok - ${name}`);
}

console.log("Running frontend unit tests...");

test("destinationValidation.getBaseName extracts trailing segment across separators", () => {
  assert.equal(destinationValidation.getBaseName("C:\\Tools\\setup.exe"), "setup.exe");
  assert.equal(destinationValidation.getBaseName("/opt/scripts/run.ps1"), "run.ps1");
  assert.equal(destinationValidation.getBaseName("C:\\Tools\\MyApp\\"), "MyApp");
  assert.equal(destinationValidation.getBaseName("plain.txt"), "plain.txt");
});

test("destinationValidation.defaultDestinationForScan chooses defaults by type and extension", () => {
  assert.equal(
    destinationValidation.defaultDestinationForScan({
      path: "C:\\Payload\\AppDir",
      isDirectory: true,
      sizeBytes: 0,
      fileCount: 3,
    }),
    ""
  );
  for (const ext of ["cmd", "bat", "ps1"]) {
    assert.equal(
      destinationValidation.defaultDestinationForScan({
        path: `C:\\Scripts\\FirstLogon.${ext}`,
        isDirectory: false,
        sizeBytes: 120,
        fileCount: 1,
      }),
      "Windows\\Setup\\Scripts"
    );
  }
  assert.equal(
    destinationValidation.defaultDestinationForScan({
      path: "C:\\Tools\\app.exe",
      isDirectory: false,
      sizeBytes: 1024,
      fileCount: 1,
    }),
    "Tools"
  );
});

test("destinationValidation.validateDestinationClient rejects traversal, UNC, drives, reserved names and trailing dots/spaces", () => {
  const invalidInputs = [
    "..",
    "Windows\\..\\System32",
    "\\\\server\\share\\dir",
    "C:\\Windows\\Temp",
    "Folder:Stream",
    "Bad<Name>",
    "...",
    "Windows\\\\DoubleSlash",
    "CON",
    "Windows\\nul\\Sub",
    "Folder.",
    "Folder \\Sub",
  ];
  for (const dest of invalidInputs) {
    const res = destinationValidation.validateDestinationClient("installWim", dest, "file.txt");
    assert.equal(res.errorKey, "invalidDestTraversal", `Expected traversal error for ${dest}`);
  }
  for (const badBaseName of ["NUL.txt", "COM1", "LPT9.log", "file.", "file "]) {
    const res = destinationValidation.validateDestinationClient("installWim", "Tools", badBaseName);
    assert.equal(
      res.errorKey,
      "invalidDestTraversal",
      `Expected reserved/trailing error for baseName ${badBaseName}`
    );
  }
});

test("destinationValidation.validateDestinationClient protects critical installWim and isoRoot targets", () => {
  assert.equal(
    destinationValidation.validateDestinationClient(
      "installWim",
      "Windows\\System32\\config",
      "SYSTEM"
    ).errorKey,
    "invalidDestProtected"
  );
  assert.equal(
    destinationValidation.validateDestinationClient(
      "installWim",
      "Users/Default",
      "NTUSER.DAT"
    ).errorKey,
    "invalidDestProtected"
  );
  assert.equal(
    destinationValidation.validateDestinationClient("isoRoot", "sources", "install.wim").errorKey,
    "invalidDestProtected"
  );
  assert.equal(
    destinationValidation.validateDestinationClient("isoRoot", "sources", "install2.swm").errorKey,
    "invalidDestProtected"
  );
  assert.equal(
    destinationValidation.validateDestinationClient("isoRoot", "efi/microsoft/boot", "efisys.bin")
      .errorKey,
    "invalidDestProtected"
  );
  assert.equal(
    destinationValidation.validateDestinationClient("isoRoot", "", "bootmgr").errorKey,
    "invalidDestProtected"
  );
});

test("destinationValidation.validateDestinationClient normalizes valid relative paths", () => {
  const res = destinationValidation.validateDestinationClient(
    "installWim",
    " /Windows/Setup/Scripts\\ ",
    "setupcomplete.cmd"
  );
  assert.deepEqual(res, {
    errorKey: null,
    cleanDir: "Windows\\Setup\\Scripts",
  });

  const customTools = destinationValidation.validateDestinationClient(
    "isoRoot",
    "Soft/Utils",
    "install.wim"
  );
  assert.deepEqual(customTools, {
    errorKey: null,
    cleanDir: "Soft\\Utils",
  });
});

test("win11Version.resolveWin11Version maps build thresholds accurately", () => {
  assert.equal(win11Version.resolveWin11Version(null), "26H2");
  assert.equal(win11Version.resolveWin11Version(26300), "26H2");
  assert.equal(win11Version.resolveWin11Version(26200), "25H2");
  assert.equal(win11Version.resolveWin11Version(26100), "24H2");
  assert.equal(win11Version.resolveWin11Version(22631), "23H2");
  assert.equal(win11Version.resolveWin11Version(22621), "22H2");
  assert.equal(win11Version.resolveWin11Version(22000), "21H2");
  assert.equal(win11Version.resolveWin11Version(19045), "26H2");
});

test("format.formatBytes formats B/KB/MB/GB in EN and RU", () => {
  i18n.changeLanguage("en");
  assert.equal(mockDocument.documentElement.lang, "en");
  assert.equal(format.formatBytes(512), "512 B");
  assert.equal(format.formatBytes(2048), "2 KB");
  assert.equal(format.formatBytes(5 * 1024 * 1024), "5.0 MB");
  assert.equal(format.formatBytes(2.5 * 1024 * 1024 * 1024), "2.50 GB");

  i18n.changeLanguage("ru");
  assert.equal(mockDocument.documentElement.lang, "ru");
  assert.equal(format.formatBytes(512), "512 Б");
  assert.equal(format.formatBytes(2048), "2 КБ");
  assert.equal(format.formatBytes(5 * 1024 * 1024), "5.0 МБ");
  assert.equal(format.formatBytes(2.5 * 1024 * 1024 * 1024), "2.50 ГБ");
});

test("i18n pluralization handles RU and EN storageFilesCount forms", () => {
  i18n.changeLanguage("ru");
  assert.equal(i18n.t("settings.storageFilesCount", { count: 1 }), "1 файл");
  assert.equal(i18n.t("settings.storageFilesCount", { count: 2 }), "2 файла");
  assert.equal(i18n.t("settings.storageFilesCount", { count: 5 }), "5 файлов");
  assert.equal(i18n.t("settings.storageFilesCount", { count: 21 }), "21 файл");

  i18n.changeLanguage("en");
  assert.equal(i18n.t("settings.storageFilesCount", { count: 1 }), "1 file");
  assert.equal(i18n.t("settings.storageFilesCount", { count: 2 }), "2 files");
});

test("backendErrors.localizeBackendError and localizeXmlError translate known strings and pass through unknown ones", () => {
  i18n.changeLanguage("ru");
  assert.equal(
    backendErrors.localizeBackendError(
      "Cannot clear workspace cache while an image operation is running."
    ),
    i18n.t("backendErrors.cannotClearCacheWhileBusy")
  );
  assert.notEqual(
    backendErrors.localizeBackendError(
      "Cannot clear workspace cache while an image operation is running."
    ),
    "Cannot clear workspace cache while an image operation is running."
  );
  assert.equal(
    backendErrors.localizeBackendError("explorer.exe not found in Windows directory."),
    i18n.t("backendErrors.explorerNotFound")
  );
  assert.notEqual(
    backendErrors.localizeBackendError("explorer.exe not found in Windows directory."),
    "explorer.exe not found in Windows directory."
  );
  assert.equal(
    backendErrors.localizeBackendError("Some unmapped custom error 42"),
    "Some unmapped custom error 42"
  );

  i18n.changeLanguage("en");
  assert.equal(
    backendErrors.localizeBackendError("explorer.exe not found in Windows directory."),
    i18n.t("backendErrors.explorerNotFound")
  );

  i18n.changeLanguage("ru");
  const xmlTranslated = backendErrors.localizeXmlError(
    "Expected root XML element <unattend>, found <settings>."
  );
  assert.ok(xmlTranslated.includes("<settings>"));
  assert.notEqual(xmlTranslated, "Expected root XML element <unattend>, found <settings>.");
  const xmlUncTranslated = backendErrors.localizeXmlError(
    "UNC network or device paths are not allowed for '\\\\server\\share\\unattend.xml'."
  );
  assert.notEqual(
    xmlUncTranslated,
    "UNC network or device paths are not allowed for '\\\\server\\share\\unattend.xml'."
  );
  assert.equal(
    backendErrors.localizeXmlError("Arbitrary XML note"),
    "Arbitrary XML note"
  );

  assert.equal(
    backendErrors.localizeBackendError(
      "Cannot change workspace directory while an image operation is running."
    ),
    i18n.t("backendErrors.cannotChangeWorkspaceWhileBusy")
  );
  assert.equal(
    backendErrors.localizeBackendError(
      "Selected oscdimg.exe must be a valid Microsoft-signed binary inside a protected Program Files directory."
    ),
    i18n.t("backendErrors.oscdimgUntrusted")
  );
  assert.notEqual(
    backendErrors.localizeBackendError(
      "Downloaded update 'kb123.msu' failed Microsoft Authenticode signature verification."
    ),
    "Downloaded update 'kb123.msu' failed Microsoft Authenticode signature verification."
  );
  assert.equal(
    backendErrors.localizeRegMessage(
      "Line 3: REG_LINK (hex(6)) values are not allowed in offline registry files."
    ),
    i18n.t("regMessages.linePrefix", {
      line: "3",
      message: i18n.t("regMessages.regLinkNotAllowed"),
    })
  );

  // Stage 1 & 2 new backend error mappings (RU and EN)
  i18n.changeLanguage("ru");
  assert.equal(
    backendErrors.localizeBackendError("pnputil.exe not found in Windows directory."),
    i18n.t("backendErrors.pnputilNotFound")
  );
  assert.equal(
    backendErrors.localizeBackendError(
      "Invalid OEM driver published name 'bad.inf'. Expected format like oem12.inf."
    ),
    i18n.t("backendErrors.driverInvalidPublishedName", { name: "bad.inf" })
  );
  assert.equal(
    backendErrors.localizeBackendError(
      "Winget packages can only run during the First Logon (User) phase."
    ),
    i18n.t("backendErrors.postSetupWingetPhase")
  );
  assert.equal(
    backendErrors.localizeBackendError("Invalid Winget package ID 'bad;id': bad"),
    i18n.t("backendErrors.postSetupWingetInvalid", { id: "bad;id" })
  );

  i18n.changeLanguage("en");
  assert.equal(
    backendErrors.localizeBackendError(
      "Post-Setup FirstLogonCommands (autounattend.xml) requires ISO output target."
    ),
    i18n.t("backendErrors.postSetupAutounattendRequiresIso")
  );

  // Offline Services Manager backend errors (RU and EN)
  i18n.changeLanguage("ru");
  assert.equal(
    backendErrors.localizeBackendError("Critical system service 'RpcSs' cannot be disabled."),
    i18n.t("backendErrors.serviceCriticalLocked", { name: "RpcSs" })
  );
  assert.equal(
    backendErrors.localizeBackendError("Duplicate service entry 'DiagTrack' in services plan."),
    i18n.t("backendErrors.serviceDuplicateEntry", { name: "DiagTrack" })
  );
  i18n.changeLanguage("en");
  assert.equal(
    backendErrors.localizeBackendError("Invalid service name 'bad\\svc'."),
    i18n.t("backendErrors.serviceInvalidName", { name: "bad\\svc" })
  );
});

test("syncVersion.normalizeVersion & bumpSemver handle valid/invalid versions", () => {
  assert.equal(normalizeVersion("0.4.3"), "0.4.3");
  assert.equal(normalizeVersion(" v0.4.3 "), "0.4.3");
  assert.equal(normalizeVersion("v0.5.0-rc.1"), "0.5.0-rc.1");

  for (const bad of ["0.4", "latest", "v1.2.3.4", ""]) {
    assert.throws(() => normalizeVersion(bad), /Invalid SemVer version/);
  }

  assert.equal(bumpSemver("0.4.2", "patch"), "0.4.3");
  assert.equal(bumpSemver("0.4.2", "minor"), "0.5.0");
  assert.equal(bumpSemver("0.4.2", "major"), "1.0.0");
  assert.equal(bumpSemver("0.4.3-rc.1", "patch"), "0.4.4");
});

test("syncVersion.resolveTargetVersion picks max tag/package and resolves prerelease", () => {
  assert.deepEqual(
    resolveTargetVersion({
      explicitVersion: "",
      bump: "patch",
      existingTags: ["v0.3.0", "v0.4.2", "v0.4.1", "non-semver"],
      currentPackageVersion: "0.2.0",
      prereleaseMode: "auto",
    }),
    {
      version: "0.4.3",
      tagName: "v0.4.3",
      winVersion: "0.4.3.0",
      isPrerelease: false,
    }
  );

  assert.deepEqual(
    resolveTargetVersion({
      explicitVersion: "v0.5.0-rc.1",
      prereleaseMode: "auto",
    }),
    {
      version: "0.5.0-rc.1",
      tagName: "v0.5.0-rc.1",
      winVersion: "0.5.0.0",
      isPrerelease: true,
    }
  );

  assert.equal(
    resolveTargetVersion({
      explicitVersion: "0.5.0-rc.1",
      prereleaseMode: "false",
    }).isPrerelease,
    false
  );
  assert.equal(
    resolveTargetVersion({
      explicitVersion: "0.4.3",
      prereleaseMode: "true",
    }).isPrerelease,
    true
  );
});

test("syncVersion.syncVersionFiles & checkVersionFiles synchronize all 7 project files cleanly", () => {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "isocraft-ver-"));
  const relFiles = [
    "package.json",
    "package-lock.json",
    "src-tauri/tauri.conf.json",
    "src-tauri/Cargo.toml",
    "src-tauri/crates/core/Cargo.toml",
    "src-tauri/Cargo.lock",
    "src-tauri/app.manifest",
  ];
  try {
    for (const rel of relFiles) {
      const src = path.join(rootDir, ...rel.split("/"));
      const dst = path.join(tmpDir, ...rel.split("/"));
      fs.mkdirSync(path.dirname(dst), { recursive: true });
      fs.copyFileSync(src, dst);
    }

    const res = syncVersionFiles(tmpDir, "0.4.3-rc.1");
    assert.deepEqual(res, { version: "0.4.3-rc.1", winVersion: "0.4.3.0" });
    checkVersionFiles(tmpDir, "0.4.3-rc.1");

    const manifest = fs.readFileSync(
      path.join(tmpDir, "src-tauri", "app.manifest"),
      "utf8"
    );
    assert.ok(manifest.includes('version="0.4.3.0"'));
    assert.ok(manifest.includes('version="6.0.0.0"'));

    const cargoLock = fs.readFileSync(
      path.join(tmpDir, "src-tauri", "Cargo.lock"),
      "utf8"
    );
    assert.ok(
      cargoLock.includes('name = "isocraft"\r\nversion = "0.4.3-rc.1"') ||
        cargoLock.includes('name = "isocraft"\nversion = "0.4.3-rc.1"')
    );
    assert.ok(
      cargoLock.includes('name = "isocraft-core"\r\nversion = "0.4.3-rc.1"') ||
        cargoLock.includes('name = "isocraft-core"\nversion = "0.4.3-rc.1"')
    );

    assert.throws(() => checkVersionFiles(tmpDir, "0.4.4"), /Version check failed/);
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});

test("contextMenu.isTextEditableInputType accepts text inputs and rejects non-text inputs", () => {
  for (const type of ["", "text", "TEXT", "search", "url", "tel", "email", "password", "number", null, undefined]) {
    assert.equal(contextMenu.isTextEditableInputType(type), true, `Expected editable for ${String(type)}`);
  }
  for (const type of ["checkbox", "radio", "button", "submit", "reset", "file", "range", "color", "hidden"]) {
    assert.equal(contextMenu.isTextEditableInputType(type), false, `Expected non-editable for ${type}`);
  }
});

test("contextMenu.buildContextMenuItems builds expected items across all snapshot kinds", () => {
  const editableNoRedo = contextMenu.buildContextMenuItems({
    kind: "editable",
    canUndo: true,
    canRedo: false,
    hasSelection: true,
    hasValue: true,
    isAllSelected: false,
    isPassword: false,
    canSelectContainer: false,
  });
  assert.deepEqual(
    editableNoRedo.map((i) => (i.type === "action" ? [i.id, i.disabled] : i.type)),
    [
      ["undo", false],
      "separator",
      ["cut", false],
      ["copy", false],
      ["paste", false],
      ["delete", false],
      "separator",
      ["selectAll", false],
    ]
  );

  const editableWithRedoPassword = contextMenu.buildContextMenuItems({
    kind: "editable",
    canUndo: false,
    canRedo: true,
    hasSelection: true,
    hasValue: true,
    isAllSelected: true,
    isPassword: true,
    canSelectContainer: false,
  });
  assert.deepEqual(
    editableWithRedoPassword.map((i) => (i.type === "action" ? [i.id, i.disabled] : i.type)),
    [
      ["undo", true],
      ["redo", false],
      "separator",
      ["cut", true],
      ["copy", true],
      ["paste", false],
      ["delete", false],
      "separator",
      ["selectAll", true],
    ]
  );

  assert.deepEqual(
    contextMenu.buildContextMenuItems({
      kind: "readonly-input",
      canUndo: false,
      canRedo: false,
      hasSelection: false,
      hasValue: false,
      isAllSelected: false,
      isPassword: false,
      canSelectContainer: false,
    }),
    []
  );

  assert.deepEqual(
    contextMenu.buildContextMenuItems({
      kind: "readonly-input",
      canUndo: false,
      canRedo: false,
      hasSelection: true,
      hasValue: true,
      isAllSelected: false,
      isPassword: false,
      canSelectContainer: false,
    }).map((i) => [i.id, i.disabled]),
    [
      ["copy", false],
      ["selectAll", false],
    ]
  );

  assert.deepEqual(
    contextMenu.buildContextMenuItems({
      kind: "selection",
      canUndo: false,
      canRedo: false,
      hasSelection: true,
      hasValue: true,
      isAllSelected: false,
      isPassword: false,
      canSelectContainer: true,
    }).map((i) => [i.id, i.disabled]),
    [
      ["copy", false],
      ["selectAll", false],
    ]
  );

  assert.deepEqual(
    contextMenu.buildContextMenuItems({
      kind: "selectable",
      canUndo: false,
      canRedo: false,
      hasSelection: false,
      hasValue: true,
      isAllSelected: false,
      isPassword: false,
      canSelectContainer: true,
    }).map((i) => [i.id, i.disabled]),
    [
      ["copy", true],
      ["selectAll", false],
    ]
  );
});

test("contextMenu.shouldBlockBrowserShortcut blocks dangerous shortcuts across layouts and preserves editing shortcuts", () => {
  const base = { key: "", code: "", ctrlKey: false, metaKey: false, altKey: false, shiftKey: false };
  // Blocked in both dev & prod (including Russian keyboard layout via e.code)
  for (const ev of [
    { ...base, key: "F5", code: "F5" },
    { ...base, key: "r", code: "KeyR", ctrlKey: true },
    { ...base, key: "к", code: "KeyR", ctrlKey: true },
    { ...base, key: "R", code: "KeyR", ctrlKey: true, shiftKey: true },
    { ...base, key: "p", code: "KeyP", ctrlKey: true },
    { ...base, key: "s", code: "KeyS", ctrlKey: true },
    { ...base, key: "u", code: "KeyU", ctrlKey: true },
    { ...base, key: "ArrowLeft", code: "ArrowLeft", altKey: true },
    { ...base, key: "ArrowRight", code: "ArrowRight", altKey: true },
    { ...base, key: "F7", code: "F7" },
  ]) {
    assert.equal(contextMenu.shouldBlockBrowserShortcut(ev, true), true);
    assert.equal(contextMenu.shouldBlockBrowserShortcut(ev, false), true);
  }

  // DevTools blocked only in prod
  for (const ev of [
    { ...base, key: "F12", code: "F12" },
    { ...base, key: "I", code: "KeyI", ctrlKey: true, shiftKey: true },
    { ...base, key: "J", code: "KeyJ", ctrlKey: true, shiftKey: true },
    { ...base, key: "C", code: "KeyC", ctrlKey: true, shiftKey: true },
  ]) {
    assert.equal(contextMenu.shouldBlockBrowserShortcut(ev, true), false);
    assert.equal(contextMenu.shouldBlockBrowserShortcut(ev, false), true);
  }

  // Safe editing and AltGr combinations are preserved
  for (const ev of [
    { ...base, key: "c", code: "KeyC", ctrlKey: true },
    { ...base, key: "v", code: "KeyV", ctrlKey: true },
    { ...base, key: "x", code: "KeyX", ctrlKey: true },
    { ...base, key: "a", code: "KeyA", ctrlKey: true },
    { ...base, key: "z", code: "KeyZ", ctrlKey: true },
    { ...base, key: "y", code: "KeyY", ctrlKey: true },
    { ...base, key: "r", code: "KeyR", ctrlKey: true, altKey: true },
  ]) {
    assert.equal(contextMenu.shouldBlockBrowserShortcut(ev, false), false);
  }
});

test("contextMenu.clampMenuPosition places, flips, and clamps menu coordinates within viewport", () => {
  assert.deepEqual(contextMenu.clampMenuPosition(100, 120, 200, 180, 1280, 800), {
    left: 100,
    top: 120,
    originX: "left",
    originY: "top",
  });

  // Flips left and up near bottom-right corner
  assert.deepEqual(contextMenu.clampMenuPosition(1250, 780, 200, 180, 1280, 800), {
    left: 1050,
    top: 600,
    originX: "right",
    originY: "bottom",
  });

  // Clamps inside small viewport when flip cannot fit
  assert.deepEqual(contextMenu.clampMenuPosition(50, 40, 200, 180, 220, 190), {
    left: 12,
    top: 8,
    originX: "left",
    originY: "top",
  });
});

console.log(`Frontend unit tests passed: ${passed} suites.`);
