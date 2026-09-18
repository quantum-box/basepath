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
    workspaces: [{ id: "personal", name: "個人", scope: "個人" }],
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
        item: { id: "a1", title: "今日の行動" },
        completed: false,
        occurrence_key: "a1:2026-09-18",
      },
    ],
  },
};

window.__calls = [];

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
