import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { createServer, loadEnv } from "vite";
import { fileURLToPath } from "node:url";
import path from "node:path";
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
for (const [key, value] of Object.entries(loadEnv("development", root, ""))) {
  if (/^(PATHBASE_|TACHYON_|FIELD_)/.test(key) && value.trim())
    process.env[key] ??= value;
}
process.env.PATHBASE_API_TOKEN ||= randomBytes(32).toString("hex");
process.env.PATHBASE_API_PORT ||= "1431";
process.env.PATHBASE_DB ||= path.join(root, "data", "pathbase.sqlite3");
process.env.PATHBASE_MODE ||= "local-preview";
if (process.env.PATHBASE_MODE === "local-preview")
  process.env.PATHBASE_SEED_DEMO ||= "1";
const native = process.argv.includes("--native");
let api;
let vite;
let stopping = false;
function stop(code = 0) {
  if (stopping) return;
  stopping = true;
  api?.kill("SIGTERM");
  void vite?.close().finally(() => process.exit(code));
  if (!vite) process.exit(code);
}
process.on("SIGINT", () => stop());
process.on("SIGTERM", () => stop());
if (!native) {
  api = spawn("cargo", ["run", "--manifest-path", "api/Cargo.toml"], {
    cwd: root,
    env: process.env,
    stdio: "inherit",
  });
  api.on("error", (error) => {
    console.error(error.message);
    stop(1);
  });
  api.on("exit", (code) => {
    if (!stopping) stop(code || 1);
  });
  let healthy = false;
  for (let i = 0; i < 600; i++) {
    if (api.exitCode !== null) break;
    try {
      const response = await fetch(
        `http://127.0.0.1:${process.env.PATHBASE_API_PORT}/v1/me`,
        {
          headers: {
            Authorization: `Bearer ${process.env.PATHBASE_API_TOKEN}`,
          },
          signal: AbortSignal.timeout(1000),
        },
      );
      if (
        (response.ok && (await response.json()).id === "local-owner") ||
        response.status === 401
      ) {
        healthy = true;
        break;
      }
    } catch {}
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  if (!healthy) {
    console.error("Rust API failed to become ready.");
    stop(1);
  }
}
vite = await createServer({ root });
await vite.listen();
vite.printUrls();
