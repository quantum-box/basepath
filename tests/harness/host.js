import {
  AppBridge,
  PostMessageTransport,
} from "@modelcontextprotocol/ext-apps/app-bridge";
// The committed bundle, exactly as the Rust server embeds it. Importing the
// same file keeps the harness from testing a different build than production.
import html from "../../api/ui/mcp-app.html?raw";

/**
 * The fixtures the harness answers tool calls with. A test can replace them
 * before the app connects by setting `window.__fixtures`.
 */
const defaults = {
  pathbase_get_context: {
    workspaces: [
      {
        id: "personal",
        name: "個人",
        scope: "個人",
        timezone: "Asia/Tokyo",
        role: "owner",
      },
    ],
    basepath_url: "https://basepath.example",
  },
  pathbase_get_graph: {
    items: [
      {
        id: "g1",
        title: "年間目標",
        kind: "outcome",
        state: "active",
        fields: { self_assessment: 40 },
      },
      {
        id: "a1",
        title: "今日の行動",
        kind: "action",
        state: "active",
        fields: {},
      },
    ],
    relations: [
      { id: "r1", source_id: "a1", target_id: "g1", type: "part_of" },
    ],
    truncated: true,
    limit: 2,
  },
  pathbase_get_today: {
    local_date: "2026-09-18",
    items: [
      {
        item: {
          id: "a1",
          title: "今日の行動",
          version: 3,
          due_date: "2026-09-20",
          fields: { assignee_id: "us_me" },
        },
        completed: false,
        occurrence_key: "a1:2026-09-18",
      },
    ],
  },
  pathbase_get_week: {
    start: "2026-09-14",
    end: "2026-09-20",
    timezone: "Asia/Tokyo",
    days: [
      { date: "2026-09-14", entries: [] },
      {
        date: "2026-09-18",
        entries: [
          {
            item: {
              id: "a1",
              title: "今日の行動",
              scheduled_time: "09:00",
              version: 3,
              fields: {},
            },
            label: "scheduled",
          },
          {
            item: { id: "h1", title: "毎日の習慣", version: 1, fields: {} },
            label: "habit",
            occurrence_key: "h1:2026-09-18",
            status: "missed",
          },
        ],
      },
      {
        date: "2026-09-20",
        entries: [
          {
            item: { id: "a1", title: "今日の行動", version: 3, fields: {} },
            label: "due",
          },
        ],
      },
    ],
    unscheduled: [{ id: "u1", title: "日付未定の項目" }],
  },
  pathbase_complete_action: { id: "change_1", status: "pending" },
  pathbase_get_weekly_review: {
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
        title: "年間目標",
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
        name: "睡眠時間",
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
  },
  pathbase_preview_changes: {
    id: "change_2",
    workspace_id: "personal",
    status: "pending",
    hash: "digest-2",
    changes: [],
  },
  pathbase_list_changes: {
    items: [
      {
        id: "change_1",
        workspace_id: "personal",
        title: "AIからの計画変更",
        status: "pending",
        hash: "digest-1",
        actor: "us_me",
        proposed_by_connection: "mcpconn_1",
        created_at: "2026-09-18T00:00:00Z",
        expires_at: "2099-01-01T00:00:00Z",
        changes: [
          {
            method: "POST",
            path: "/v1/workspaces/personal/items",
            collection: "items",
            id: "new1",
            title: "新しい行動",
            effect: "created",
            before: null,
            after: { title: "新しい行動", state: "active", kind: "action" },
          },
          {
            method: "DELETE",
            path: "/v1/workspaces/personal/relations/r9",
            collection: "relations",
            id: "r9",
            title: "消される関係",
            effect: "deleted",
            before: { title: "消される関係" },
            after: null,
          },
        ],
      },
    ],
    next_cursor: null,
  },
  pathbase_reject_change: { id: "change_1", status: "rejected" },
  pathbase_apply_changes: {
    changeset: { id: "change_1", status: "applied" },
    results: [],
  },
};

window.__calls = [];
/** Full requests, for tests that care about what was sent, not only that it was. */
window.__requests = [];

const iframe = document.getElementById("app");
iframe.srcdoc = html;
await new Promise((resolve) =>
  iframe.addEventListener("load", resolve, { once: true }),
);

const bridge = new AppBridge(
  null,
  { name: "Test host", version: "1.0.0" },
  window.__hostCapabilities ?? { serverTools: {} },
);

bridge.oncalltool = async (request) => {
  window.__calls.push(request.name);
  window.__requests.push({
    name: request.name,
    arguments: request.arguments ?? {},
  });
  const fixtures = window.__fixtures ?? defaults;
  const failure = fixtures[`${request.name}:error`];
  if (failure) {
    return { isError: true, structuredContent: failure, content: [] };
  }
  return {
    isError: false,
    structuredContent: fixtures[request.name],
    content: [
      { type: "text", text: JSON.stringify(fixtures[request.name] ?? null) },
    ],
  };
};

await bridge.connect(
  new PostMessageTransport(iframe.contentWindow, iframe.contentWindow),
);
window.__bridgeReady = true;
