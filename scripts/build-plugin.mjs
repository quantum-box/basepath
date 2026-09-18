#!/usr/bin/env node
/**
 * Assembles a distributable plugin package for one AI host.
 *
 * The workflows live once, in `skills/`, and are copied into every host's
 * package. A rule about how to work with someone's plan does not change
 * because the conversation is happening somewhere else, so it is not written
 * twice — only the manifest, the connection URL and the artwork are.
 *
 * `--check` verifies the sources without writing, which is what CI runs.
 */
import {
  cpSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
} from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const hostsDir = path.join(root, "plugin");
const skillsDir = path.join(root, "skills");
const outRoot = path.join(root, "dist", "plugin");

const problems = [];
const fail = (message) => problems.push(message);

/** Reads the YAML frontmatter a SKILL.md must start with. */
function frontmatter(source, where) {
  if (!source.startsWith("---\n")) {
    fail(`${where}: SKILL.md must start with YAML frontmatter`);
    return {};
  }
  const end = source.indexOf("\n---\n", 3);
  if (end === -1) {
    fail(`${where}: the frontmatter block is not closed`);
    return {};
  }
  const fields = {};
  for (const line of source.slice(4, end).split("\n")) {
    const separator = line.indexOf(":");
    if (separator === -1) continue;
    fields[line.slice(0, separator).trim()] = line.slice(separator + 1).trim();
  }
  return fields;
}

/**
 * Host names that must not appear in a shared skill.
 *
 * A skill that says "in ChatGPT" is a skill that has to be rewritten for the
 * next host, which is the duplication this layout exists to prevent.
 */
const HOST_NAMES = /\b(ChatGPT|OpenAI|Claude|Anthropic|Gemini|Copilot)\b/;

function readSkills() {
  const skills = [];
  for (const entry of readdirSync(skillsDir, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const file = path.join(skillsDir, entry.name, "SKILL.md");
    if (!existsSync(file)) {
      fail(`skills/${entry.name}: SKILL.md is missing`);
      continue;
    }
    const source = readFileSync(file, "utf8");
    const fields = frontmatter(source, `skills/${entry.name}`);
    if (fields.name !== entry.name) {
      fail(
        `skills/${entry.name}: frontmatter name is ${JSON.stringify(fields.name)}, which does not match the directory`,
      );
    }
    if (!fields.description) {
      fail(
        `skills/${entry.name}: a description decides when the skill is used`,
      );
    } else if (fields.description.length < 40) {
      fail(
        `skills/${entry.name}: the description must say what the workflow is and when it applies`,
      );
    }
    const host = source.match(HOST_NAMES);
    if (host) {
      fail(
        `skills/${entry.name}: names the host ${host[0]}; host-specific wording belongs in plugin/<host>/`,
      );
    }
    skills.push(entry.name);
  }
  if (skills.length === 0) fail("skills/: no skill was found");
  return skills;
}

/**
 * Where each host keeps its manifest, and where the skills go inside its
 * package.
 *
 * This is the whole host-specific surface. Everything else — the workflows,
 * the rules, the tool names — is shared, so adding a host means adding a row
 * here rather than a second copy of any of it.
 */
const LAYOUTS = {
  chatgpt: {
    manifest: "plugin.json",
    mcp: "mcp.json",
    transport: "streamable-http",
    /** Returns the assets the manifest points at, so they can be checked. */
    check: (manifest, host) => {
      if (!manifest.$schema) fail(`plugin/${host}: $schema is required`);
      if (manifest.skills !== "./skills/") {
        fail(`plugin/${host}: skills must point at the bundled "./skills/"`);
      }
      const openai = manifest.extensions?.["com.openai"]?.interface ?? {};
      for (const key of ["composerIcon", "logo"]) {
        if (!openai[key]) {
          fail(
            `plugin/${host}: extensions.com.openai.interface.${key} is required`,
          );
        }
      }
      return [openai.composerIcon, openai.logo].filter(Boolean);
    },
  },
  claude: {
    manifest: ".claude-plugin/plugin.json",
    mcp: ".mcp.json",
    transport: "http",
    check: (manifest, host) => {
      if (manifest.mcpServers !== "./.mcp.json") {
        fail(
          `plugin/${host}: mcpServers must point at the bundled "./.mcp.json"`,
        );
      }
      return [];
    },
  },
};

function readHost(host) {
  const dir = path.join(hostsDir, host);
  const layout = LAYOUTS[host];
  if (!layout) {
    fail(
      `plugin/${host}: no layout is defined for this host in scripts/build-plugin.mjs`,
    );
    return null;
  }
  const manifestPath = path.join(dir, layout.manifest);
  if (!existsSync(manifestPath)) {
    fail(`plugin/${host}: ${layout.manifest} is missing`);
    return null;
  }
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  for (const field of ["name", "version", "description"]) {
    if (!manifest[field]) {
      fail(`plugin/${host}/${layout.manifest}: ${field} is required`);
    }
  }
  if (manifest.name && !/^[a-z0-9]+(-[a-z0-9]+)*$/.test(manifest.name)) {
    fail(`plugin/${host}/${layout.manifest}: name must be kebab-case`);
  }
  // An asset a manifest points at has to exist, or the listing renders with a
  // hole in it.
  for (const asset of layout.check(manifest, host) ?? []) {
    if (!existsSync(path.join(dir, asset))) {
      fail(`plugin/${host}: ${asset} is declared but not present`);
    }
  }

  const mcpPath = path.join(dir, layout.mcp);
  if (!existsSync(mcpPath)) {
    fail(`plugin/${host}: ${layout.mcp} is missing`);
  } else {
    const mcp = JSON.parse(readFileSync(mcpPath, "utf8"));
    const servers = Object.entries(mcp.mcpServers ?? {});
    if (servers.length === 0) {
      fail(`plugin/${host}/${layout.mcp}: no MCP server is declared`);
    }
    for (const [name, server] of servers) {
      if (server.type !== layout.transport) {
        fail(
          `plugin/${host}/${layout.mcp}: ${name} must use the ${layout.transport} transport`,
        );
      }
      if (!server.url?.startsWith("https://")) {
        fail(
          `plugin/${host}/${layout.mcp}: ${name} must be reached over https`,
        );
      }
    }
  }
  // A package is published; a secret in one is a secret everyone has.
  for (const file of walk(dir)) {
    const source = readFileSync(file, "utf8");
    if (
      /(client_secret|api[_-]?key|BEGIN [A-Z ]*PRIVATE KEY|Bearer\s+[A-Za-z0-9._-]{20,})/.test(
        source,
      )
    ) {
      fail(`${path.relative(root, file)}: looks like it contains a credential`);
    }
  }
  return manifest;
}

function walk(dir) {
  const files = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) files.push(...walk(full));
    else files.push(full);
  }
  return files;
}

const check = process.argv.includes("--check");
const skills = readSkills();
const hosts = existsSync(hostsDir)
  ? readdirSync(hostsDir, { withFileTypes: true })
      .filter((entry) => entry.isDirectory())
      .map((entry) => entry.name)
  : [];
if (hosts.length === 0) fail("plugin/: no host package was found");

const manifests = {};
for (const host of hosts) manifests[host] = readHost(host);

if (problems.length > 0) {
  for (const problem of problems) console.error(`error: ${problem}`);
  process.exit(1);
}

if (check) {
  console.log(
    `Plugin sources are valid: ${hosts.length} host package(s), ${skills.length} shared skill(s).`,
  );
  process.exit(0);
}

rmSync(outRoot, { recursive: true, force: true });
for (const host of hosts) {
  const out = path.join(outRoot, host);
  mkdirSync(out, { recursive: true });
  cpSync(path.join(hostsDir, host), out, { recursive: true });
  cpSync(skillsDir, path.join(out, "skills"), { recursive: true });
  console.log(
    `Wrote ${path.relative(root, out)} (${manifests[host].name}@${manifests[host].version}, ${skills.length} skills).`,
  );
}
