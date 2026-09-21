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
const { FixtureHost, HostError, loadPlanView, structuredResult, McpToolHost } =
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

test("a focused breakdown result is rendered as the same goal tree", () => {
  const tree = treeFrom({
    nodes: [
      {
        id: "root",
        title: "組織の目標",
        kind: "outcome",
        state: "active",
        parent_id: null,
      },
      {
        id: "child",
        title: "組織の取り組み",
        kind: "initiative",
        state: "active",
        parent_id: "root",
      },
    ],
    truncated: false,
    limit: 200,
  });
  assert.equal(tree.nodes[0].title, "組織の目標");
  assert.equal(tree.nodes[0].children[0].title, "組織の取り組み");
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

test("an explicit workspace wins over the personal fallback", () => {
  const view = buildPlanView({
    context: {
      workspaces: [
        { id: "organization", name: "組織", scope: "組織" },
        { id: "personal", name: "個人", scope: "個人" },
      ],
    },
    workspaceId: "organization",
  });
  assert.equal(view.workspaces[0].id, "personal");
  assert.equal(view.workspace.id, "organization");
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
  const mcp = new McpToolHost(
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
  const host = new McpToolHost(async () => ({}), { serverTools: false });
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

// --- The weekly review -------------------------------------------------
//
// The server already aggregated the week. These tests are about the line the
// view model must not cross: it may reshape and label, never recompute, and
// never turn an absence into a number.

const {
  weeklyReviewFrom,
  isWeeklyReview,
  mondayOf,
  shiftWeek,
  draftInputFrom,
  draftBody,
  draftDiffers,
  draftKey,
  hasDraftText,
} = await import("../src/shared/weeklyView.ts");

const week = {
  workspace_id: "personal",
  timezone: "Asia/Tokyo",
  week_start: "2026-09-14",
  week_end: "2026-09-20",
  actions: {
    total: 4,
    completed: 3,
    skipped: 1,
    incomplete: 0,
    items: [
      {
        item_id: "a1",
        title: "朝の散歩",
        date: "2026-09-15",
        status: "completed",
        record_id: "rec_1",
        actor: "us_me",
      },
      {
        item_id: "a1",
        title: "朝の散歩",
        date: "2026-09-16",
        status: "skipped",
        record_id: null,
        actor: "us_me",
      },
    ],
  },
  goals: [
    {
      item_id: "g1",
      title: "体力をつける",
      self_assessment: 40,
      assessed_at: "2026-09-14T00:00:00Z",
    },
    {
      item_id: "g2",
      title: "未評価の目標",
      self_assessment: null,
      assessed_at: null,
    },
  ],
  metrics: [
    {
      metric_id: "m1",
      item_id: "g1",
      name: "走行距離",
      unit: "km",
      latest: 12,
      previous: 9,
      delta: 3,
      status: "current",
      latest_observation_id: "obs_1",
    },
    {
      metric_id: "m2",
      item_id: "g1",
      name: "体重",
      unit: "kg",
      latest: null,
      previous: null,
      delta: null,
      status: "unmeasured",
      latest_observation_id: null,
    },
    {
      metric_id: "m3",
      item_id: "g1",
      name: "睡眠",
      unit: "h",
      latest: 6,
      previous: null,
      delta: null,
      status: "stale",
      latest_observation_id: "obs_9",
    },
  ],
  members: [],
  review: null,
  history: [],
};

test("the weekly view reports the server's own counts, not its own", () => {
  const view = weeklyReviewFrom(week);
  assert.equal(view.totals.completed, 3);
  assert.equal(view.totals.skipped, 1);
  assert.equal(view.totals.incomplete, 0);
  // Four occurrences were planned even though only two rows are listed: the
  // totals are the server's and are never re-derived from the visible rows.
  assert.equal(view.totals.total, 4);
  assert.equal(view.actions.length, 2);
  assert.equal(view.completionRate, 75);
  assert.equal(view.empty, false);
});

test("an unmeasured value stays unmeasured instead of becoming zero", () => {
  const view = weeklyReviewFrom(week);
  const [current, unmeasured, stale] = view.metrics;
  assert.equal(current.latest, 12);
  assert.equal(current.delta, 3);
  assert.equal(unmeasured.latest, null);
  assert.equal(unmeasured.previous, null);
  assert.equal(unmeasured.delta, null);
  assert.equal(unmeasured.status, "unmeasured");
  // A measurement with nothing to compare against has no delta, not a zero one.
  assert.equal(stale.latest, 6);
  assert.equal(stale.delta, null);
  assert.equal(stale.status, "stale");
  // A goal nobody assessed is unassessed, never 0%.
  assert.equal(view.goals[1].selfAssessment, null);
});

test("a week with nothing in it has no completion rate at all", () => {
  const view = weeklyReviewFrom({
    ...week,
    actions: { total: 0, completed: 0, skipped: 0, incomplete: 0, items: [] },
    goals: [],
    metrics: [],
  });
  assert.equal(view.totals.total, 0);
  // Not 0%: there was nothing to complete, which is a different statement.
  assert.equal(view.completionRate, null);
  assert.equal(view.empty, true);
});

test("completing actions is kept apart from achieving a goal", () => {
  const view = weeklyReviewFrom(week);
  // Every action done, the goal still assessed at 40%. Nothing combines them.
  assert.equal(view.completionRate, 75);
  assert.equal(view.goals[0].selfAssessment, 40);
  assert.ok(!("progress" in view));
});

test("a malformed or unrelated payload is not rendered as a week", () => {
  assert.equal(weeklyReviewFrom(null), null);
  assert.equal(weeklyReviewFrom({ items: [] }), null);
  assert.equal(isWeeklyReview({ week_start: "2026-09-14" }), false);
  assert.equal(isWeeklyReview(week), true);
  // A week whose lists are missing renders empty rather than throwing.
  const view = weeklyReviewFrom({ week_start: "2026-09-14", actions: {} });
  assert.equal(view.totals.total, 0);
  assert.deepEqual(view.actions, []);
  assert.equal(view.review, null);
});

test("the review shown is the newest revision, with the history kept", () => {
  const view = weeklyReviewFrom({
    ...week,
    review: {
      id: "wr2",
      status: "draft",
      revision: 2,
      version: 1,
      learnings: "第2版",
      challenges: "",
      next_focus: "",
      author: "us_me",
      updated_at: "2026-09-21T00:00:00Z",
      finalized_at: null,
    },
    history: [
      {
        id: "wr1",
        status: "finalized",
        revision: 1,
        version: 2,
        learnings: "第1版",
        challenges: "",
        next_focus: "",
        author: "us_me",
        updated_at: "2026-09-20T00:00:00Z",
        finalized_at: "2026-09-20T00:00:00Z",
      },
      {
        id: "wr2",
        status: "draft",
        revision: 2,
        version: 1,
        learnings: "第2版",
        challenges: "",
        next_focus: "",
        author: "us_me",
        updated_at: "2026-09-21T00:00:00Z",
        finalized_at: null,
      },
    ],
  });
  assert.equal(view.review.learnings, "第2版");
  assert.equal(view.history.length, 2);
  // The correction never rewrites what the earlier revision said.
  assert.equal(view.history[0].learnings, "第1版");
  assert.equal(view.history[0].status, "finalized");
});

test("the week boundary is Monday to Sunday, whatever date is given", () => {
  assert.equal(mondayOf("2026-09-17"), "2026-09-14");
  assert.equal(mondayOf("2026-09-14"), "2026-09-14");
  assert.equal(mondayOf("2026-09-20"), "2026-09-14");
  assert.equal(shiftWeek("2026-09-14", -1), "2026-09-07");
  assert.equal(shiftWeek("2026-09-14", 1), "2026-09-21");
  // A month boundary is not a special case.
  assert.equal(shiftWeek("2026-10-05", -1), "2026-09-28");
});

test("a save carries a version only when it could overwrite an edit", () => {
  const draft = { learnings: "学び", challenges: "", next_focus: "" };
  const input = { learnings: "学び", challenges: "", nextFocus: "" };
  // Nothing saved yet: there is no version to conflict with.
  assert.equal(
    "expected_version" in draftBody("2026-09-14", input, null),
    false,
  );
  const saved = {
    id: "wr1",
    status: "draft",
    revision: 1,
    version: 4,
    learnings: "前の下書き",
    challenges: "",
    nextFocus: "",
    author: "us_me",
    updatedAt: "",
    finalizedAt: null,
  };
  const body = draftBody("2026-09-14", input, saved);
  assert.equal(body.expected_version, 4);
  assert.equal(body.week_start, "2026-09-14");
  assert.equal(body.next_focus, "");
  // A finalized revision is never written over: a correction starts a new one.
  assert.equal(
    "expected_version" in
      draftBody("2026-09-14", input, { ...saved, status: "finalized" }),
    false,
  );
  assert.equal(draft.learnings, "学び");
});

test("unsent text is recognised so a newer result cannot silently drop it", () => {
  const saved = {
    id: "wr1",
    status: "draft",
    revision: 1,
    version: 1,
    learnings: "保存済み",
    challenges: "",
    nextFocus: "",
    author: "us_me",
    updatedAt: "",
    finalizedAt: null,
  };
  assert.equal(draftDiffers(draftInputFrom(saved), saved), false);
  assert.equal(
    draftDiffers(
      { learnings: "書きかけ", challenges: "", nextFocus: "" },
      saved,
    ),
    true,
  );
  assert.equal(
    hasDraftText({ learnings: "  ", challenges: "", nextFocus: "" }),
    false,
  );
  assert.equal(
    hasDraftText({ learnings: "", challenges: "課題", nextFocus: "" }),
    true,
  );
  // Edited text is a different proposal and must not collide with the earlier
  // one, while resending the same text must.
  const a = { learnings: "同じ", challenges: "", nextFocus: "" };
  const b = { learnings: "同じ", challenges: "", nextFocus: "" };
  const c = { learnings: "ちがう", challenges: "", nextFocus: "" };
  assert.equal(draftKey(a), draftKey(b));
  assert.notEqual(draftKey(a), draftKey(c));
  // The key is safe in an HTTP header, which rejects non-ASCII.
  assert.match(draftKey(c), /^[0-9a-f]+$/);
});

// --- Planning periods ---------------------------------------------------
//
// A period frames work; it does not own it. The tests below are mostly about
// what must not be claimed: that a workspace without periods is misconfigured,
// or that elapsed time is progress.

const {
  planningViewFrom,
  elapsedPercent,
  dayAfter,
  nextCycleBody,
  spanLabel,
  cadenceLabel,
  statusLabel,
} = await import("../src/shared/planningView.ts");

const quarter = {
  id: "cycle_q4",
  cadence: "quarter",
  label: "2026 Q4",
  start_date: "2026-10-01",
  end_date: "2026-12-31",
  status: "active",
  previous_id: "cycle_q3",
  version: 3,
};

const planning = {
  workspace_id: "personal",
  timezone: "Asia/Tokyo",
  today: "2026-11-15",
  cycles: [
    {
      id: "cycle_q3",
      cadence: "quarter",
      label: "2026 Q3",
      start_date: "2026-07-01",
      end_date: "2026-09-30",
      status: "closed",
      previous_id: null,
      version: 5,
    },
    quarter,
  ],
  current: { cycle: quarter, item_count: 7 },
  previous: {
    cycle: {
      id: "cycle_q3",
      cadence: "quarter",
      label: "2026 Q3",
      start_date: "2026-07-01",
      end_date: "2026-09-30",
      status: "closed",
      previous_id: null,
      version: 5,
    },
    item_count: 4,
  },
  next: null,
  unassigned_items: 2,
  last_finalized_review: {
    week_start: "2026-11-09",
    next_focus: "朝に寄せる",
    learnings: "観測: 3件完了",
    challenges: "",
  },
};

test("the planning view reads the period the server chose", () => {
  const view = planningViewFrom(planning);
  assert.equal(view.current.id, "cycle_q4");
  assert.equal(view.current.itemCount, 7);
  assert.equal(view.current.version, 3, "a write needs the version");
  assert.equal(view.previous.label, "2026 Q3");
  assert.equal(view.previous.status, "closed");
  assert.equal(view.next, null);
  assert.equal(view.unassignedItems, 2);
  assert.equal(view.unused, false);
  // The review is the person's words, carried through unchanged.
  assert.equal(view.lastReview.nextFocus, "朝に寄せる");
});

test("a workspace with no periods is a normal workspace", () => {
  const view = planningViewFrom({
    workspace_id: "personal",
    timezone: "Asia/Tokyo",
    today: "2026-11-15",
    cycles: [],
    current: null,
    previous: null,
    next: null,
    unassigned_items: 12,
    last_finalized_review: null,
  });
  // Not an error, not a setup step: just no periods.
  assert.equal(view.unused, true);
  assert.equal(view.current, null);
  assert.equal(view.unassignedItems, 12);
  assert.equal(view.lastReview, null);
  // And a payload that is not a planning context is not rendered as one.
  assert.equal(planningViewFrom(null), null);
  assert.equal(planningViewFrom({ cycles: [] }), null);
});

test("elapsed time is elapsed time, and only inside the period", () => {
  const view = planningViewFrom(planning);
  // 2026-10-01..2026-12-31 is 92 days; 2026-11-15 is day 46.
  assert.equal(elapsedPercent(view.current, "2026-11-15"), 50);
  assert.equal(elapsedPercent(view.current, "2026-10-01"), 1);
  assert.equal(elapsedPercent(view.current, "2026-12-31"), 100);
  // Outside the period there is no such number, so none is offered.
  assert.equal(elapsedPercent(view.current, "2026-09-30"), null);
  assert.equal(elapsedPercent(view.current, "2027-01-01"), null);
  assert.equal(elapsedPercent(view.current, "not-a-date"), null);
});

test("the next period follows from this one, and nothing else does", () => {
  const view = planningViewFrom(planning);
  assert.equal(dayAfter("2026-12-31"), "2027-01-01");
  assert.equal(dayAfter("2026-02-28"), "2026-03-01");
  const body = nextCycleBody(view.current);
  assert.deepEqual(body, {
    cadence: "quarter",
    start_date: "2027-01-01",
    previous_id: "cycle_q4",
  });
  // No label and no end date: the cadence decides those, and inventing them
  // here would let the client and the server disagree about what a quarter is.
  assert.equal("label" in body, false);
  assert.equal("end_date" in body, false);
});

test("a period says what it is in words a person uses", () => {
  const view = planningViewFrom(planning);
  assert.equal(spanLabel(view.current), "2026-10-01〜2026-12-31");
  assert.equal(cadenceLabel("quarter"), "四半期");
  assert.equal(cadenceLabel("custom"), "任意期間");
  assert.equal(statusLabel("closed"), "終了");
  assert.equal(statusLabel("planned"), "予定");
});

test("an unknown cadence or status degrades instead of breaking the screen", () => {
  const view = planningViewFrom({
    ...planning,
    cycles: [{ ...quarter, cadence: "fortnight", status: "paused" }],
    current: {
      cycle: { ...quarter, cadence: "fortnight", status: "paused" },
      item_count: 1,
    },
  });
  assert.equal(view.current.cadence, "custom");
  assert.equal(view.current.status, "planned");
  assert.equal(view.current.label, "2026 Q4");
});

// --- Goal alignment -----------------------------------------------------
//
// One workspace, one graph. `partOf` and `contributesTo` stay different
// questions all the way to the screen.

const {
  alignmentViewFrom,
  ownerLabel,
  ownerKindLabel,
  roots,
  supporters,
  chainUpward,
} = await import("../src/shared/alignmentView.ts");

const alignment = {
  workspace_id: "team",
  goals: [
    {
      id: "g_company",
      title: "償却前利益を伸ばす",
      kind: "outcome",
      state: "active",
      owner: { kind: "organization", id: "" },
      cycle: { id: "cycle_q4", label: "2026 Q4" },
      self_assessment: 30,
      due_date: "2026-12-31",
      part_of: [],
      contributes_to: [],
      supported_by: ["g_team"],
      descendant_work: 0,
      orphan: true,
    },
    {
      id: "g_team",
      title: "運営コストを下げる",
      kind: "outcome",
      state: "active",
      owner: { kind: "team", id: "運営" },
      cycle: null,
      self_assessment: null,
      due_date: null,
      part_of: ["g_company"],
      contributes_to: ["g_other"],
      supported_by: ["g_person"],
      descendant_work: 4,
      orphan: false,
    },
    {
      id: "g_person",
      title: "発注の手戻りを減らす",
      kind: "outcome",
      state: "active",
      owner: { kind: "person", id: "us_alice" },
      cycle: null,
      self_assessment: 60,
      due_date: null,
      part_of: ["g_team"],
      contributes_to: [],
      supported_by: [],
      descendant_work: 2,
      orphan: false,
    },
    {
      id: "g_other",
      title: "別の会社目標",
      kind: "outcome",
      state: "active",
      owner: { kind: "organization", id: "" },
      cycle: null,
      self_assessment: null,
      due_date: null,
      part_of: [],
      contributes_to: [],
      supported_by: ["g_team"],
      descendant_work: 0,
      orphan: true,
    },
  ],
  teams: ["運営"],
  people: ["us_alice"],
  unowned_goals: 0,
  orphan_goals: 2,
};

test("structure and contribution stay separate all the way to the view", () => {
  const view = alignmentViewFrom(alignment);
  const team = view.goals.find((goal) => goal.id === "g_team");
  // One structural parent, one contribution: different fields, different
  // questions. Merging them would answer neither.
  assert.deepEqual(team.partOf, ["g_company"]);
  assert.deepEqual(team.contributesTo, ["g_other"]);
  assert.deepEqual(team.supportedBy, ["g_person"]);
  assert.equal(team.descendantWork, 4);
});

test("reading starts from the goals with nothing above them", () => {
  const view = alignmentViewFrom(alignment);
  assert.deepEqual(
    roots(view).map((goal) => goal.id),
    ["g_company", "g_other"],
  );
  // Which is normal at the top: two company goals, neither part of anything.
  assert.equal(view.orphanGoals, 2);
  assert.deepEqual(
    supporters(view, view.goals[0]).map((goal) => goal.id),
    ["g_team"],
  );
});

test("the chain upward follows structure only", () => {
  const view = alignmentViewFrom(alignment);
  const mine = view.goals.find((goal) => goal.id === "g_person");
  assert.deepEqual(
    chainUpward(view, mine).map((goal) => goal.id),
    ["g_team", "g_company"],
  );
  // Following contribution too would produce several chains and answer a
  // different question.
  assert.equal(chainUpward(view, view.goals[0]).length, 0);
});

test("an owner is named the way a person would name it", () => {
  const view = alignmentViewFrom(alignment);
  assert.equal(ownerLabel(view.goals[0].owner), "組織");
  assert.equal(ownerLabel(view.goals[1].owner), "運営");
  assert.equal(ownerLabel(view.goals[2].owner), "us_alice");
  assert.equal(ownerLabel(null), "担当なし");
  assert.equal(ownerKindLabel("organization"), "組織の目標");
  assert.equal(ownerKindLabel("person"), "個人の目標");
});

test("an unassessed goal is unassessed, not zero", () => {
  const view = alignmentViewFrom(alignment);
  assert.equal(view.goals[1].selfAssessment, null);
  assert.equal(view.goals[2].selfAssessment, 60);
  assert.equal(view.goals[0].cycleLabel, "2026 Q4");
  assert.equal(view.goals[1].cycleLabel, null);
});

test("a malformed owner or payload does not become a goal nobody owns", () => {
  assert.equal(alignmentViewFrom(null), null);
  assert.equal(alignmentViewFrom({ goals: [] }), null);
  const view = alignmentViewFrom({
    ...alignment,
    goals: [{ ...alignment.goals[0], owner: { kind: "department", id: "x" } }],
  });
  // An owner kind this model does not have is no owner at all, rather than
  // being quietly rendered as one.
  assert.equal(view.goals[0].owner, null);
  assert.equal(ownerLabel(view.goals[0].owner), "担当なし");
});

test("a goal supported through two paths is not rendered twice", () => {
  // The graph refuses cycles, but a diamond is legal: two goals can both be
  // supported by the same one.
  const view = alignmentViewFrom({
    ...alignment,
    goals: alignment.goals.map((goal) =>
      goal.id === "g_other" ? { ...goal, supported_by: ["g_team"] } : goal,
    ),
  });
  const reachable = new Set();
  const walk = (goal) => {
    if (reachable.has(goal.id)) return;
    reachable.add(goal.id);
    for (const child of supporters(view, goal)) walk(child);
  };
  for (const root of roots(view)) walk(root);
  assert.equal(reachable.size, 4, "every goal is reachable exactly once");
});

// --- Goal dashboard -----------------------------------------------------
//
// Four different facts, four named fields, and absent is never zero.

const {
  dashboardViewFrom,
  healthLabel,
  methodLabel,
  percent,
  metricValueLabel,
} = await import("../src/shared/dashboardView.ts");

const board = {
  workspace_id: "team",
  today: "2026-11-15",
  goals: [
    {
      id: "g1",
      title: "四つが並ぶ目標",
      owner: { kind: "organization", id: "" },
      cycle: { id: "c1", label: "2026 Q4" },
      state: "active",
      due_date: "2026-12-31",
      action_completion: { total: 4, completed: 1, rate: 25 },
      metric_progress: {
        method: "metric_worst",
        source: "metrics",
        value: 20,
        counted: 2,
        missing: 1,
        metrics: [
          {
            metric_id: "m1",
            name: "売上",
            unit: "万円",
            direction: "increase",
            baseline: 0,
            target: 100,
            latest: 90,
            progress: 90,
            status: "current",
            latest_observation_id: "obs_1",
            observed_at: "2026-11-10T00:00:00Z",
          },
          {
            metric_id: "m2",
            name: "解約率",
            unit: "%",
            direction: "decrease",
            baseline: 10,
            target: 5,
            latest: 9,
            progress: 20,
            status: "stale",
            latest_observation_id: "obs_2",
            observed_at: "2026-09-01T00:00:00Z",
          },
          {
            metric_id: "m3",
            name: "測っていない指標",
            unit: "件",
            direction: "increase",
            baseline: 0,
            target: 10,
            latest: null,
            progress: null,
            status: "unmeasured",
            latest_observation_id: null,
            observed_at: null,
          },
        ],
      },
      self_assessment: 60,
      assessed_at: "2026-11-01T00:00:00Z",
      health: {
        status: "at_risk",
        note: "人手が足りない",
        set_at: "2026-11-12T00:00:00Z",
        set_by: "us_alice",
      },
      suggested_health: {
        status: "at_risk",
        reasons: ["2週間以上更新されていない指標があります"],
      },
      signals: {
        overdue: false,
        stale_metrics: 1,
        unmeasured_metrics: 1,
        last_checkin_at: "2026-11-12T00:00:00Z",
        days_since_checkin: 3,
      },
    },
    {
      id: "g2",
      title: "何も分かっていない目標",
      owner: null,
      cycle: null,
      state: "active",
      due_date: null,
      action_completion: { total: 0, completed: 0, rate: null },
      metric_progress: {
        method: null,
        source: "none",
        value: null,
        counted: 0,
        missing: 0,
        metrics: [],
      },
      self_assessment: null,
      assessed_at: null,
      health: null,
      suggested_health: null,
      signals: {
        overdue: false,
        stale_metrics: 0,
        unmeasured_metrics: 0,
        last_checkin_at: null,
        days_since_checkin: null,
      },
    },
  ],
  by_health: { on_track: 0, at_risk: 1, off_track: 0, unknown: 1 },
  without_rollup_method: 1,
  rollup_methods: [
    "metric_average",
    "metric_worst",
    "children_average",
    "children_worst",
  ],
};

test("the four figures stay four figures, with four different values", () => {
  const view = dashboardViewFrom(board);
  const goal = view.goals[0];
  assert.equal(goal.actionCompletion.rate, 25);
  assert.equal(goal.metricProgress.value, 20);
  assert.equal(goal.selfAssessment, 60);
  assert.equal(goal.health.status, "at_risk");
  // There is deliberately no single "progress" field to reach for.
  assert.equal("progress" in goal, false);
});

test("nothing known reads as nothing known, never as zero", () => {
  const view = dashboardViewFrom(board);
  const blank = view.goals[1];
  assert.equal(blank.actionCompletion.rate, null);
  assert.equal(blank.metricProgress.value, null);
  assert.equal(blank.metricProgress.method, null);
  assert.equal(blank.selfAssessment, null);
  assert.equal(blank.health, null);
  // And each renders as an absence rather than a number.
  assert.equal(percent(blank.actionCompletion.rate), "—");
  assert.equal(percent(blank.selfAssessment, "未設定"), "未設定");
  assert.equal(healthLabel(null), "未記入");
  assert.equal(methodLabel(null), "集計方法なし");
  // A real zero still reads as zero: this is about absence, not about
  // hiding bad numbers.
  assert.equal(percent(0), "0%");
});

test("a derived number carries the method that derived it", () => {
  const view = dashboardViewFrom(board);
  const goal = view.goals[0];
  assert.equal(goal.metricProgress.method, "metric_worst");
  assert.equal(methodLabel("metric_worst"), "指標の最小");
  assert.equal(goal.metricProgress.counted, 2);
  // And says what it could not count rather than averaging what it had.
  assert.equal(goal.metricProgress.missing, 1);
});

test("every metric can be followed back to its observation", () => {
  const view = dashboardViewFrom(board);
  const [first, , unmeasured] = view.goals[0].metricProgress.metrics;
  assert.equal(first.latestObservationId, "obs_1");
  assert.equal(first.observedAt, "2026-11-10T00:00:00Z");
  assert.equal(metricValueLabel(first), "90 万円");
  // An unmeasured metric says so and points at nothing, rather than at 0.
  assert.equal(unmeasured.latest, null);
  assert.equal(unmeasured.progress, null);
  assert.equal(unmeasured.latestObservationId, null);
  assert.equal(metricValueLabel(unmeasured), "未計測");
});

test("a suggestion stays a suggestion", () => {
  const view = dashboardViewFrom(board);
  const goal = view.goals[0];
  // Both exist, separately: the suggestion does not replace the judgement,
  // and the judgement does not silence the signals.
  assert.equal(goal.suggestedHealth.status, "at_risk");
  assert.deepEqual(goal.suggestedHealth.reasons, [
    "2週間以上更新されていない指標があります",
  ]);
  assert.equal(goal.health.setBy, "us_alice");
  // A goal nobody has judged has no health, whatever the signals say.
  assert.equal(view.goals[1].health, null);
  assert.equal(view.byHealth.unknown, 1);
});

test("a malformed health or payload does not become a verdict", () => {
  assert.equal(dashboardViewFrom(null), null);
  assert.equal(dashboardViewFrom({ goals: [] }), null);
  const view = dashboardViewFrom({
    ...board,
    goals: [{ ...board.goals[0], health: { status: "絶好調", set_by: "x" } }],
  });
  // A status this model does not have is no status at all, rather than being
  // rendered as though someone had said it.
  assert.equal(view.goals[0].health, null);
  assert.equal(healthLabel(view.goals[0].health?.status ?? null), "未記入");
});

// --- Check-ins, timeline, review ----------------------------------------
//
// A history is only worth having if it says what was believed at the time.

const {
  checkinsFrom,
  timelineFrom,
  reviewQueueFrom,
  eventLabel,
  healthLabel: checkinHealthLabel,
  reviewCounts,
} = await import("../src/shared/checkinView.ts");

test("a correction is shown as a correction, not as the only truth", () => {
  const history = checkinsFrom({
    standing_id: "ci2",
    items: [
      {
        id: "ci2",
        item_id: "g1",
        health: "off_track",
        comment: "見込み違いでした",
        author: "us_alice",
        created_at: "2026-11-10T00:00:00Z",
        supersedes_id: "ci1",
        observation_ids: [],
      },
      {
        id: "ci1",
        item_id: "g1",
        health: "on_track",
        comment: "順調です",
        self_assessment: 70,
        author: "us_alice",
        created_at: "2026-11-01T00:00:00Z",
        observation_ids: [],
      },
    ],
  });
  assert.equal(history.length, 2);
  const [current, original] = history;
  assert.equal(current.standing, true);
  assert.equal(current.superseded, false);
  assert.equal(current.supersedesId, "ci1");
  // The one it replaced is still here, still saying what it said.
  assert.equal(original.standing, false);
  assert.equal(original.superseded, true);
  assert.equal(original.comment, "順調です");
  assert.equal(original.selfAssessment, 70);
});

test("a timeline replayed to a moment reports that moment", () => {
  const line = timelineFrom({
    item_id: "g1",
    title: "履歴のある目標",
    as_of: "2026-11-05T00:00:00Z",
    events: [
      { at: "2026-10-01T00:00:00Z", kind: "created", summary: "作成" },
      {
        at: "2026-11-01T00:00:00Z",
        kind: "checkin",
        actor: "us_alice",
        summary: "on_track",
        ref: "ci1",
      },
    ],
    state: {
      health: "on_track",
      self_assessment: 70,
      checkin_id: "ci1",
      checked_in_at: "2026-11-01T00:00:00Z",
      checked_in_by: "us_alice",
    },
  });
  assert.equal(line.asOf, "2026-11-05T00:00:00Z");
  assert.equal(line.state.health, "on_track");
  assert.equal(line.state.checkedInBy, "us_alice");
  assert.equal(line.events.length, 2);
  assert.equal(eventLabel("checkin"), "チェックイン");
  assert.equal(eventLabel("alignment_changed"), "つながりの変更");
  // An event kind nobody has named still renders as itself rather than blank.
  assert.equal(eventLabel("something_new"), "something_new");
});

test("before anyone had spoken, nothing was known", () => {
  const line = timelineFrom({
    item_id: "g1",
    title: "まだ何もない目標",
    as_of: "2000-01-01T00:00:00Z",
    events: [],
    state: {
      health: null,
      self_assessment: null,
      checkin_id: null,
      checked_in_at: null,
      checked_in_by: null,
    },
  });
  assert.equal(line.state.health, null);
  assert.equal(line.state.selfAssessment, null);
  assert.equal(checkinHealthLabel(line.state.health), "未記入");
  assert.deepEqual(line.events, []);
  assert.equal(timelineFrom(null), null);
  assert.equal(timelineFrom({ events: [] }), null);
});

test("silence and a warning are different lists", () => {
  const queue = reviewQueueFrom({
    workspace_id: "team",
    stale_days: 14,
    never_checked_in: [
      { id: "g_quiet", title: "誰も何も言っていない", owner: null },
    ],
    stale: [],
    at_risk: [
      {
        id: "g_trouble",
        title: "問題がある",
        owner: { kind: "team", id: "運営" },
        health: "at_risk",
        blockers: "人手",
        next_focus: "採用",
        last_checkin_at: "2026-11-10T00:00:00Z",
        last_checkin_by: "us_alice",
      },
    ],
    recently_updated: [
      {
        id: "g_trouble",
        title: "問題がある",
        owner: { kind: "team", id: "運営" },
      },
    ],
  });
  const counts = reviewCounts(queue);
  assert.equal(counts.silent, 1);
  assert.equal(counts.atRisk, 1);
  // The quiet goal is not in the warning list, and never becomes one by
  // being quiet for longer.
  assert.equal(
    queue.atRisk.some((entry) => entry.id === "g_quiet"),
    false,
  );
  assert.equal(queue.neverCheckedIn[0].lastCheckinAt, null);
  assert.equal(queue.neverCheckedIn[0].ownerLabel, "担当なし");
  assert.equal(queue.atRisk[0].ownerLabel, "運営");
  assert.equal(queue.atRisk[0].blockers, "人手");
});

test("a health value this model does not have is not rendered as one", () => {
  const history = checkinsFrom({
    standing_id: "ci1",
    items: [
      {
        id: "ci1",
        item_id: "g1",
        health: "絶好調",
        author: "us_alice",
        created_at: "2026-11-01T00:00:00Z",
      },
    ],
  });
  assert.equal(history[0].health, null);
  assert.equal(checkinHealthLabel(history[0].health), "未記入");
  // And the observation links default to an empty list rather than undefined,
  // so a screen never has to guard for it.
  assert.deepEqual(history[0].observationIds, []);
});

// --- Personal memory ----------------------------------------------------
//
// Confirmed and suggested are different. No longer current is not wrong.

const {
  memoryListFrom,
  duplicateGroupsFrom,
  inactiveReason,
  current: currentMemories,
  countsByKind,
  kindLabel,
  statusLabel: memoryStatusLabel,
} = await import("../src/shared/memoryView.ts");

const memories = {
  superseded_ids: ["m_old"],
  items: [
    {
      id: "m_new",
      kind: "preference",
      title: "午前に集中したい",
      status: "verified",
      source: "本人",
      created_at: "2026-11-01T00:00:00Z",
      version: 1,
      evidence_ids: [],
      item_ids: [],
      topics: [],
      people: [],
      author: "us_alice",
    },
    {
      id: "m_old",
      kind: "preference",
      title: "夜に集中したい",
      status: "verified",
      source: "本人",
      created_at: "2026-01-01T00:00:00Z",
      version: 1,
      author: "us_alice",
    },
    {
      id: "m_guess",
      kind: "context",
      title: "移動の多い週は進みが遅いようだ",
      status: "proposed",
      confidence: 0.6,
      source: "観測: 完了率",
      created_at: "2026-11-05T00:00:00Z",
      version: 1,
      author: "us_alice",
    },
    {
      id: "m_expired",
      kind: "preference",
      title: "前職では夜型だった",
      status: "verified",
      source: "本人",
      valid_to: "2024-12-31T00:00:00Z",
      created_at: "2024-01-01T00:00:00Z",
      version: 1,
      author: "us_alice",
    },
  ],
};

test("a suggestion never renders as something the person said", () => {
  const list = memoryListFrom(memories);
  const guess = list.memories.find((memory) => memory.id === "m_guess");
  assert.equal(guess.status, "proposed");
  assert.equal(guess.confidence, 0.6);
  assert.equal(memoryStatusLabel("proposed"), "AIの候補");
  assert.equal(memoryStatusLabel("verified"), "本人が確認");
  // Something the person confirmed carries no machine estimate of itself.
  const said = list.memories.find((memory) => memory.id === "m_new");
  assert.equal(said.confidence, null);
});

test("no longer current is never called wrong", () => {
  const list = memoryListFrom(memories);
  const now = new Date("2026-11-15T00:00:00Z");
  assert.equal(inactiveReason(list.memories[0], now), null);
  // Superseded and expired are different reasons, and neither is "wrong".
  assert.equal(
    inactiveReason(
      list.memories.find((memory) => memory.id === "m_old"),
      now,
    ),
    "更新済み",
  );
  assert.equal(
    inactiveReason(
      list.memories.find((memory) => memory.id === "m_expired"),
      now,
    ),
    "この期間は過ぎました",
  );
  for (const memory of list.memories) {
    const reason = inactiveReason(memory, now);
    if (reason) assert.ok(!reason.includes("誤"), reason);
  }
});

test("what stands now is a subset, and the rest is still there", () => {
  const list = memoryListFrom(memories);
  const now = new Date("2026-11-15T00:00:00Z");
  const standing = currentMemories(list, now).map((memory) => memory.id);
  assert.deepEqual(standing.sort(), ["m_guess", "m_new"]);
  // Nothing was dropped from the list itself.
  assert.equal(list.memories.length, 4);
});

test("kinds are counted and named the way a person would", () => {
  const list = memoryListFrom(memories);
  const counts = countsByKind(list);
  assert.equal(counts.preference, 3);
  assert.equal(counts.context, 1);
  assert.equal(counts.fact, 0);
  assert.equal(kindLabel("decision"), "決定");
  assert.equal(kindLabel("episode"), "出来事");
});

test("an unknown kind does not become a fact", () => {
  const list = memoryListFrom({
    items: [{ id: "m1", kind: "hunch", title: "x", status: "verified" }],
  });
  // The one direction this must never guess in is toward "the person said so".
  assert.equal(list.memories[0].kind, "context");
  const guessed = memoryListFrom({
    items: [{ id: "m2", kind: "fact", title: "x", status: "何か" }],
  });
  assert.equal(guessed.memories[0].status, "proposed");
  assert.equal(memoryListFrom(null), null);
});

test("duplicates are reported as a question, not an answer", () => {
  const groups = duplicateGroupsFrom({
    merged: false,
    groups: [
      {
        id: "m1",
        title: "毎週金曜に振り返り",
        kind: "preference",
        similar: [{ id: "m2", title: "毎週金曜に振り返りをする", overlap: 79 }],
      },
    ],
  });
  assert.equal(groups.length, 1);
  assert.equal(groups[0].similar[0].overlap, 79);
  // Nothing in the shape suggests one of them has been chosen.
  assert.equal("winner" in groups[0], false);
  assert.deepEqual(duplicateGroupsFrom({}), []);
});
