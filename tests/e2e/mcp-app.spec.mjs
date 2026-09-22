import { expect, test } from "@playwright/test";

const HARNESS = "/tests/harness/host.html";

async function openHarness(page, options = {}) {
  await page.addInitScript(({ fixtures, capabilities }) => {
    if (fixtures) window.__fixtures = fixtures;
    if (capabilities) window.__hostCapabilities = capabilities;
  }, options);
  await page.goto(HARNESS);
  await page.waitForFunction(() => window.__bridgeReady === true);
  return page.frameLocator("#app");
}

test("renders one compact goal tree from the MCP tool result", async ({
  page,
}) => {
  const app = await openHarness(page);
  await expect(app.locator(".plan-header")).toHaveCount(0);
  await expect(app.getByText("個人の目標")).toBeVisible();
  await expect(app.getByText("個人の行動")).toBeVisible();
  await expect(app.getByRole("heading", { name: "目標ツリー" })).toBeVisible();
  await expect(app.locator('[aria-label="今日の行動"]')).toHaveCount(0);
  await expect(app.locator('[aria-label="今週の行動"]')).toHaveCount(0);
  await expect(app.locator(".plan-workspace-switch")).toHaveCount(0);
  const calls = await page.evaluate(() => window.__calls);
  expect(calls).not.toContain("pathbase_get_today");
  expect(calls).not.toContain("pathbase_get_week");
});

test("switches between the nested list and a React Flow-style map", async ({
  page,
}) => {
  const app = await openHarness(page);
  await expect(app.locator(".plan-tree")).toBeVisible();
  await app.getByRole("button", { name: "マップ表示" }).click();

  await expect(app.locator(".mcp-plan-flow")).toBeVisible();
  await expect(app.locator(".plan-tree")).toHaveCount(0);
  await expect(app.locator(".react-flow__edge")).toHaveCount(1);
  await expect(app.getByRole("button", { name: "Zoom In" })).toBeVisible();
  await expect(app.getByRole("button", { name: "全体表示" })).toBeVisible();
  await expect(app.getByText("個人の目標")).toBeVisible();
  await expect(
    app.getByRole("button", { name: "個人の目標の下位を閉じる" }),
  ).toBeVisible();

  await app.getByRole("button", { name: "個人の目標の下位を閉じる" }).click();
  await expect(app.getByText("個人の行動")).toBeHidden();
  await app.getByRole("button", { name: "全体表示" }).click();
  await expect(app.getByText("個人の行動")).toBeVisible();

  await app.getByRole("button", { name: "リスト表示" }).click();
  await expect(app.locator(".plan-tree")).toBeVisible();
});

test("switches the visible tree when the tool names an organization workspace", async ({
  page,
}) => {
  const app = await openHarness(page);
  await expect(app.locator(".plan-header")).toHaveCount(0);

  await page.evaluate(async () => {
    await window.__pushToolResult(
      {
        items: [
          {
            id: "o1",
            title: "償却前利益3億円",
            kind: "outcome",
            state: "active",
            fields: {},
          },
          {
            id: "o2",
            title: "稼働率を上げる",
            kind: "initiative",
            state: "active",
            fields: {},
          },
        ],
        relations: [{ source_id: "o2", target_id: "o1", type: "part_of" }],
        truncated: false,
        limit: 200,
      },
      { workspace_id: "organization" },
    );
  });

  await expect(
    app.locator('.plan-panel[data-workspace-scope="組織"]'),
  ).toBeVisible();
  await expect(app.getByText("償却前利益3億円")).toBeVisible();
  await expect(app.getByText("個人の目標")).toBeHidden();
});

test("refreshes context when an explicit workspace is not cached", async ({
  page,
}) => {
  const personal = {
    id: "personal",
    name: "個人",
    scope: "個人",
    timezone: "Asia/Tokyo",
    role: "owner",
  };
  const fixtures = {
    pathbase_get_context: { workspaces: [personal] },
    pathbase_get_graph: {
      personal: {
        items: [
          {
            id: "p1",
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
      organization: {
        items: [
          {
            id: "full-o1",
            title: "再取得された全体ツリー",
            kind: "outcome",
            state: "active",
            fields: {},
          },
        ],
        relations: [],
        truncated: false,
        limit: 200,
      },
    },
  };
  const app = await openHarness(page, { fixtures });
  await expect(app.getByText("個人の目標")).toBeVisible();

  await page.evaluate(() => {
    window.__fixtures.pathbase_get_context = {
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
    };
  });
  await page.evaluate(async () => {
    await window.__pushToolResult(
      {
        nodes: [
          {
            id: "focused-o1",
            title: "権限付与後の部分ツリー",
            kind: "outcome",
            state: "active",
            parent_id: null,
          },
          {
            id: "focused-o2",
            title: "権限付与後の部分ツリーの下位",
            kind: "initiative",
            state: "active",
            parent_id: "focused-o1",
          },
        ],
        truncated: false,
        limit: 200,
      },
      { workspace_id: "organization" },
    );
  });

  await expect(
    app.locator('.plan-panel[data-workspace-scope="組織"]'),
  ).toBeVisible();
  await expect(
    app.getByText("権限付与後の部分ツリー", { exact: true }),
  ).toBeVisible();
  await expect(
    app.getByText("権限付与後の部分ツリーの下位"),
  ).toBeVisible();
  await expect(app.getByText("再取得された全体ツリー")).toBeHidden();
  await expect(app.getByText("個人の目標")).toBeHidden();
});

test("folds and opens a branch without leaving the tree surface", async ({
  page,
}) => {
  const app = await openHarness(page);
  const toggle = app.getByRole("button", { name: "個人の目標の下位を閉じる" });
  await toggle.click();
  await expect(app.getByText("下位1件")).toBeVisible();
  await app.getByRole("button", { name: "すべて開く" }).click();
  await expect(app.getByText("個人の行動")).toBeVisible();
});

test("renders a focused breakdown as the same goal tree", async ({ page }) => {
  const app = await openHarness(page);
  await expect(app.locator(".plan-header")).toHaveCount(0);
  await page.evaluate(async () => {
    await window.__pushToolResult(
      {
        nodes: [
          {
            id: "b1",
            title: "組織の目標",
            kind: "outcome",
            state: "active",
            parent_id: null,
          },
          {
            id: "b2",
            title: "組織の取り組み",
            kind: "initiative",
            state: "active",
            parent_id: "b1",
          },
        ],
        truncated: false,
        limit: 200,
      },
      { workspace_id: "organization" },
    );
  });
  await expect(app.getByText("組織の目標")).toBeVisible();
  await expect(app.getByText("組織の取り組み")).toBeVisible();
});

test("explains an unavailable host instead of rendering a blank plan", async ({
  page,
}) => {
  const app = await openHarness(page, { capabilities: {} });
  await expect(app.getByRole("alert")).toContainText(
    "このホストでは表示できません",
  );
});

test("stays readable in a narrow conversation pane", async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 720 });
  const app = await openHarness(page);
  await expect(app.getByRole("heading", { name: "目標ツリー" })).toBeVisible();
  const overflow = await page
    .frameLocator("#app")
    .locator("body")
    .evaluate((body) => body.scrollWidth - body.clientWidth);
  expect(overflow).toBeLessThanOrEqual(1);
});
