import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { access, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createServer } from "vite";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const apiPort = process.env.PATHBASE_E2E_API_PORT || "1435";
const webPort = Number(process.env.PATHBASE_E2E_WEB_PORT || "1425");
const binary = path.resolve(
  process.env.PATHBASE_E2E_API_BIN ||
    path.join(root, "api/target/debug/pathbase-api"),
);
await access(binary).catch(() => {
  throw new Error(
    "E2E requires a built Rust API. CI supplies it; for an explicit local run, set PATHBASE_E2E_API_BIN to an existing binary.",
  );
});

const directory = await mkdtemp(path.join(tmpdir(), "pathbase-e2e-"));
// Never inherit cloud credentials, the developer database, or .env files.
for (const key of Object.keys(process.env)) {
  if (/^(PATHBASE_|TACHYON_|FIELD_)/.test(key)) delete process.env[key];
}
Object.assign(process.env, {
  PATHBASE_MODE: "local-preview",
  PATHBASE_DB: path.join(directory, "test.sqlite3"),
  PATHBASE_API_TOKEN: randomBytes(32).toString("hex"),
  PATHBASE_API_PORT: apiPort,
  PATHBASE_SEED_DEMO: "0",
});

let vite;
let stopping = false;
const api = spawn(binary, [], {
  cwd: root,
  env: process.env,
  stdio: "inherit",
});
const exited = new Promise((resolve) => {
  api.once("exit", resolve);
  api.once("error", resolve);
});

async function stop(code = 0) {
  if (stopping) return;
  stopping = true;
  await vite?.close();
  api.kill("SIGINT");
  const force = setTimeout(() => api.kill("SIGKILL"), 3_000);
  await exited;
  clearTimeout(force);
  await rm(directory, { recursive: true, force: true });
  process.exit(code);
}
process.once("SIGINT", () => void stop());
process.once("SIGTERM", () => void stop());
api.once("error", (error) => {
  console.error(error.message);
  void stop(1);
});
api.once("exit", (code) => {
  if (!stopping) {
    console.error(`E2E Rust API exited unexpectedly (${code}).`);
    void stop(1);
  }
});

try {
  let ready = false;
  for (let attempt = 0; attempt < 100 && !stopping; attempt++) {
    try {
      const response = await fetch(`http://127.0.0.1:${apiPort}/v1/me`, {
        headers: { Authorization: `Bearer ${process.env.PATHBASE_API_TOKEN}` },
        signal: AbortSignal.timeout(500),
      });
      if (response.ok && (await response.json()).id === "local-owner") {
        ready = true;
        break;
      }
    } catch {
      // The new process may still be binding its port or initializing SQLite.
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  if (!ready) throw new Error("Isolated E2E Rust API failed to become ready.");
  vite = await createServer({
    root,
    envDir: directory,
    server: { host: "127.0.0.1", port: webPort, strictPort: true },
  });
  await vite.listen();
  console.log(`Isolated E2E app ready at http://127.0.0.1:${webPort}`);
} catch (error) {
  console.error(error.message);
  await stop(1);
}
