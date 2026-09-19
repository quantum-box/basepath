import { expect, test } from "@playwright/test";
import { open } from "./navigate.mjs";

/**
 * The dashboard, in the browser.
 *
 * The screen's job is to keep four different facts apart and to show absence
 * as absence. These tests mostly assert what is *not* on screen: no 0% where
 * nothing is known, no derived progress where no method was chosen, and no
 * health that nobody wrote.
 */
async function shared(request, name) {
  const response = await request.post("/api/v1/workspaces", {
    headers: {
      "idempotency-key": `e2e-db-${Date.now()}-${Math.random().toString(16).slice(2)}`,
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

async function openDashboard(page, workspaceName) {
  // The workspace is the context now, so it is crossed to before the screen is
  // opened rather than chosen from a dropdown inside it.
  await open(page, { workspace: workspaceName, screen: "ダッシュボード" });
  if (workspaceName) {
    await expect(page.locator(".dashboard-intro .eyebrow")).toHaveText(
      workspaceName,
    );
  }
}

test("a goal with nothing measured shows absence, not zero", async ({
  page,
  request,
}) => {
  const workspaceId = await shared(request, "E2E空欄");
  await post(request, `/v1/workspaces/${workspaceId}/items`, {
    kind: "outcome",
    title: "E2E: 何も分かっていない目標",
  });

  await openDashboard(page, "E2E空欄");
  const goal = page.locator(".dashboard-goal");
  await expect(goal.getByText("E2E: 何も分かっていない目標")).toBeVisible();
  // Three em dashes and one "未設定": no completion rate, no derived
  // progress, no self-assessment, no health.
  await expect(goal.getByText("—")).toHaveCount(2);
  await expect(goal.getByText("未設定")).toBeVisible();
  await expect(goal.getByText("未記入")).toBeVisible();
  await expect(goal.getByText("集計方法なし")).toBeVisible();
  // Nothing anywhere claims 0%.
  await expect(goal.getByText("0%")).toBeHidden();
});

test("the four figures are labelled as four different things", async ({
  page,
  request,
}) => {
  const workspaceId = await shared(request, "E2E四つ");
  const goal = await post(request, `/v1/workspaces/${workspaceId}/items`, {
    kind: "outcome",
    title: "E2E: 四つ並ぶ目標",
    fields: { rollup: "metric_average", self_assessment: 30 },
  });
  const metric = await post(request, `/v1/workspaces/${workspaceId}/metrics`, {
    item_id: goal.id,
    name: "E2E売上",
    unit: "件",
    direction: "increase",
    baseline: 0,
    target: 100,
  });
  await post(request, `/v1/workspaces/${workspaceId}/observations`, {
    metric_id: metric.id,
    value: 80,
    unit: "件",
    source: "手入力",
    observed_at: new Date().toISOString(),
  });

  await openDashboard(page, "E2E四つ");
  const row = page.locator(".dashboard-goal");
  await expect(row.getByText("行動の実施")).toBeVisible();
  await expect(row.getByText("指標の進捗")).toBeVisible();
  await expect(row.getByText("自己評価")).toBeVisible();
  await expect(row.getByText("状況", { exact: true })).toBeVisible();
  // Different numbers, because they measure different things.
  await expect(row.getByText("80%")).toBeVisible();
  await expect(row.getByText("30%")).toBeVisible();
  await expect(row.getByText("指標の平均")).toBeVisible();
});

test("recording health puts a name and a date on the judgement", async ({
  page,
  request,
}) => {
  const workspaceId = await shared(request, "E2E状況");
  await post(request, `/v1/workspaces/${workspaceId}/items`, {
    kind: "outcome",
    title: "E2E: 状況を記録する目標",
  });

  await openDashboard(page, "E2E状況");
  await expect(
    page.getByText("誰も記入していません", { exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "注意として記録" }).click();

  await expect(page.getByText("local-owner が記入")).toBeVisible();
  const items = await request.get(`/api/v1/workspaces/${workspaceId}/items`);
  const stored = (await items.json()).items[0];
  expect(stored.fields.health.status).toBe("at_risk");
  expect(stored.fields.health.set_by).toBe("local-owner");
  expect(stored.fields.health.set_at).toBeTruthy();
});

test("signals are shown as signals, not as a verdict", async ({
  page,
  request,
}) => {
  const workspaceId = await shared(request, "E2E兆候");
  const goal = await post(request, `/v1/workspaces/${workspaceId}/items`, {
    kind: "outcome",
    title: "E2E: 兆候のある目標",
    due_date: "2020-01-01",
    fields: { rollup: "metric_average" },
  });
  await post(request, `/v1/workspaces/${workspaceId}/metrics`, {
    item_id: goal.id,
    name: "E2E未計測",
    unit: "件",
    direction: "increase",
    baseline: 0,
    target: 10,
  });

  await openDashboard(page, "E2E兆候");
  await expect(page.getByText("気になる点", { exact: false })).toBeVisible();
  await expect(
    page.getByText("期限を過ぎています", { exact: false }),
  ).toBeVisible();
  // The signals do not decide the health.
  await expect(page.getByText("状況はまだ誰も記入していません")).toBeVisible();
  await expect(
    page
      .locator(".dashboard-figure")
      .filter({ hasText: "状況" })
      .getByText("未記入"),
  ).toBeVisible();
});

test("a number can be opened to the observation behind it", async ({
  page,
  request,
}) => {
  const workspaceId = await shared(request, "E2E根拠");
  const goal = await post(request, `/v1/workspaces/${workspaceId}/items`, {
    kind: "outcome",
    title: "E2E: 根拠のある目標",
    fields: { rollup: "metric_average" },
  });
  const metric = await post(request, `/v1/workspaces/${workspaceId}/metrics`, {
    item_id: goal.id,
    name: "E2E解約率",
    unit: "%",
    direction: "decrease",
    baseline: 10,
    target: 5,
  });
  await post(request, `/v1/workspaces/${workspaceId}/observations`, {
    metric_id: metric.id,
    value: 7.5,
    unit: "%",
    source: "集計",
    observed_at: "2026-09-14T00:00:00Z",
  });

  await openDashboard(page, "E2E根拠");
  await page.getByRole("button", { name: /根拠の指標/ }).click();
  const table = page.locator(".dashboard-metrics");
  await expect(table.getByText("E2E解約率")).toBeVisible();
  await expect(table.getByText("7.5 %")).toBeVisible();
  await expect(table.getByText("10 → 5")).toBeVisible();
  // Halfway from 10 down to 5.
  await expect(table.getByText("50%")).toBeVisible();
  await expect(table.getByText("2026-09-14")).toBeVisible();
});

test("the dashboard is usable at 390px", async ({ page, request }) => {
  // The screen only exists in an organization, so the test needs one: a
  // personal Basepath does not offer it at any width.
  const workspace = `E2E狭幅指標${Date.now()}`;
  await shared(request, workspace);
  await page.setViewportSize({ width: 390, height: 844 });
  await openDashboard(page, workspace);
  expect(
    await page
      .locator(".dashboard-screen")
      .evaluate((element) => element.scrollWidth <= element.clientWidth),
  ).toBe(true);
});
