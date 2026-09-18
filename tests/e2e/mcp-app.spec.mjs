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
