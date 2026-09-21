import {
  AppBridge,
  PostMessageTransport,
} from "@modelcontextprotocol/ext-apps/app-bridge";
import html from "../../api/ui/mcp-app.html?raw";

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
      {
        id: "organization",
        name: "ゴルフ場運営",
        scope: "組織",
        timezone: "Asia/Tokyo",
        role: "editor",
      },
    ],
  },
  pathbase_get_graph: {
    personal: {
      items: [
        { id: "p1", title: "個人の目標", kind: "outcome", state: "active", fields: {} },
        { id: "p2", title: "個人の行動", kind: "action", state: "active", fields: {} },
      ],
      relations: [{ source_id: "p2", target_id: "p1", type: "part_of" }],
      truncated: false,
      limit: 200,
    },
    organization: {
      items: [
        { id: "o1", title: "償却前利益3億円", kind: "outcome", state: "active", fields: { self_assessment: 62 } },
        { id: "o2", title: "稼働率を上げる", kind: "initiative", state: "active", fields: {} },
        { id: "o3", title: "平日の予約枠を増やす", kind: "action", state: "active", fields: {} },
      ],
      relations: [
        { source_id: "o2", target_id: "o1", type: "part_of" },
        { source_id: "o3", target_id: "o2", type: "part_of" },
      ],
      truncated: false,
      limit: 200,
    },
  },
  pathbase_get_today: { local_date: "2026-09-22", items: [] },
};

window.__calls = [];
window.__requests = [];

const iframe = document.getElementById("app");
iframe.srcdoc = html;
await new Promise((resolve) =>
  iframe.addEventListener("load", resolve, { once: true }),
);

const bridge = new AppBridge(
  null,
  { name: "Basepath test host", version: "1.0.0" },
  window.__hostCapabilities ?? { serverTools: {} },
);

function fixtureFor(name, args) {
  const fixtures = window.__fixtures ?? defaults;
  if (name === "pathbase_get_graph") {
    const byWorkspace = fixtures.pathbase_get_graph;
    if (byWorkspace?.items || byWorkspace?.nodes) return byWorkspace;
    return byWorkspace?.[args?.workspace_id] ?? byWorkspace?.personal ?? {};
  }
  return fixtures[name] ?? {};
}

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
  const value = fixtureFor(request.name, request.arguments ?? {});
  return {
    isError: false,
    structuredContent: value,
    content: [{ type: "text", text: JSON.stringify(value) }],
  };
};

await bridge.connect(
  new PostMessageTransport(iframe.contentWindow, iframe.contentWindow),
);

window.__pushToolResult = async (structured, arguments_ = {}) => {
  await bridge.sendToolInput({ arguments: arguments_ });
  await bridge.sendToolResult({
    isError: false,
    structuredContent: structured,
    content: [{ type: "text", text: JSON.stringify(structured) }],
  });
};
window.__bridgeReady = true;
