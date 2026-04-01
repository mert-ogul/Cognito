/**
 * Sync uv dependencies and copy `sidecar/` into `src-tauri/resources/sidecar/` for Tauri bundling.
 * Run before `cargo tauri build` (see package.json `package:sidecar` and tauri.conf.json).
 */
import { execSync } from "node:child_process";
import fs from "node:fs";
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

execSync("uv sync", { cwd: sidecarDir, stdio: "inherit" });

fs.rmSync(dest, { recursive: true, force: true });
fs.mkdirSync(path.dirname(dest), { recursive: true });
fs.cpSync(sidecarDir, dest, {
  recursive: true,
  filter: (src) => {
    const b = path.basename(src);
    if (b === ".git" || b === "__pycache__" || b === ".pytest_cache" || b === ".ruff_cache") {
      return false;
    }
    return true;
  },
});

console.log(`package-sidecar: copied venv + sources to ${dest}`);
