import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const SEMVER_RE = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$/;

const LOCK_PKG_VERSION_RE =
  /(^\[\[package\]\]\r?\nname = "(?:isocraft|isocraft-core)"\r?\nversion = ")[^"]+(")/gm;
const TOML_VERSION_RE = /(\[package\][\r\n]+[\s\S]*?^version\s*=\s*")[^"]+(")/m;
const MANIFEST_IDENTITY_RE = /(<assemblyIdentity\s+version=")[^"]+(")/;

function parseSemverParts(raw) {
  const normalized = normalizeVersion(raw);
  const match = normalized.match(SEMVER_RE);
  return {
    version: normalized,
    major: Number(match[1]),
    minor: Number(match[2]),
    patch: Number(match[3]),
    prerelease: match[4] ?? null,
  };
}

export function normalizeVersion(raw) {
  if (typeof raw !== "string") {
    throw new Error(`Invalid SemVer version: ${String(raw)}`);
  }
  const trimmed = raw.trim().replace(/^v/, "");
  if (!SEMVER_RE.test(trimmed)) {
    throw new Error(`Invalid SemVer version: '${raw}'`);
  }
  return trimmed;
}

export function compareSemver(a, b) {
  const pa = parseSemverParts(a);
  const pb = parseSemverParts(b);
  if (pa.major !== pb.major) return pa.major - pb.major;
  if (pa.minor !== pb.minor) return pa.minor - pb.minor;
  if (pa.patch !== pb.patch) return pa.patch - pb.patch;
  if (!pa.prerelease && !pb.prerelease) return 0;
  if (!pa.prerelease) return 1;
  if (!pb.prerelease) return -1;
  if (pa.prerelease === pb.prerelease) return 0;
  return pa.prerelease < pb.prerelease ? -1 : 1;
}

export function bumpSemver(baseVersion, kind = "patch") {
  const parts = parseSemverParts(baseVersion);
  if (kind === "major") {
    return `${parts.major + 1}.0.0`;
  }
  if (kind === "minor") {
    return `${parts.major}.${parts.minor + 1}.0`;
  }
  if (kind === "patch") {
    return `${parts.major}.${parts.minor}.${parts.patch + 1}`;
  }
  throw new Error(
    `Invalid bump kind '${kind}'. Expected one of: patch, minor, major.`
  );
}

export function resolveTargetVersion({
  explicitVersion,
  bump = "patch",
  existingTags = [],
  currentPackageVersion = "0.0.0",
  prereleaseMode = "auto",
} = {}) {
  let version;
  const trimmedExplicit =
    typeof explicitVersion === "string" ? explicitVersion.trim() : "";

  if (trimmedExplicit.length > 0) {
    version = normalizeVersion(trimmedExplicit);
  } else {
    const candidates = [];
    for (const candidate of [currentPackageVersion, ...existingTags]) {
      try {
        candidates.push(normalizeVersion(candidate));
      } catch {
        // Ignore non-SemVer tags or placeholders
      }
    }
    const maxBase =
      candidates.length > 0
        ? candidates.reduce((best, cur) =>
            compareSemver(cur, best) > 0 ? cur : best
          )
        : "0.0.0";
    version = bumpSemver(maxBase, bump);
  }

  let isPrerelease;
  if (prereleaseMode === "true" || prereleaseMode === true) {
    isPrerelease = true;
  } else if (prereleaseMode === "false" || prereleaseMode === false) {
    isPrerelease = false;
  } else if (prereleaseMode === "auto") {
    isPrerelease = version.includes("-");
  } else {
    throw new Error(
      `Invalid prereleaseMode '${prereleaseMode}'. Expected: auto, true, or false.`
    );
  }

  const parts = parseSemverParts(version);
  return {
    version,
    tagName: `v${version}`,
    winVersion: `${parts.major}.${parts.minor}.${parts.patch}.0`,
    isPrerelease,
  };
}

function readUtf8(filePath) {
  const raw = fs.readFileSync(filePath, "utf8");
  const eol = raw.includes("\r\n") ? "\r\n" : "\n";
  return { raw, eol };
}

function writeUtf8WithEol(filePath, content, eol) {
  const normalized = content.replace(/\r?\n/g, eol);
  fs.writeFileSync(filePath, normalized, "utf8");
}

function formatJsonPreserveEol(obj, hadTrailingNewline, eol) {
  const body = JSON.stringify(obj, null, 2);
  const withNewline = hadTrailingNewline ? `${body}\n` : body;
  return withNewline.replace(/\r?\n/g, eol);
}

export function syncVersionFiles(rootDir, rawVersion) {
  const parts = parseSemverParts(rawVersion);
  const ver = parts.version;
  const winVer = `${parts.major}.${parts.minor}.${parts.patch}.0`;

  // 1. package.json
  const pkgPath = path.join(rootDir, "package.json");
  {
    const { raw, eol } = readUtf8(pkgPath);
    const data = JSON.parse(raw);
    data.version = ver;
    writeUtf8WithEol(
      pkgPath,
      formatJsonPreserveEol(data, raw.endsWith("\n"), eol),
      eol
    );
  }

  // 2. package-lock.json
  const pkgLockPath = path.join(rootDir, "package-lock.json");
  {
    const { raw, eol } = readUtf8(pkgLockPath);
    const data = JSON.parse(raw);
    data.version = ver;
    if (data.packages && typeof data.packages[""] === "object" && data.packages[""] !== null) {
      data.packages[""].version = ver;
    }
    writeUtf8WithEol(
      pkgLockPath,
      formatJsonPreserveEol(data, raw.endsWith("\n"), eol),
      eol
    );
  }

  // 3. src-tauri/tauri.conf.json
  const tauriConfPath = path.join(rootDir, "src-tauri", "tauri.conf.json");
  {
    const { raw, eol } = readUtf8(tauriConfPath);
    const data = JSON.parse(raw);
    data.version = ver;
    writeUtf8WithEol(
      tauriConfPath,
      formatJsonPreserveEol(data, raw.endsWith("\n"), eol),
      eol
    );
  }

  // 4 & 5. src-tauri/Cargo.toml & src-tauri/crates/core/Cargo.toml
  for (const rel of [
    path.join("src-tauri", "Cargo.toml"),
    path.join("src-tauri", "crates", "core", "Cargo.toml"),
  ]) {
    const filePath = path.join(rootDir, rel);
    const { raw, eol } = readUtf8(filePath);
    if (!TOML_VERSION_RE.test(raw)) {
      throw new Error(`Could not locate version field in ${rel}`);
    }
    const updated = raw.replace(TOML_VERSION_RE, `$1${ver}$2`);
    writeUtf8WithEol(filePath, updated, eol);
  }

  // 6. src-tauri/Cargo.lock
  const cargoLockPath = path.join(rootDir, "src-tauri", "Cargo.lock");
  {
    const { raw, eol } = readUtf8(cargoLockPath);
    let replacements = 0;
    const updated = raw.replace(LOCK_PKG_VERSION_RE, (_full, prefix, suffix) => {
      replacements += 1;
      return `${prefix}${ver}${suffix}`;
    });
    if (replacements !== 2) {
      throw new Error(
        `Expected 2 workspace package entries (isocraft, isocraft-core) in src-tauri/Cargo.lock, found ${replacements}`
      );
    }
    writeUtf8WithEol(cargoLockPath, updated, eol);
  }

  // 7. src-tauri/app.manifest
  const manifestPath = path.join(rootDir, "src-tauri", "app.manifest");
  {
    const { raw, eol } = readUtf8(manifestPath);
    if (!MANIFEST_IDENTITY_RE.test(raw)) {
      throw new Error("Could not locate <assemblyIdentity version=...> in src-tauri/app.manifest");
    }
    const updated = raw.replace(MANIFEST_IDENTITY_RE, `$1${winVer}$2`);
    writeUtf8WithEol(filePathSafe(manifestPath), updated, eol);
  }

  return { version: ver, winVersion: winVer };
}

function filePathSafe(p) {
  return p;
}

export function checkVersionFiles(rootDir, rawVersion) {
  const parts = parseSemverParts(rawVersion);
  const ver = parts.version;
  const winVer = `${parts.major}.${parts.minor}.${parts.patch}.0`;
  const mismatches = [];

  // 1. package.json
  const pkg = JSON.parse(fs.readFileSync(path.join(rootDir, "package.json"), "utf8"));
  if (pkg.version !== ver) {
    mismatches.push(`package.json: expected '${ver}', found '${pkg.version}'`);
  }

  // 2. package-lock.json
  const pkgLock = JSON.parse(
    fs.readFileSync(path.join(rootDir, "package-lock.json"), "utf8")
  );
  if (pkgLock.version !== ver) {
    mismatches.push(
      `package-lock.json (root): expected '${ver}', found '${pkgLock.version}'`
    );
  }
  const lockRootPkg = pkgLock.packages?.[""];
  if (!lockRootPkg || lockRootPkg.version !== ver) {
    mismatches.push(
      `package-lock.json (packages[""]): expected '${ver}', found '${lockRootPkg?.version ?? "<missing>"}'`
    );
  }

  // 3. src-tauri/tauri.conf.json
  const tauriConf = JSON.parse(
    fs.readFileSync(path.join(rootDir, "src-tauri", "tauri.conf.json"), "utf8")
  );
  if (tauriConf.version !== ver) {
    mismatches.push(
      `src-tauri/tauri.conf.json: expected '${ver}', found '${tauriConf.version}'`
    );
  }

  // 4 & 5. Cargo.toml files
  for (const rel of [
    "src-tauri/Cargo.toml",
    "src-tauri/crates/core/Cargo.toml",
  ]) {
    const raw = fs.readFileSync(path.join(rootDir, ...rel.split("/")), "utf8");
    const match = raw.match(/\[package\][\r\n]+[\s\S]*?^version\s*=\s*"([^"]+)"/m);
    const found = match ? match[1] : "<missing>";
    if (found !== ver) {
      mismatches.push(`${rel}: expected '${ver}', found '${found}'`);
    }
  }

  // 6. src-tauri/Cargo.lock
  const cargoLockRaw = fs.readFileSync(
    path.join(rootDir, "src-tauri", "Cargo.lock"),
    "utf8"
  );
  const lockMatches = Array.from(
    cargoLockRaw.matchAll(
      /^\[\[package\]\]\r?\nname = "(isocraft|isocraft-core)"\r?\nversion = "([^"]+)"/gm
    )
  );
  if (lockMatches.length !== 2) {
    mismatches.push(
      `src-tauri/Cargo.lock: expected 2 workspace packages, found ${lockMatches.length}`
    );
  } else {
    for (const m of lockMatches) {
      if (m[2] !== ver) {
        mismatches.push(
          `src-tauri/Cargo.lock (${m[1]}): expected '${ver}', found '${m[2]}'`
        );
      }
    }
  }

  // 7. src-tauri/app.manifest
  const manifestRaw = fs.readFileSync(
    path.join(rootDir, "src-tauri", "app.manifest"),
    "utf8"
  );
  const manifestMatch = manifestRaw.match(/<assemblyIdentity\s+version="([^"]+)"/);
  const foundManifest = manifestMatch ? manifestMatch[1] : "<missing>";
  if (foundManifest !== winVer) {
    mismatches.push(
      `src-tauri/app.manifest: expected '${winVer}', found '${foundManifest}'`
    );
  }

  if (mismatches.length > 0) {
    throw new Error(`Version check failed:\n - ${mismatches.join("\n - ")}`);
  }
}

function runCli() {
  const args = process.argv.slice(2);
  const scriptDir = path.dirname(fileURLToPath(import.meta.url));
  const rootDir = path.resolve(scriptDir, "..");

  if (args[0] === "--resolve") {
    let explicitVersion = "";
    let bump = "patch";
    let prereleaseMode = "auto";
    let useGitTags = false;

    for (let i = 1; i < args.length; i += 1) {
      const flag = args[i];
      if (flag === "--version") {
        explicitVersion = args[++i] ?? "";
      } else if (flag === "--bump") {
        bump = args[++i] ?? "patch";
      } else if (flag === "--prerelease") {
        prereleaseMode = args[++i] ?? "auto";
      } else if (flag === "--git-tags") {
        useGitTags = true;
      } else {
        throw new Error(`Unknown option: ${flag}`);
      }
    }

    const pkg = JSON.parse(
      fs.readFileSync(path.join(rootDir, "package.json"), "utf8")
    );
    const existingTags = useGitTags
      ? execFileSync("git", ["tag", "--list", "v*"], {
          cwd: rootDir,
          encoding: "utf8",
        })
          .split(/\r?\n/)
          .map((t) => t.trim())
          .filter(Boolean)
      : [];

    const resolved = resolveTargetVersion({
      explicitVersion,
      bump,
      existingTags,
      currentPackageVersion: pkg.version ?? "0.0.0",
      prereleaseMode,
    });
    process.stdout.write(`${JSON.stringify(resolved)}\n`);
    return;
  }

  if (args[0] === "--check") {
    let target = args[1];
    if (!target) {
      const pkg = JSON.parse(
        fs.readFileSync(path.join(rootDir, "package.json"), "utf8")
      );
      target = pkg.version;
    }
    if (!target) {
      throw new Error("Usage: node scripts/sync-version.mjs --check [version]");
    }
    checkVersionFiles(rootDir, target);
    const norm = normalizeVersion(target);
    console.log(`Version check passed: all 7 project files match ${norm}.`);
    return;
  }

  const target = args[0];
  if (!target) {
    throw new Error(
      "Usage: node scripts/sync-version.mjs <version> | --check <version> | --resolve [...]"
    );
  }
  const { version, winVersion } = syncVersionFiles(rootDir, target);
  checkVersionFiles(rootDir, version);
  console.log(
    `Synchronized all 7 project files to ${version} (Windows manifest: ${winVersion}).`
  );
}

const isMain =
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  runCli();
}
