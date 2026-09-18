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

const { localDateIn, weekBounds, weekFrom, nodeIds, findNode } =
  await import("../src/shared/viewModel.ts");
const { pruneSelection } = await import("../src/shared/useTreeState.ts");

test("today belongs to the workspace's timezone, not the viewer's device", () => {
  // 2026-09-18T15:30Z is already the 19th in Tokyo and still the 18th in UTC.
  const instant = new Date("2026-09-18T15:30:00Z");
  assert.equal(localDateIn("Asia/Tokyo", instant), "2026-09-19");
  assert.equal(localDateIn("UTC", instant), "2026-09-18");
  assert.equal(localDateIn("America/Los_Angeles", instant), "2026-09-18");
  // An unknown timezone must not blank the screen.
  assert.match(localDateIn("Not/AZone", instant), /^\d{4}-\d{2}-\d{2}$/);
});

test("the week runs Monday to Sunday, as the server requires", () => {
  assert.deepEqual(weekBounds("2026-09-18"), {
    start: "2026-09-14",
    end: "2026-09-20",
  });
  // A Monday is its own week start, and a Sunday belongs to the week before.
  assert.deepEqual(weekBounds("2026-09-14"), {
    start: "2026-09-14",
    end: "2026-09-20",
  });
  assert.deepEqual(weekBounds("2026-09-20"), {
    start: "2026-09-14",
    end: "2026-09-20",
  });
  assert.deepEqual(weekBounds("nonsense"), {
    start: "nonsense",
    end: "nonsense",
  });
});

test("the week view keeps habit occurrences and unscheduled work visible", () => {
  const week = weekFrom({
    start: "2026-09-14",
    end: "2026-09-20",
    timezone: "Asia/Tokyo",
    days: [
      {
        date: "2026-09-18",
        entries: [
          {
            item: {
              id: "h1",
              title: "習慣",
              version: 2,
              fields: { assignee_id: "us_a" },
            },
            label: "habit",
            occurrence_key: "h1:2026-09-18",
            status: "skip",
          },
        ],
      },
    ],
    unscheduled: [{ id: "u1", title: "日付未定" }],
  });
  assert.equal(week.days[0].entries[0].status, "skip");
  assert.equal(week.days[0].entries[0].assignee, "us_a");
  assert.equal(week.days[0].entries[0].version, 2);
  assert.equal(week.unscheduled[0].title, "日付未定");
  // A response without days is not a week.
  assert.equal(weekFrom({}), null);
  assert.equal(weekFrom(undefined), null);
});

test("a selection that left the tree is dropped rather than shown", () => {
  const nodes = treeFrom(graph).nodes;
  const ids = nodeIds(nodes);
  assert.equal(pruneSelection("g1", ids), "g1");
  assert.equal(pruneSelection("gone", ids), "");
  assert.equal(pruneSelection("", ids), "");
  assert.equal(findNode(nodes, "a1").title, "行動");
  assert.equal(findNode(nodes, "gone"), null);
});

test("an action carries what a proposal needs, without inventing it", () => {
  const [first] = actionsFrom({
    local_date: "2026-09-18",
    items: [
      {
        item: {
          id: "a1",
          title: "行動",
          version: 7,
          due_date: "2026-09-30",
          fields: { assignee_id: "us_a" },
        },
        completed: false,
        occurrence_key: "a1:2026-09-18",
      },
    ],
  }).actions;
  assert.equal(first.version, 7);
  assert.equal(first.dueDate, "2026-09-30");
  assert.equal(first.assignee, "us_a");
  // A missing version falls back to 1 rather than to undefined, which would
  // be sent as a malformed expected_version.
  const [fallback] = actionsFrom({
    items: [{ item: { id: "a2", title: "版なし" }, occurrence_key: "k" }],
  }).actions;
  assert.equal(fallback.version, 1);
  assert.equal(fallback.assignee, null);
});
