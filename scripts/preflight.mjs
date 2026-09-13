import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { loadEnv } from "vite";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
for (const [key, value] of Object.entries(loadEnv("development", root, ""))) {
  if (/^(PATHBASE_|TACHYON_|FIELD_)/.test(key) && value.trim()) {
    process.env[key] ??= value;
  }
}

const child = spawn(
  "cargo",
  [
    "run",
    "--quiet",
    "--locked",
    "--manifest-path",
    "api/Cargo.toml",
    "--",
    "--preflight",
  ],
  {
    cwd: root,
    env: process.env,
    stdio: "inherit",
  },
);

child.on("error", (error) => {
  console.error(`プリフライトを開始できません: ${error.message}`);
  process.exitCode = 1;
});
child.on("exit", (code, signal) => {
  process.exitCode = signal ? 1 : (code ?? 1);
});
