import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const productionFieldOperator = "tn_01j91h09tpj5ehwbwfwfxpak2b";

test("production delegates Field authorization through the Field directory root", async () => {
  const [manifest, example] = await Promise.all([
    readFile(new URL("../tachyon.yml", import.meta.url), "utf8"),
    readFile(new URL("../.env.example", import.meta.url), "utf8"),
  ]);

  assert.match(
    manifest,
    new RegExp(
      `name: FIELD_OPERATOR_ID\\n(?:[ \\t]*#.*\\n)*[ \\t]*value: ${productionFieldOperator}`,
    ),
  );
  assert.match(
    example,
    new RegExp(`^FIELD_OPERATOR_ID=${productionFieldOperator}$`, "m"),
  );
  assert.doesNotMatch(
    manifest,
    /name: FIELD_OPERATOR_ID\n(?:[ \t]*#.*\n)*[ \t]*value: tn_01m2a6ztfmd95p70gwgyshbevq/,
  );
});

async function apps() {
  const { parseAllDocuments } = await import("yaml");
  const raw = await readFile(
    new URL("../tachyon.yml", import.meta.url),
    "utf8",
  );
  const cloud = parseAllDocuments(raw)
    .map((document) => document.toJS())
    .find((document) => document?.kind === "CloudApps");
  assert.ok(cloud, "the manifest must declare a CloudApps document");
  return Object.fromEntries(cloud.spec.apps.map((app) => [app.name, app]));
}

test("only the Rust API declares a managed database", async () => {
  const declared = await apps();
  const api = declared["pathbase-api"];
  const worker = declared["pathbase-v2"];

  // Tachyon issues the database, user, grant, and DSN secret. The manifest
  // must not name a secret path or carry a literal DSN.
  assert.deepEqual(api.provisionedDatabase, {
    provider: "tidb",
    engine: "mysql",
    envVar: "DATABASE_URL",
  });
  // The static Worker never receives a database secret.
  assert.equal(worker.provisionedDatabase, undefined);
  assert.equal(worker.environments, undefined);
});

test("previews get their own database and never share production's", async () => {
  const api = (await apps())["pathbase-api"];

  assert.deepEqual(api.environments.preview.provisionedDatabase, {
    provider: "tidb",
    engine: "mysql",
    envVar: "DATABASE_URL",
  });
  // ADR-0049: opting a preview into the production database has to be an
  // explicit declaration, and this app must never make it.
  assert.equal(
    api.provisionedDatabase.previewSharesProductionDatabase,
    undefined,
  );
  assert.equal(
    api.environments.preview.provisionedDatabase
      .previewSharesProductionDatabase,
    undefined,
  );
});

test("every environment labels the database it is allowed to use", async () => {
  const api = (await apps())["pathbase-api"];
  const label = (environment) =>
    (api.environments[environment].envVars ?? []).find(
      (variable) => variable.name === "PATHBASE_DB_ENVIRONMENT",
    )?.value;

  // Declaring an overlay makes the platform require one per environment, and
  // the label is what lets a process refuse a database another deployment
  // claimed.
  assert.equal(label("production"), "production");
  assert.equal(label("preview"), "preview");
  // The base list must not carry the label, or the two would conflict.
  assert.equal(
    api.envVars.find((variable) => variable.name === "PATHBASE_DB_ENVIRONMENT"),
    undefined,
  );
});

test("the API readiness proof reaches the database", async () => {
  const api = (await apps())["pathbase-api"];

  // A plain 200 does not prove durability; readiness has to report the
  // applied schema. `api/tests/deployment_gate.rs` asserts the response body
  // that this matcher depends on.
  assert.deepEqual(api.readinessProof, {
    path: "/health/ready",
    expectedStatus: 200,
    expectedBody: '"schema":"current"',
  });
});

test("no local SQLite path survives in the deployed API", async () => {
  const [manifest, example] = await Promise.all([
    readFile(new URL("../tachyon.yml", import.meta.url), "utf8"),
    readFile(new URL("../.env.example", import.meta.url), "utf8"),
  ]);

  // The Lambda must not be told to use a per-execution-environment file.
  assert.doesNotMatch(manifest, /name: PATHBASE_DB\b/);
  assert.doesNotMatch(manifest, /pathbase\.sqlite3/);
  // A DSN must never be committed, in the manifest or the example file.
  assert.doesNotMatch(manifest, /mysql:\/\//);
  assert.doesNotMatch(example, /^DATABASE_URL=.+$/m);
});
