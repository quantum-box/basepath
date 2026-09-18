// The view model is what every surface renders, so it is tested on its own,
// without a browser, a host, or a database.
import assert from "node:assert/strict";
import test from "node:test";
import { register } from "node:module";

// Vite-specific imports (CSS, .tsx) are not loadable by node:test directly, so
// the pure modules are compiled on the fly.
register("./ts-loader.mjs", import.meta.url);

const { treeFrom, actionsFrom, buildPlanView, countNodes } =
  await import("../src/shared/viewModel.ts");
const { FixtureHost, HostError, loadPlanView, structuredResult, McpAppHost } =
  await import("../src/shared/host.ts");

const graph = {
  items: [
    {
      id: "g1",
      title: "年間目標",
      kind: "outcome",
      state: "active",
      due_date: "2026-12-31",
      fields: { self_assessment: 40 },
    },
    {
      id: "i1",
      title: "取り組み",
      kind: "initiative",
      state: "active",
      fields: {},
    },
    { id: "a1", title: "行動", kind: "action", state: "done", fields: {} },
    {
      id: "orphan",
      title: "親が範囲外",
      kind: "initiative",
      state: "active",
      fields: {},
    },
  ],
  relations: [
    { id: "r1", source_id: "i1", target_id: "g1", type: "part_of" },
    { id: "r2", source_id: "a1", target_id: "i1", type: "part_of" },
    { id: "r3", source_id: "orphan", target_id: "missing", type: "part_of" },
    { id: "r4", source_id: "a1", target_id: "g1", type: "contributes_to" },
  ],
  truncated: true,
  limit: 4,
};

const today = {
  local_date: "2026-09-18",
  items: [
    {
      item: { id: "a1", title: "行動", scheduled_time: "09:00" },
      completed: true,
      occurrence_key: "a1:2026-09-18",
    },
    {
      item: { id: "a2", title: "もうひとつ" },
      completed: false,
      occurrence_key: "a2:2026-09-18",
    },
  ],
};

test("the goal tree nests part_of and keeps orphans visible", () => {
  const tree = treeFrom(graph);
  assert.equal(tree.truncated, true);
  assert.equal(tree.limit, 4);
  // Only `part_of` nests. `contributes_to` is a different relation and must not
  // move a node in the tree.
  const titles = tree.nodes.map((node) => node.title).sort();
  assert.deepEqual(titles, ["年間目標", "親が範囲外"]);
  const goal = tree.nodes.find((node) => node.id === "g1");
  assert.equal(goal.children.length, 1);
  assert.equal(goal.children[0].children[0].title, "行動");
  assert.equal(goal.selfAssessment, 40);
  assert.equal(goal.dueDate, "2026-12-31");
  assert.equal(countNodes(tree.nodes), 4);
});

test("a self-referencing relation cannot build an infinite tree", () => {
  const tree = treeFrom({
    items: [{ id: "x", title: "自己参照", kind: "outcome", state: "active" }],
    relations: [{ source_id: "x", target_id: "x", type: "part_of" }],
  });
  assert.equal(tree.nodes.length, 1);
  assert.equal(tree.nodes[0].children.length, 0);
});

test("actions carry their occurrence and completion", () => {
  const actions = actionsFrom(today);
  assert.equal(actions.localDate, "2026-09-18");
  assert.deepEqual(
    actions.actions.map((action) => [action.title, action.completed]),
    [
      ["行動", true],
      ["もうひとつ", false],
    ],
  );
  assert.equal(actions.actions[0].scheduledTime, "09:00");
});

test("missing or malformed data renders as empty rather than throwing", () => {
  const view = buildPlanView({});
  assert.deepEqual(view.nodes, []);
  assert.equal(view.workspace, null);
  assert.deepEqual(treeFrom(null).nodes, []);
  assert.deepEqual(treeFrom({ items: "nonsense" }).nodes, []);
  assert.deepEqual(actionsFrom(undefined).actions, []);
});

test("a tool error becomes a typed failure instead of being rendered as data", () => {
  assert.throws(
    () =>
      structuredResult({
        isError: true,
        structuredContent: {
          code: "INSUFFICIENT_SCOPE",
          message: "権限がありません",
          status: 403,
        },
      }),
    (error) =>
      error instanceof HostError &&
      error.code === "INSUFFICIENT_SCOPE" &&
      error.status === 403,
  );
  // A host that only returns text is still usable.
  assert.deepEqual(
    structuredResult({ content: [{ type: "text", text: '{"ok":true}' }] }),
    { ok: true },
  );
});

test("the fixture host and a host adapter produce the same view", async () => {
  const context = {
    workspaces: [{ id: "personal", name: "個人", scope: "個人" }],
  };
  const fixture = new FixtureHost({
    pathbase_get_context: context,
    pathbase_get_graph: graph,
    pathbase_get_today: today,
  });
  const fromFixture = await loadPlanView(fixture);
  assert.equal(fromFixture.error, null);
  assert.equal(fromFixture.view.workspace.name, "個人");
  assert.equal(fromFixture.view.actions.length, 2);
  assert.equal(fromFixture.view.truncated, true);

  // The MCP host wraps the same responses in tool-result envelopes. The view
  // must come out identical.
  const mcp = new McpAppHost(
    async ({ name }) => ({
      isError: false,
      structuredContent: {
        pathbase_get_context: context,
        pathbase_get_graph: graph,
        pathbase_get_today: today,
      }[name],
    }),
    { serverTools: true },
  );
  const fromMcp = await loadPlanView(mcp);
  assert.deepEqual(fromMcp.view, fromFixture.view);
});

test("a host without tool support fails with a reason the UI can explain", async () => {
  const host = new McpAppHost(async () => ({}), { serverTools: false });
  const { error } = await loadPlanView(host);
  assert.equal(error.code, "HOST_UNSUPPORTED");
});
