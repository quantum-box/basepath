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
