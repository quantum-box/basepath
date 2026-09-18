#!/usr/bin/env node
/**
 * The shipping gate.
 *
 * A pull request can change the code and the tests in the same commit, so
 * "CI passed" only means something if the checks that must pass are named
 * somewhere a pull request cannot quietly narrow. Two things hold that line:
 *
 * 1. **Branch protection on `main`** names the required checks. It lives in
 *    the repository settings, not in a file this repository can edit, so a
 *    pull request cannot remove a requirement by editing itself.
 * 2. **This script**, which runs inside CI and asserts that the workflow still
 *    declares every job branch protection requires, and that the suites those
 *    jobs are the gate *for* have not been emptied.
 *
 * Neither is sufficient alone. (1) cannot see whether the job still runs the
 * tests; (2) can be edited by the pull request it is checking. Together, a
 * change that weakens the gate has to either fail here or visibly remove a
 * required check in the repository settings, which is not something a diff
 * can do.
 *
 * This is a check, not a ceremony: every rule below exists because losing it
 * would let something ship unverified.
 */
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (file) => readFileSync(path.join(root, file), "utf8");

const problems = [];
const fail = (message) => problems.push(message);

const workflow = read(".github/workflows/ci.yml");

/**
 * Jobs that must exist, and what each one is the gate for.
 *
 * The names are the ones branch protection requires; `docs/release-checklist.md`
 * lists the same set, and a mismatch between the two is itself a finding.
 */
const REQUIRED_JOBS = [
  {
    name: "Web build and Sites tests",
    mustRun: [
      "npm run check",
      "npm run build",
      "npm run test:sites",
      "npm run test:config",
      "npm run test:view",
      "npm run test:plugin",
      "npm run check:mcp-app",
      "npm run check:plugin",
    ],
  },
  {
    name: "Rust API tests and lint",
    mustRun: ["cargo", "test", "clippy", "fmt"],
  },
  {
    name: "Shared TiDB behaviour",
    // The storage the product actually runs on. Without this in the gate, the
    // contract that survives a redeploy is checked only against SQLite.
    mustRun: [
      "--test tidb",
      "--test acceptance",
      "--test durability",
      "--test migration",
      "--test database_configuration",
      "--test deployment_gate",
    ],
  },
  {
    name: "Browser integration tests",
    mustRun: ["playwright"],
  },
  { name: "Tauri compile check", mustRun: ["cargo"] },
];

for (const job of REQUIRED_JOBS) {
  if (!workflow.includes(`name: ${job.name}`)) {
    fail(`the workflow no longer declares the required job "${job.name}"`);
    continue;
  }
  for (const fragment of job.mustRun) {
    if (!workflow.includes(fragment)) {
      fail(`"${job.name}" no longer runs ${JSON.stringify(fragment)}`);
    }
  }
}

/**
 * Suites whose disappearance would be invisible in a green build.
 *
 * A deleted test file makes CI pass faster. Each entry names the file and the
 * smallest count of cases that has ever been true of it, so shrinking one is a
 * deliberate act with a diff, not an accident.
 */
const SUITES = [
  { file: "api/tests/acceptance.rs", pattern: /#\[tokio::test\]/g, least: 3 },
  { file: "api/tests/approval.rs", pattern: /#\[tokio::test\]/g, least: 6 },
  {
    file: "api/tests/oauth.rs",
    pattern: /#\[tokio::test\]|#\[test\]/g,
    least: 12,
  },
  { file: "api/tests/injection.rs", pattern: /#\[tokio::test\]/g, least: 5 },
  { file: "api/tests/planning.rs", pattern: /#\[tokio::test\]/g, least: 10 },
  { file: "api/tests/alignment.rs", pattern: /#\[tokio::test\]/g, least: 9 },
  { file: "api/tests/dashboard.rs", pattern: /#\[tokio::test\]/g, least: 11 },
  { file: "api/tests/checkin.rs", pattern: /#\[tokio::test\]/g, least: 9 },
  { file: "api/tests/memory.rs", pattern: /#\[tokio::test\]/g, least: 11 },
  { file: "api/tests/tidb.rs", pattern: /#\[tokio::test\]/g, least: 9 },
  {
    file: "api/tests/collaboration.rs",
    pattern: /#\[tokio::test\]/g,
    least: 7,
  },
  {
    file: "api/tests/skills_over_mcp.rs",
    pattern: /#\[tokio::test\]/g,
    least: 4,
  },
  { file: "api/tests/mcp_remote.rs", pattern: /#\[tokio::test\]/g, least: 2 },
  { file: "tests/e2e/oauth-consent.spec.mjs", pattern: /^test\(/gm, least: 7 },
  {
    file: "tests/e2e/change-approval.spec.mjs",
    pattern: /^test\(/gm,
    least: 3,
  },
  { file: "tests/e2e/mcp-app.spec.mjs", pattern: /^test\(/gm, least: 23 },
  { file: "tests/e2e/workspace.spec.mjs", pattern: /^test\(/gm, least: 11 },
  { file: "tests/e2e/planning.spec.mjs", pattern: /^test\(/gm, least: 5 },
  { file: "tests/e2e/alignment.spec.mjs", pattern: /^test\(/gm, least: 5 },
  { file: "tests/e2e/dashboard.spec.mjs", pattern: /^test\(/gm, least: 6 },
  { file: "tests/e2e/review.spec.mjs", pattern: /^test\(/gm, least: 5 },
  { file: "tests/e2e/memory.spec.mjs", pattern: /^test\(/gm, least: 6 },
  { file: "tests/plugin.test.mjs", pattern: /^test\(/gm, least: 9 },
  { file: "tests/sites-worker.test.mjs", pattern: /^test\(/gm, least: 12 },
];

for (const suite of SUITES) {
  let source;
  try {
    source = read(suite.file);
  } catch {
    fail(`${suite.file} is gone; it is part of the shipping gate`);
    continue;
  }
  const found = (source.match(suite.pattern) ?? []).length;
  if (found < suite.least) {
    fail(
      `${suite.file} declares ${found} tests, fewer than the ${suite.least} the gate expects`,
    );
  }
}

/**
 * Claims that must stay honest.
 *
 * The release checklist is where "verified" and "not verified" are kept apart.
 * A checklist that stopped saying what is unverified would be worse than none.
 */
const checklist = (() => {
  try {
    return read("docs/release-checklist.md");
  } catch {
    fail("docs/release-checklist.md is missing");
    return "";
  }
})();

for (const required of [
  "## 出荷前に通るもの",
  "## 未検証",
  "## 停止と切り戻し",
]) {
  if (!checklist.includes(required)) {
    fail(`docs/release-checklist.md no longer has the section ${required}`);
  }
}
for (const job of REQUIRED_JOBS) {
  if (!checklist.includes(job.name)) {
    fail(
      `docs/release-checklist.md does not list the required check "${job.name}"`,
    );
  }
}

// Secrets must not be printed by anything the gate runs, and a workflow that
// starts uploading the database or the environment would be the way that
// happens quietly.
for (const forbidden of [
  "DATABASE_URL }}",
  "secrets.PATHBASE_SESSION_KEYS",
  "env | ",
  "printenv",
]) {
  if (workflow.includes(forbidden)) {
    fail(
      `the workflow exposes ${JSON.stringify(forbidden)} to logs or artifacts`,
    );
  }
}

if (problems.length > 0) {
  console.error("The shipping gate refuses this change:\n");
  for (const problem of problems) console.error(`  - ${problem}`);
  console.error(
    "\nThese are the checks that stand between a green build and a claim that\n" +
      "something works. Changing one is allowed; doing it without noticing is not.",
  );
  process.exit(1);
}

console.log(
  `Shipping gate: ${REQUIRED_JOBS.length} required checks declared, ${SUITES.length} suites intact, release checklist current.`,
);
