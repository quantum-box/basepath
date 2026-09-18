import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { createConnection } from "node:net";
import { createServer, loadEnv } from "vite";
import { fileURLToPath } from "node:url";
import path from "node:path";
// The API crate ships two binaries. Local development always runs the plain
// HTTP server; `lambda-pathbase-api` only speaks the Lambda runtime protocol
// and exits immediately outside AWS.
import { API_BINARY, cargoRunArgs } from "./api-binary.mjs";
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
for (const [key, value] of Object.entries(loadEnv("development", root, ""))) {
  if (/^(PATHBASE_|TACHYON_|FIELD_)/.test(key) && value.trim())
    process.env[key] ??= value;
}
process.env.PATHBASE_API_PORT ||= "1431";
process.env.PATHBASE_MODE ||= "local-preview";
const localPreview = process.env.PATHBASE_MODE === "local-preview";
if (localPreview) {
  // Ephemeral credentials and the on-disk SQLite file belong to the explicit
  // local preview only. A real PATHBASE_MODE must bring its own configuration.
  process.env.PATHBASE_API_TOKEN ||= randomBytes(32).toString("hex");
  process.env.PATHBASE_DB ||= path.join(root, "data", "pathbase.sqlite3");
  process.env.PATHBASE_SEED_DEMO ||= "1";
} else {
  // A real mode authenticates through Tachyon cookies. Never mint a local
  // owner credential, seed demo data, or fall back to the developer database.
  console.log(
    `PATHBASE_MODE=${process.env.PATHBASE_MODE}: local-preview の一時トークン・サンプルデータ・既定SQLiteは使用しません。`,
  );
}
const native = process.argv.includes("--native");
const apiPort = Number(process.env.PATHBASE_API_PORT);
const webPort = Number(process.env.PATHBASE_WEB_PORT || "1420");
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

function portInUse(port) {
  return new Promise((resolve) => {
    const socket = createConnection({ host: "127.0.0.1", port });
    const settle = (value) => {
      socket.destroy();
      resolve(value);
    };
    socket.setTimeout(500);
    socket.once("connect", () => settle(true));
    socket.once("timeout", () => settle(false));
    socket.once("error", () => settle(false));
  });
}

// Refuse to start against a port another process already owns: the Rust
// server would exit and leave Vite talking to a stranger's API.
for (const [label, port] of [
  ["Rust API", native ? null : apiPort],
  ["Vite", webPort],
]) {
  if (port === null) continue;
  if (await portInUse(port)) {
    console.error(
      `ポート ${port} は既に使用されています（${label}）。既存のプロセスを停止するか、PATHBASE_API_PORT / PATHBASE_WEB_PORT を変更してください。`,
    );
    process.exit(1);
  }
}

if (!native) {
  api = spawn("cargo", cargoRunArgs(), {
    cwd: root,
    env: process.env,
    stdio: "inherit",
  });
  api.on("error", (error) => {
    console.error(`${API_BINARY} を起動できません: ${error.message}`);
    stop(1);
  });
  api.on("exit", (code, signal) => {
    if (!stopping) {
      console.error(
        `${API_BINARY} が予期せず終了しました (code=${code ?? "null"}, signal=${signal ?? "null"})。Viteは起動しません。`,
      );
      stop(code || 1);
    }
  });
  let healthy = false;
  for (let i = 0; i < 600; i++) {
    if (api.exitCode !== null) break;
    try {
      const health = await fetch(`http://127.0.0.1:${apiPort}/health`, {
        signal: AbortSignal.timeout(1000),
      });
      if (health.ok && (await health.json()).status === "ok") {
        const response = await fetch(`http://127.0.0.1:${apiPort}/v1/me`, {
          headers: {
            Authorization: `Bearer ${process.env.PATHBASE_API_TOKEN}`,
          },
          signal: AbortSignal.timeout(1000),
        });
        if (
          (response.ok && (await response.json()).id === "local-owner") ||
          response.status === 401
        ) {
          healthy = true;
          break;
        }
      }
    } catch {}
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  if (!healthy) {
    console.error(`${API_BINARY} が応答しませんでした。`);
    stop(1);
  }
}
try {
  vite = await createServer({
    root,
    server: { port: webPort, strictPort: true },
  });
  await vite.listen();
  vite.printUrls();
} catch (error) {
  console.error(`画面を起動できません: ${error.message}`);
  stop(1);
}
