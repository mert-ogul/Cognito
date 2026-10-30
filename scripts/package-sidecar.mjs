/**
 * Bundle a standalone CPython + sidecar sources into `src-tauri/resources/sidecar/` for Tauri.
 * Run before `cargo tauri build` (see package.json `package:sidecar` and tauri.conf.json).
 */
import { execFileSync, execSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(__dirname, "..");
const sidecarDir = path.join(root, "sidecar");
const dest = path.join(root, "src-tauri", "resources", "sidecar");

if (!fs.existsSync(path.join(sidecarDir, "pyproject.toml"))) {
  console.error("package-sidecar: missing sidecar/pyproject.toml");
  process.exit(1);
}

const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "cognito-sidecar-"));
const pythonInstallDir = path.join(tempRoot, "python-install");
const requirementsPath = path.join(tempRoot, "requirements.txt");

try {
  fs.rmSync(dest, { recursive: true, force: true });
  fs.mkdirSync(dest, { recursive: true });
  fs.writeFileSync(path.join(dest, ".gitkeep"), "");

  console.log("package-sidecar: installing standalone CPython 3.12…");
  execSync(`uv python install 3.12 --install-dir "${pythonInstallDir}"`, {
    cwd: root,
    stdio: "inherit",
  });

  const cpythonDirs = fs
    .readdirSync(pythonInstallDir, { withFileTypes: true })
    .filter((d) => d.isDirectory() && d.name.startsWith("cpython-"))
    .map((d) => d.name);
  if (cpythonDirs.length !== 1) {
    console.error(
      `package-sidecar: expected one cpython-* under ${pythonInstallDir}, found: ${cpythonDirs.join(", ") || "(none)"}`,
    );
    process.exit(1);
  }
  const cpythonSrc = path.join(pythonInstallDir, cpythonDirs[0]);
  const pythonDest = path.join(dest, "python");
  fs.cpSync(cpythonSrc, pythonDest, { recursive: true, verbatimSymlinks: true });

  const bundledPython =
    process.platform === "win32"
      ? path.join(pythonDest, "python.exe")
      : path.join(pythonDest, "bin", "python3");
  if (!fs.existsSync(bundledPython)) {
    console.error(`package-sidecar: bundled interpreter missing: ${bundledPython}`);
    process.exit(1);
  }

  console.log("package-sidecar: exporting requirements and installing into bundle…");
  execSync(
    `uv export --frozen --no-dev --no-hashes --no-emit-project -o "${requirementsPath}"`,
    { cwd: sidecarDir, stdio: "inherit" },
  );
  execFileSync(
    "uv",
    [
      "pip",
      "install",
      "--python",
      bundledPython,
      "--break-system-packages",
      "--no-cache",
      "-r",
      requirementsPath,
    ],
    { cwd: sidecarDir, stdio: "inherit" },
  );

  const copyEntries = ["main.py", "common", "ingestion", "query"];
  for (const name of copyEntries) {
    const src = path.join(sidecarDir, name);
    if (!fs.existsSync(src)) {
      console.error(`package-sidecar: missing ${src}`);
      process.exit(1);
    }
    const target = path.join(dest, name);
    fs.cpSync(src, target, { recursive: true });
  }

  function removePycache(dir) {
    for (const ent of fs.readdirSync(dir, { withFileTypes: true })) {
      const p = path.join(dir, ent.name);
      if (ent.isDirectory()) {
        if (ent.name === "__pycache__") {
          fs.rmSync(p, { recursive: true, force: true });
        } else {
          removePycache(p);
        }
      }
    }
  }
  removePycache(dest);

  function dirSizeBytes(dir) {
    let total = 0;
    for (const ent of fs.readdirSync(dir, { withFileTypes: true })) {
      const p = path.join(dir, ent.name);
      if (ent.isDirectory()) {
        total += dirSizeBytes(p);
      } else if (ent.isSymbolicLink()) {
        // count link itself, not target
        total += fs.lstatSync(p).size;
      } else {
        total += fs.statSync(p).size;
      }
    }
    return total;
  }
  const bytes = dirSizeBytes(dest);
  const mb = (bytes / (1024 * 1024)).toFixed(1);
  console.log(`package-sidecar: bundle ready at ${dest} (${mb} MiB)`);
} finally {
  fs.rmSync(tempRoot, { recursive: true, force: true });
}
