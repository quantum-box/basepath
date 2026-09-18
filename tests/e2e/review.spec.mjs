import { expect, test } from "@playwright/test";

/**
 * The goal review, in the browser.
 *
 * The screen's job is to keep silence and warning apart, and to show a
 * corrected check-in as a correction rather than replacing what was said.
 */
async function shared(request, name) {
  const response = await request.post("/api/v1/workspaces", {
    headers: {
      "idempotency-key": `e2e-rv-${Date.now()}-${Math.random().toString(16).slice(2)}`,
      "content-type": "application/json",
    },
    data: { name, scope: "チーム" },
  });
  expect(response.ok(), await response.text()).toBeTruthy();
  return (await response.json()).id;
}

async function post(request, path, data) {
  const response = await request.post(`/api${path}`, {
    headers: {
      "idempotency-key": `e2e-${Date.now()}-${Math.random().toString(16).slice(2)}`,
      "content-type": "application/json",
    },
    data,
  });
  expect(response.ok(), await response.text()).toBeTruthy();
  return response.json();
}

async function openReview(page, workspaceName) {
  await page.goto("/");
  const menu = page.getByRole("button", {
    name: "メニューを開く",
    exact: true,
  });
  if (await menu.isVisible().catch(() => false)) await menu.click();
  const entry = page
    .getByRole("navigation", { name: "メインメニュー" })
    .getByRole("button", { name: "目標レビュー", exact: true });
  await entry.focus();
  await entry.press("Enter");
  await expect(
    page.getByRole("heading", { name: "目標レビュー", exact: true, level: 1 }),
  ).toBeVisible();
  if (workspaceName) {
    await page
      .getByRole("combobox")
      .first()
      .selectOption({ label: workspaceName });
    await expect(page.locator(".review-intro .eyebrow")).toHaveText(
      workspaceName,
    );
  }
}

test("silence is its own list, and says it is not a warning", async ({
  page,
  request,
}) => {
  const workspaceId = await shared(request, "E2Eレビュー沈黙");
  await post(request, `/v1/workspaces/${workspaceId}/items`, {
    kind: "outcome",
    title: "E2E: 誰も何も言っていない目標",
  });
  const trouble = await post(request, `/v1/workspaces/${workspaceId}/items`, {
    kind: "outcome",
    title: "E2E: 問題があると言われた目標",
  });
  await post(
    request,
    `/v1/workspaces/${workspaceId}/items/${trouble.id}/checkins`,
    { health: "at_risk", blockers: "E2E人手", next_focus: "E2E採用" },
  );

  await openReview(page, "E2Eレビュー沈黙");
  const silent = page
    .locator(".review-list")
    .filter({ hasText: "誰も何も言っていない目標" })
    .first();
  await expect(
    silent.getByText("問題があるとは限りません", { exact: false }),
  ).toBeVisible();
  await expect(silent.getByText("E2E: 誰も何も言っていない目標")).toBeVisible();

  const flagged = page
    .locator(".review-list")
    .filter({ hasText: "問題があると言われた目標" })
    .first();
  await expect(
    flagged.getByText("E2E: 問題があると言われた目標"),
  ).toBeVisible();
  await expect(flagged.getByText("課題: E2E人手")).toBeVisible();
  // The quiet goal is not in the warning list.
  await expect(flagged.getByText("E2E: 誰も何も言っていない目標")).toBeHidden();
});

test("recording a check-in puts it in the history with a name", async ({
  page,
  request,
}) => {
  const workspaceId = await shared(request, "E2E記録");
  await post(request, `/v1/workspaces/${workspaceId}/items`, {
    kind: "outcome",
    title: "E2E: 記録する目標",
  });

  await openReview(page, "E2E記録");
  await page
    .getByRole("button", { name: /E2E: 記録する目標/ })
    .first()
    .click();
  const history = page.locator(".review-history");
  await expect(history).toBeVisible();

  await history.getByLabel("状況").selectOption("at_risk");
  await history.getByLabel("課題").fill("E2E: 見積もりが甘かった");
  await history
    .getByRole("button", { name: "チェックインを記録", exact: true })
    .click();

  await expect(
    history.getByText("課題: E2E: 見積もりが甘かった"),
  ).toBeVisible();
  // The check-in carries the author's name, in the history entry itself.
  await expect(
    history
      .locator(".review-checkins")
      .getByText("local-owner", {
        exact: false,
      })
      .first(),
  ).toBeVisible();
  const items = await request.get(`/api/v1/workspaces/${workspaceId}/items`);
  expect((await items.json()).items[0].fields.health.status).toBe("at_risk");
});

test("a corrected check-in is shown as corrected, not replaced", async ({
  page,
  request,
}) => {
  const workspaceId = await shared(request, "E2E訂正");
  const goal = await post(request, `/v1/workspaces/${workspaceId}/items`, {
    kind: "outcome",
    title: "E2E: 訂正される目標",
  });
  const first = await post(
    request,
    `/v1/workspaces/${workspaceId}/items/${goal.id}/checkins`,
    { health: "on_track", comment: "E2E: 順調です" },
  );
  await post(
    request,
    `/v1/workspaces/${workspaceId}/items/${goal.id}/checkins`,
    {
      health: "off_track",
      comment: "E2E: 見込み違いでした",
      supersedes_id: first.id,
    },
  );

  await openReview(page, "E2E訂正");
  await page
    .getByRole("button", { name: /E2E: 訂正される目標/ })
    .first()
    .click();
  const history = page.locator(".review-history");
  // Both are on screen: the history says what was believed at the time.
  await expect(history.getByText("E2E: 順調です")).toBeVisible();
  await expect(history.getByText("E2E: 見込み違いでした")).toBeVisible();
  await expect(history.getByText("訂正済み")).toBeVisible();
});

test("the timeline shows what happened to the goal, in order", async ({
  page,
  request,
}) => {
  const workspaceId = await shared(request, "E2E履歴");
  const goal = await post(request, `/v1/workspaces/${workspaceId}/items`, {
    kind: "outcome",
    title: "E2E: 履歴のある目標",
  });
  const metric = await post(request, `/v1/workspaces/${workspaceId}/metrics`, {
    item_id: goal.id,
    name: "E2E件数",
    unit: "件",
    direction: "increase",
    baseline: 0,
    target: 10,
  });
  await post(request, `/v1/workspaces/${workspaceId}/observations`, {
    metric_id: metric.id,
    value: 4,
    unit: "件",
    source: "集計",
    observed_at: "2026-09-14T00:00:00Z",
  });
  await post(
    request,
    `/v1/workspaces/${workspaceId}/items/${goal.id}/checkins`,
    { health: "on_track", comment: "E2E: 進んでいます" },
  );

  await openReview(page, "E2E履歴");
  await page
    .getByRole("button", { name: /E2E: 履歴のある目標/ })
    .first()
    .click();
  const timeline = page.locator(".review-timeline");
  await expect(timeline.getByText("作成", { exact: true })).toBeVisible();
  await expect(timeline.getByText("観測", { exact: true })).toBeVisible();
  await expect(
    timeline.getByText("チェックイン", { exact: true }),
  ).toBeVisible();
  await expect(timeline.getByText("E2E件数 4 件")).toBeVisible();
});

test("the review screen is usable at 390px", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openReview(page);
  expect(
    await page
      .locator(".review-screen")
      .evaluate((element) => element.scrollWidth <= element.clientWidth),
  ).toBe(true);
});
