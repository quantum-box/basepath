import { expect, test } from "@playwright/test";

/**
 * The MCP App, driven through a real MCP Apps host.
 *
 * `tests/harness/host.html` runs the published AppBridge against the committed
 * bundle in a sandboxed iframe, so this exercises the actual postMessage
 * protocol: initialization, capability negotiation, tool calls and errors.
 */
const HARNESS = "/tests/harness/host.html";

async function openHarness(page, { fixtures, capabilities } = {}) {
  await page.addInitScript(
    ({ fixtures, capabilities }) => {
      if (fixtures) window.__fixtures = fixtures;
      if (capabilities) window.__hostCapabilities = capabilities;
    },
    { fixtures, capabilities },
  );
  await page.goto(HARNESS);
  await page.waitForFunction(() => window.__bridgeReady === true);
  return page.frameLocator("#app");
}

test("renders the plan from tool results inside the host", async ({ page }) => {
  const app = await openHarness(page);

  await expect(app.getByRole("heading", { name: "個人" })).toBeVisible();
  await expect(app.getByText("年間目標")).toBeVisible();
  await expect(app.getByText("今日の行動").first()).toBeVisible();
  // A truncated graph says so rather than looking complete.
  await expect(
    app.getByText("これは計画の一部です", { exact: false }),
  ).toBeVisible();
  await expect(app.getByText("2026-09-18の行動")).toBeVisible();

  // The app asked the host for exactly the data it needed, through the host.
  const calls = await page.evaluate(() => window.__calls);
  expect(calls).toContain("pathbase_get_context");
  expect(calls).toContain("pathbase_get_graph");
  expect(calls).toContain("pathbase_get_today");
});

test("explains a refusal instead of rendering an empty plan", async ({
  page,
}) => {
  const app = await openHarness(page, {
    fixtures: {
      "pathbase_get_context:error": {
        code: "INSUFFICIENT_SCOPE",
        message: "この接続には pathbase.read の権限がありません",
        status: 403,
      },
    },
  });

  await expect(app.getByRole("alert")).toContainText("権限が足りません");
  await expect(
    app.getByText("目標と行動を読む", { exact: false }),
  ).toBeVisible();
  await expect(app.getByRole("button", { name: "再試行" })).toBeVisible();
});

test("a host without tool support still says why nothing is shown", async ({
  page,
}) => {
  const app = await openHarness(page, { capabilities: {} });
  await expect(app.getByRole("alert")).toContainText(
    "このホストでは表示できません",
  );
});

test("an empty workspace reads as empty, not as a failure", async ({
  page,
}) => {
  const app = await openHarness(page, {
    fixtures: {
      pathbase_get_context: {
        workspaces: [{ id: "personal", name: "個人", scope: "個人" }],
      },
      pathbase_get_graph: {
        items: [],
        relations: [],
        truncated: false,
        limit: 200,
      },
      pathbase_get_today: { local_date: "2026-09-18", items: [] },
    },
  });
  await expect(app.getByText("まだ目標がありません")).toBeVisible();
  await expect(
    app.getByText("この日に予定された行動はありません"),
  ).toBeVisible();
});

test("the app is readable in a narrow conversation pane", async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 720 });
  const app = await openHarness(page);
  await expect(app.getByRole("heading", { name: "個人" })).toBeVisible();
  const overflow = await page
    .frameLocator("#app")
    .locator("body")
    .evaluate((body) => body.scrollWidth - body.clientWidth);
  expect(overflow).toBeLessThanOrEqual(1);
});

test("the bundle loads nothing from another origin", async ({
  page,
  baseURL,
}) => {
  // The resource declares an empty CSP, so anything it tried to fetch from
  // elsewhere would simply be blocked by the host. Catching it here means the
  // bundle never grows a dependency on a CDN in the first place.
  const local = new URL(baseURL).host;
  const external = [];
  page.on("request", (request) => {
    const url = new URL(request.url());
    if (
      url.protocol !== "data:" &&
      url.protocol !== "blob:" &&
      url.host !== local
    ) {
      external.push(request.url());
    }
  });
  const app = await openHarness(page);
  await expect(app.getByRole("heading", { name: "個人" })).toBeVisible();
  expect(external).toEqual([]);
});

test("shows the goal map, folding, selection detail, today and the week", async ({
  page,
}) => {
  const app = await openHarness(page);

  // The week the workspace is in, with its timezone stated.
  await expect(
    app.getByText("今週（2026-09-14 〜 2026-09-20 / Asia/Tokyo）"),
  ).toBeVisible();
  await expect(app.getByText("毎日の習慣")).toBeVisible();
  await expect(app.getByText("未実施")).toBeVisible();
  await expect(app.getByText("日付のない項目が1件あります")).toBeVisible();
  // The action carries who it is for and when it is due.
  await expect(app.getByText("担当 us_me")).toBeVisible();
  await expect(app.getByText("期限 2026-09-20")).toBeVisible();

  // Folding a branch hides its children and says how many are hidden.
  const toggle = app.getByRole("button", { name: "年間目標の下位を閉じる" });
  await expect(app.getByText("今日の行動").first()).toBeVisible();
  await toggle.click();
  await expect(app.getByText("下位1件")).toBeVisible();
  await expect(app.getByRole("button", { name: "すべて開く" })).toBeVisible();
  await app.getByRole("button", { name: "すべて開く" }).click();
  await expect(app.getByText("下位1件")).toBeHidden();

  // Selecting a node opens its detail; selecting again closes it.
  const node = app.getByRole("button", { name: "年間目標の詳細" });
  await node.click();
  const detail = app.getByRole("region", { name: "選択中の項目" });
  await expect(detail).toBeVisible();
  await expect(detail.getByText("自己評価")).toBeVisible();
  await node.click();
  await expect(detail).toBeHidden();
});

test("completing an action is reported as a proposal, never as done", async ({
  page,
}) => {
  const app = await openHarness(page);
  await app.getByRole("button", { name: "完了を提案" }).click();

  // The wording must not claim the plan changed.
  await expect(
    app.getByText("変更案を作成しました", { exact: false }),
  ).toBeVisible();
  // The checkbox stays unchecked: the server still says it is not done.
  await expect(app.locator("[data-done='false']").first()).toBeVisible();

  // The view is reloaded from the server rather than guessed at.
  const calls = await page.evaluate(() => window.__calls);
  expect(
    calls.filter((name) => name === "pathbase_complete_action"),
  ).toHaveLength(1);
  expect(calls.lastIndexOf("pathbase_get_today")).toBeGreaterThan(
    calls.indexOf("pathbase_complete_action"),
  );
});

test("a refused proposal says why instead of looking like it worked", async ({
  page,
}) => {
  const app = await openHarness(page, {
    fixtures: {
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
      },
      pathbase_get_graph: {
        items: [],
        relations: [],
        truncated: false,
        limit: 200,
      },
      pathbase_get_today: {
        local_date: "2026-09-18",
        items: [
          {
            item: { id: "a1", title: "衝突する行動", version: 3, fields: {} },
            completed: false,
            occurrence_key: "a1:2026-09-18",
          },
        ],
      },
      pathbase_get_week: {
        start: "2026-09-14",
        end: "2026-09-20",
        timezone: "Asia/Tokyo",
        days: [],
        unscheduled: [],
      },
      "pathbase_complete_action:error": {
        code: "VERSION_CONFLICT",
        message: "別の操作で更新されています",
        status: 409,
      },
    },
  });

  await app.getByRole("button", { name: "完了を提案" }).click();
  await expect(
    app.getByText("別の操作で更新されています", { exact: false }),
  ).toBeVisible();
  // Nothing may look like it succeeded.
  await expect(
    app.getByText("変更案を作成しました", { exact: false }),
  ).toBeHidden();
  await expect(app.locator("[data-done='false']").first()).toBeVisible();
});

test("switching workspace does not carry folding or selection across", async ({
  page,
}) => {
  const app = await openHarness(page, {
    fixtures: {
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
            id: "team",
            name: "チーム",
            scope: "チーム",
            timezone: "UTC",
            role: "viewer",
          },
        ],
      },
      pathbase_get_graph: {
        items: [
          {
            id: "g1",
            title: "個人の目標",
            kind: "outcome",
            state: "active",
            fields: {},
          },
        ],
        relations: [],
        truncated: false,
        limit: 200,
      },
      pathbase_get_today: { local_date: "2026-09-18", items: [] },
      pathbase_get_week: {
        start: "2026-09-14",
        end: "2026-09-20",
        timezone: "Asia/Tokyo",
        days: [],
        unscheduled: [],
      },
    },
  });

  await expect(app.getByRole("heading", { name: "個人" })).toBeVisible();
  await app.getByRole("button", { name: "個人の目標の詳細" }).click();
  await expect(app.getByRole("region", { name: "選択中の項目" })).toBeVisible();

  await app.getByLabel("ワークスペース").selectOption("team");
  await expect(app.getByRole("heading", { name: "チーム" })).toBeVisible();
  // The previous workspace's selection must not survive the switch.
  await expect(app.getByRole("region", { name: "選択中の項目" })).toBeHidden();
  // The viewer's role is shown, so it is clear what they may do.
  await expect(app.getByText("viewer")).toBeVisible();
});

test("a deep tree and long Japanese titles stay readable at 320px", async ({
  page,
}) => {
  const deep = [];
  const relations = [];
  const long =
    "とても長い日本語のタイトルで折り返しの挙動を確認するための項目名です";
  for (let depth = 0; depth < 8; depth += 1) {
    deep.push({
      id: `n${depth}`,
      title: `${long}${depth}`,
      kind: depth === 0 ? "outcome" : "initiative",
      state: "active",
      fields: {},
    });
    if (depth > 0) {
      relations.push({
        id: `r${depth}`,
        source_id: `n${depth}`,
        target_id: `n${depth - 1}`,
        type: "part_of",
      });
    }
  }
  await page.setViewportSize({ width: 320, height: 800 });
  const app = await openHarness(page, {
    fixtures: {
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
      },
      pathbase_get_graph: {
        items: deep,
        relations,
        truncated: false,
        limit: 200,
      },
      pathbase_get_today: { local_date: "2026-09-18", items: [] },
      pathbase_get_week: {
        start: "2026-09-14",
        end: "2026-09-20",
        timezone: "Asia/Tokyo",
        days: [],
        unscheduled: [],
      },
    },
  });

  await expect(app.getByText(`${long}7`)).toBeVisible();
  const overflow = await page
    .frameLocator("#app")
    .locator("body")
    .evaluate((body) => body.scrollWidth - body.clientWidth);
  expect(overflow).toBeLessThanOrEqual(1);
});
