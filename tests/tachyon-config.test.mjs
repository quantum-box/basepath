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

test("the MCP endpoint is enabled by declaration, with no OAuth client of its own", async () => {
  const { parseAllDocuments } = await import("yaml");
  const raw = await readFile(
    new URL("../tachyon.yml", import.meta.url),
    "utf8",
  );
  const documents = parseAllDocuments(raw).map((document) => document.toJS());
  const clients = documents.filter(
    (document) => document?.kind === "OAuth2Client",
  );
  const names = clients.map((client) => client.metadata.name);
  assert.ok(names.includes("pathbase-local"), "web sign-in client");
  // Basepath issues the MCP tokens itself, so there is no platform OAuth
  // client for AI hosts to be registered against. One that existed but was
  // unused would only be a credential nobody was watching.
  assert.equal(
    names.filter((name) => name.includes("mcp")).length,
    0,
    "the MCP endpoint must not carry an unused identity-provider client",
  );

  // Every declared client is public and uses PKCE: no secret is committed.
  for (const client of clients) {
    assert.equal(client.spec.clientType, "public");
    assert.equal(client.spec.clientSecret, undefined);
    assert.ok(client.spec.grantTypes.includes("authorization_code"));
  }

  // The endpoint exists only where it is declared, and the declaration is a
  // plain switch rather than a credential reference.
  const api = (await apps())["pathbase-api"];
  const enabled = api.envVars.find(
    (variable) => variable.name === "PATHBASE_MCP_ENABLED",
  );
  assert.equal(enabled.value, "1");
  assert.equal(enabled.type, undefined);
  assert.equal(
    api.envVars.find((variable) => variable.name === "PATHBASE_MCP_CLIENT_ID"),
    undefined,
    "the removed variable must not linger",
  );
});

test("production and preview are different MCP resources", async () => {
  const api = (await apps())["pathbase-api"];
  const resource = (environment) =>
    (api.environments[environment].envVars ?? []).find(
      (variable) => variable.name === "PATHBASE_MCP_RESOURCE",
    )?.value;

  const production = resource("production");
  const preview = resource("preview");
  assert.ok(production, "production declares its MCP resource");
  assert.ok(preview, "preview declares its MCP resource");
  // A connection approved against one must not be a token for the other.
  assert.notEqual(production, preview);
  // The base list must not fix a resource, or the overlays would be moot.
  assert.equal(
    api.envVars.find((variable) => variable.name === "PATHBASE_MCP_RESOURCE"),
    undefined,
  );
});

test("no MCP shared secret survives anywhere", async () => {
  const [manifest, example] = await Promise.all([
    readFile(new URL("../tachyon.yml", import.meta.url), "utf8"),
    readFile(new URL("../.env.example", import.meta.url), "utf8"),
  ]);
  // The fixed-token mode is gone: an MCP client authenticates as the person.
  for (const source of [manifest, example]) {
    assert.doesNotMatch(source, /PATHBASE_MCP_TOKEN/);
    assert.doesNotMatch(source, /PATHBASE_MCP_ACTOR_ID/);
  }
});

test("the MCP host check is off, because the deployment sits behind proxies", async () => {
  // `Host` is whatever the last hop addressed. This process is reached through
  // a Worker and an API gateway, each rewriting it to a name the deployment
  // neither chooses nor can enumerate — so an allowlist of names cannot be
  // written correctly here, and a wrong one refuses every real request *after*
  // authentication has succeeded, which reads as an approved connection that
  // does nothing.
  //
  // What stops a rebinding attempt is the token: an untokened request gets a
  // 401 whatever Host it claims, the same answer it gets by calling the URL
  // directly. `api/tests/mcp_remote.rs` holds that behaviour.
  const declared = await apps();
  for (const environment of ["production", "preview"]) {
    const allowed = declared["pathbase-api"].environments[
      environment
    ].envVars.find(
      (variable) => variable.name === "PATHBASE_MCP_ALLOWED_HOSTS",
    ).value;
    assert.equal(String(allowed).trim(), "*", environment);
  }
});
