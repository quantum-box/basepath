// What ships to an AI host's plugin directory.
//
// A package is published: a mistake here is a mistake everyone downloads. The
// build script enforces the structural rules; these tests cover what a reader
// of the package would check, and the one thing neither can see from the
// files alone — that the shared skills stay shared.
import assert from "node:assert/strict";
import test from "node:test";
import { execFileSync } from "node:child_process";
import { readFile, readdir } from "node:fs/promises";

const url = (path) => new URL(`../${path}`, import.meta.url);
const read = (path) => readFile(url(path), "utf8");
const json = async (path) => JSON.parse(await read(path));

test("the sources pass the package build's own checks", () => {
  // Runs the real script, so a broken manifest fails here rather than in a
  // host's importer.
  const output = execFileSync(
    process.execPath,
    ["scripts/build-plugin.mjs", "--check"],
    { cwd: new URL("..", import.meta.url), encoding: "utf8" },
  );
  assert.match(output, /Plugin sources are valid/);
});

test("the manifest declares the portable fields and the host's own metadata", async () => {
  const manifest = await json("plugin/chatgpt/plugin.json");
  assert.equal(
    manifest.$schema,
    "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
  );
  assert.equal(manifest.name, "basepath");
  assert.match(manifest.version, /^\d+\.\d+\.\d+$/);
  assert.ok(manifest.description.length > 40);
  assert.equal(manifest.skills, "./skills/");

  const openai = manifest.extensions["com.openai"].interface;
  for (const field of [
    "displayName",
    "shortDescription",
    "longDescription",
    "developerName",
    "category",
    "composerIcon",
    "logo",
  ]) {
    assert.ok(openai[field], `${field} is required for the listing`);
  }
  // The listing is where someone decides whether to connect their plan, so it
  // has to say what the AI can and cannot do.
  assert.match(openai.longDescription, /変更案/);
  assert.match(openai.longDescription, /承認/);
});

test("the package points at the hosted MCP endpoint over https and nothing else", async () => {
  const mcp = await json("plugin/chatgpt/mcp.json");
  const servers = Object.values(mcp.mcpServers);
  assert.equal(servers.length, 1);
  assert.equal(servers[0].type, "streamable-http");
  assert.equal(servers[0].url, "https://pathbase-v2.txcloud.app/api/mcp");
});

test("the Claude package declares the same endpoint in its own format", async () => {
  const manifest = await json("plugin/claude/.claude-plugin/plugin.json");
  assert.equal(manifest.name, "basepath");
  assert.ok(manifest.description.length > 40);
  assert.equal(manifest.mcpServers, "./.mcp.json");

  const mcp = await json("plugin/claude/.mcp.json");
  const servers = Object.values(mcp.mcpServers);
  assert.equal(servers.length, 1);
  assert.equal(servers[0].url, "https://pathbase-v2.txcloud.app/api/mcp");

  // Same server, same version, same description as the other host's package:
  // two packages of one thing, not two things.
  const chatgpt = await json("plugin/chatgpt/plugin.json");
  assert.equal(manifest.version, chatgpt.version);
  assert.equal(manifest.description, chatgpt.description);
  assert.equal(
    Object.values((await json("plugin/chatgpt/mcp.json")).mcpServers)[0].url,
    servers[0].url,
  );
});

test("both packages ship the same skills, and so does the server", async () => {
  execFileSync(process.execPath, ["scripts/build-plugin.mjs"], {
    cwd: new URL("..", import.meta.url),
    encoding: "utf8",
  });
  const names = (await readdir(url("skills"), { withFileTypes: true }))
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name);

  for (const name of names) {
    const source = await read(`skills/${name}/SKILL.md`);
    for (const host of ["chatgpt", "claude"]) {
      assert.equal(
        await read(`dist/plugin/${host}/skills/${name}/SKILL.md`),
        source,
        `${host}: the packaged skill must be the shared one`,
      );
    }
    // And the binary embeds the same file, so a host reading it over MCP and a
    // host reading it from a package cannot get different instructions.
    assert.match(
      await read("api/src/skills.rs"),
      new RegExp(`skills/${name}/SKILL\\.md`),
      `${name}: is not embedded in the MCP server`,
    );
  }
});

test("the in-conversation app depends on no host's private API", async () => {
  // The same bundle renders in every host that supports MCP Apps. It talks to
  // the host over the published AppBridge protocol; reaching for a global one
  // product happens to provide would quietly make it that product's app.
  const bundle = await read("api/ui/mcp-app.html");
  for (const forbidden of [
    "window.openai",
    "window.anthropic",
    "webkit.messageHandlers",
  ]) {
    assert.ok(
      !bundle.includes(forbidden),
      `the MCP App bundle reaches for ${forbidden}`,
    );
  }
  // What it does use.
  assert.match(bundle, /ui\/initialize/);
});

test("nothing in the package is a credential", async () => {
  const files = ["plugin/chatgpt/plugin.json", "plugin/chatgpt/mcp.json"];
  for (const file of files) {
    const source = await read(file);
    assert.doesNotMatch(source, /client_secret/i);
    assert.doesNotMatch(source, /api[_-]?key/i);
    assert.doesNotMatch(source, /Bearer\s+\S{20,}/);
  }
  // The connection is registered dynamically by the host, so there is no
  // client id to embed either — and an embedded one would be shared by every
  // person who installed the package.
  const manifest = await read("plugin/chatgpt/plugin.json");
  assert.doesNotMatch(manifest, /client_id/);
});

test("every skill says when it applies and names only Basepath's own tools", async () => {
  const names = (await readdir(url("skills"), { withFileTypes: true }))
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name);
  assert.deepEqual(names.sort(), [
    "basepath-goal-breakdown",
    "basepath-record-progress",
    "basepath-week-planning",
    "basepath-weekly-review",
  ]);

  for (const name of names) {
    const source = await read(`skills/${name}/SKILL.md`);
    assert.ok(source.startsWith("---\n"), `${name}: frontmatter`);
    assert.match(source, new RegExp(`\\nname: ${name}\\n`), `${name}: name`);
    const description = source.match(/\ndescription: (.+)\n/)?.[1] ?? "";
    assert.ok(description.length > 40, `${name}: description`);
    // Naming a host here is what forces a second copy for the next one.
    assert.doesNotMatch(
      source,
      /\b(ChatGPT|OpenAI|Claude|Anthropic)\b/,
      `${name}: host-specific wording belongs in plugin/<host>/`,
    );
    // Every skill has to reach the plan through the MCP tools, not through
    // some endpoint it made up.
    assert.match(source, /pathbase_[a-z_]+/, `${name}: names no tool`);
    for (const tool of source.match(/pathbase_[a-z_]+/g) ?? []) {
      assert.ok(KNOWN_TOOLS.has(tool), `${name}: unknown tool ${tool}`);
    }
  }
});

test("the skills that write say the change is a proposal", async () => {
  for (const name of [
    "basepath-goal-breakdown",
    "basepath-week-planning",
    "basepath-record-progress",
    "basepath-weekly-review",
  ]) {
    const source = await read(`skills/${name}/SKILL.md`);
    assert.match(
      source,
      /pathbase_preview_changes|pathbase_record|pathbase_complete_action/,
    );
    // The one rule none of them may leave out: approval happens in Basepath,
    // and the model cannot stand in for it.
    assert.match(
      source,
      /承認|approv/i,
      `${name}: must say where a change becomes real`,
    );
  }
  // And none of them may tell the model to apply on its own say-so.
  const review = await read("skills/basepath-weekly-review/SKILL.md");
  assert.match(review, /Finalizing is not proposable/);
});

/** The tools the MCP server actually exposes, read from its own source. */
const KNOWN_TOOLS = new Set(
  (await read("api/src/mcp.rs"))
    .match(/"pathbase_[a-z_]+"/g)
    .map((quoted) => quoted.slice(1, -1)),
);
