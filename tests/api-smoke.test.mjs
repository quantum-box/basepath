// Lightweight startup smoke test for the plain HTTP API binary.
//
// It answers three questions the browser suite does not:
//   1. do the dev scripts start `pathbase-api` rather than the Lambda binary?
//   2. does a clean local-preview start serve /health and an authenticated read?
//   3. do misconfiguration and shutdown produce a detectable exit instead of a
//      half-running process?
import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { readFile, access, mkdtemp, rm } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import assert from "node:assert/strict";
import test from "node:test";
import {
  API_BINARY,
  LAMBDA_BINARY,
  cargoRunArgs,
} from "../scripts/api-binary.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (relative) => readFile(path.join(root, relative), "utf8");

test("cargo run arguments name the plain HTTP binary", () => {
  assert.deepEqual(cargoRunArgs(), [
    "run",
    "--manifest-path",
    "api/Cargo.toml",
    "--bin",
    API_BINARY,
  ]);
  assert.deepEqual(
    cargoRunArgs(["--preflight"], { quiet: true, locked: true }),
    [
      "run",
      "--quiet",
      "--locked",
      "--manifest-path",
      "api/Cargo.toml",
      "--bin",
      API_BINARY,
      "--",
      "--preflight",
    ],
  );
});

test("package scripts and dev tooling select a binary explicitly", async () => {
  const pkg = JSON.parse(await read("package.json"));
  for (const name of ["api", "api:mcp"]) {
    assert.match(
      pkg.scripts[name],
      new RegExp(`--bin ${API_BINARY}\\b`),
      `npm run ${name} must name --bin ${API_BINARY}`,
    );
    assert.doesNotMatch(pkg.scripts[name], new RegExp(LAMBDA_BINARY));
  }
  for (const script of ["scripts/dev.mjs", "scripts/preflight.mjs"]) {
    const source = await read(script);
    assert.match(
      source,
      /cargoRunArgs\(/,
      `${script} must go through scripts/api-binary.mjs`,
    );
    assert.doesNotMatch(
      source,
      /"run",\s*"--manifest-path"/,
      `${script} must not spawn an ambiguous cargo run`,
    );
  }
});

test("the Lambda entrypoint stays a separate binary", async () => {
  const manifest = await read("api/Cargo.toml");
  assert.match(manifest, new RegExp(`name = "${LAMBDA_BINARY}"`));
  assert.match(manifest, new RegExp(`name = "${API_BINARY}"`));
  const e2e = await read("scripts/e2e-server.mjs");
  assert.match(e2e, new RegExp(`target/debug/${API_BINARY}`));
});

test("the production Lambda uses the shared session store", async () => {
  const source = await read("api/src/bin/lambda.rs");
  assert.match(
    source,
    /TachyonAuth::for_runtime_from_env[\s\S]*?\.with_database\(service\.db\.clone\(\)\)/,
    "the production auth client must be connected to the shared DB so sessions can refresh",
  );
});

const binary = path.resolve(
  process.env.PATHBASE_SMOKE_API_BIN ||
    path.join(root, `api/target/debug/${API_BINARY}`),
);
const built = await access(binary).then(
  () => true,
  () => false,
);

async function freePort() {
  const server = createServer();
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const { port } = server.address();
  await new Promise((resolve) => server.close(resolve));
  return port;
}

// Never inherit the developer's cloud credentials or database.
function cleanEnv(overrides) {
  const env = {};
  for (const [key, value] of Object.entries(process.env)) {
    if (!/^(PATHBASE_|TACHYON_|FIELD_)/.test(key)) env[key] = value;
  }
  return Object.assign(env, overrides);
}

async function startApi(overrides = {}) {
  const directory = await mkdtemp(path.join(tmpdir(), "pathbase-smoke-"));
  const port = String(await freePort());
  const token = randomBytes(32).toString("hex");
  const child = spawn(binary, [], {
    cwd: root,
    env: cleanEnv({
      PATHBASE_MODE: "local-preview",
      PATHBASE_DB: path.join(directory, "smoke.sqlite3"),
      PATHBASE_API_TOKEN: token,
      PATHBASE_API_PORT: port,
      PATHBASE_SEED_DEMO: "0",
      ...overrides,
    }),
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stderr = "";
  child.stderr.on("data", (chunk) => (stderr += chunk));
  child.stdout.resume();
  const state = { exited: false };
  const exited = new Promise((resolve) =>
    child.once("exit", (code, signal) => {
      state.exited = true;
      resolve({ code, signal });
    }),
  );
  return {
    child,
    port,
    token,
    exited,
    state,
    stderr: () => stderr,
    async cleanup() {
      child.kill("SIGKILL");
      await exited.catch(() => {});
      await rm(directory, { recursive: true, force: true });
    },
  };
}

async function waitForHealth(port, state) {
  for (let attempt = 0; attempt < 100; attempt++) {
    if (state.exited) return null;
    try {
      const response = await fetch(`http://127.0.0.1:${port}/health`, {
        signal: AbortSignal.timeout(500),
      });
      if (response.ok) return await response.json();
    } catch {
      // The process may still be binding its port.
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  return null;
}

test(
  "a clean local-preview start serves health and an authenticated read",
  { skip: built ? false : `built ${API_BINARY} not available` },
  async (t) => {
    const api = await startApi();
    t.after(() => api.cleanup());

    const health = await waitForHealth(api.port, api.state);
    assert.ok(health, `API never became healthy: ${api.stderr()}`);
    assert.equal(health.status, "ok");
    assert.equal(health.service, "pathbase-api");

    const me = await fetch(`http://127.0.0.1:${api.port}/v1/me`, {
      headers: { Authorization: `Bearer ${api.token}` },
    });
    assert.equal(me.status, 200);
    assert.equal((await me.json()).id, "local-owner");

    const anonymous = await fetch(`http://127.0.0.1:${api.port}/v1/me`);
    assert.equal(anonymous.status, 401);

    // A terminating signal must stop the process, not leave it listening.
    api.child.kill("SIGTERM");
    const { code, signal } = await api.exited;
    assert.ok(
      code === 0 || signal === "SIGTERM",
      `unexpected shutdown: code=${code} signal=${signal}`,
    );
  },
);

test(
  "a rejected configuration exits instead of listening",
  { skip: built ? false : `built ${API_BINARY} not available` },
  async (t) => {
    // local-preview requires at least 32 characters of API token.
    const api = await startApi({ PATHBASE_API_TOKEN: "too-short" });
    t.after(() => api.cleanup());

    const { code } = await api.exited;
    assert.notEqual(code, 0);
    await assert.rejects(
      fetch(`http://127.0.0.1:${api.port}/health`, {
        signal: AbortSignal.timeout(500),
      }),
      "the rejected configuration must leave nothing listening",
    );
  },
);
