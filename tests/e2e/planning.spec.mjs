import { expect, test } from "@playwright/test";
import { open } from "./navigate.mjs";

/**
 * Planning periods, in the browser.
 *
 * The screen has to make three things obvious: which period this is, that a
 * workspace without periods is fine, and that carrying work forward copies it
 * rather than moving it.
 */
async function ownWorkspace(request, name) {
  const response = await request.post("/api/v1/workspaces", {
    headers: {
      "idempotency-key": `e2e-cyc-${Date.now()}-${Math.random().toString(16).slice(2)}`,
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

async function openPlanning(page, workspaceName) {
  // The workspace is the context now, so it is crossed to before the screen is
  // opened rather than chosen from a dropdown inside it.
  await open(page, { workspace: workspaceName, screen: "計画期間" });
}

test("a workspace with no periods says so and offers no chore", async ({
  page,
}) => {
  await openPlanning(page);
  await expect(
    page.getByText("このワークスペースは計画期間を使っていません", {
      exact: false,
    }),
  ).toBeVisible();
  // Nothing is presented as an unfinished setup step.
  await expect(page.getByText("設定が完了していません")).toBeHidden();
  await expect(
    page.getByRole("button", { name: "期間を追加", exact: true }),
  ).toBeVisible();
});

test("a quarter gets its own length and name from the cadence", async ({
  page,
}) => {
  await openPlanning(page);
  await page.getByRole("button", { name: "期間を追加", exact: true }).click();
  await page.getByLabel("開始日").fill("2027-04-01");
  await page.getByRole("button", { name: "作成", exact: true }).click();

  // 2027 Q2, ending on the last day of June, without anyone typing either.
  await expect(page.getByText("2027 Q2").first()).toBeVisible();
  await expect(page.getByText("2027-04-01〜2027-06-30").first()).toBeVisible();
  await expect(page.getByText("期間を作成しました。")).toBeVisible();
});

test("carrying work forward copies it and leaves the original in place", async ({
  page,
  request,
}) => {
  const workspaceId = await ownWorkspace(request, "E2E期間");
  const first = await post(request, `/v1/workspaces/${workspaceId}/cycles`, {
    cadence: "quarter",
    start_date: "2026-07-01",
  });
  const second = await post(request, `/v1/workspaces/${workspaceId}/cycles`, {
    cadence: "quarter",
    start_date: "2026-10-01",
    previous_id: first.id,
  });
  await post(request, `/v1/workspaces/${workspaceId}/items`, {
    kind: "outcome",
    title: "E2E: 終わらなかった目標",
    fields: { cycle_id: first.id },
  });

  await openPlanning(page, "E2E期間");
  // Look at the later period, then carry the unfinished goal into it.
  await page
    .getByRole("button", { name: /2026 Q4/ })
    .first()
    .click();
  const carry = page.locator(".planning-carry");
  await expect(carry).toBeVisible();
  await carry.getByText("E2E: 終わらなかった目標").click();
  await page.getByRole("button", { name: /件を引き継ぐ/ }).click();

  await expect(
    page.getByText("元の項目はそのまま残ります", { exact: false }),
  ).toBeVisible();

  // Two items now exist: the original in Q3 and the copy in Q4, and the copy
  // points back at where it came from.
  const items = await request.get(`/api/v1/workspaces/${workspaceId}/items`);
  const listed = (await items.json()).items;
  expect(listed.length).toBe(2);
  const original = listed.find((item) => item.fields.cycle_id === first.id);
  const copy = listed.find((item) => item.fields.cycle_id === second.id);
  expect(original.version).toBe(1);
  expect(copy.fields.carried_from).toBe(original.id);
  expect(copy.id).not.toBe(original.id);
});

test("the bar shows elapsed time, and calls it elapsed time", async ({
  page,
  request,
}) => {
  const workspaceId = await ownWorkspace(request, "E2E経過");
  const today = new Date();
  const iso = (offset) => {
    const date = new Date(today);
    date.setUTCDate(date.getUTCDate() + offset);
    return date.toISOString().slice(0, 10);
  };
  await post(request, `/v1/workspaces/${workspaceId}/cycles`, {
    cadence: "custom",
    label: "E2E進行中",
    start_date: iso(-10),
    end_date: iso(10),
  });

  await openPlanning(page, "E2E経過");
  await expect(page.getByText("E2E進行中").first()).toBeVisible();
  // "Elapsed", never "progress": the two are different claims.
  await expect(page.getByText(/期間の経過 \d+%/)).toBeVisible();
  await expect(page.getByText(/進捗 \d+%/)).toBeHidden();
});

test("the planning screen is usable at 390px", async ({ page, request }) => {
  const workspace = `E2E狭幅期間${Date.now()}`;
  await ownWorkspace(request, workspace);
  await page.setViewportSize({ width: 390, height: 844 });
  await openPlanning(page, workspace);
  await expect(
    page.getByRole("heading", { name: "計画期間", exact: true, level: 1 }),
  ).toBeVisible();
  expect(
    await page
      .locator(".planning-screen")
      .evaluate((element) => element.scrollWidth <= element.clientWidth),
  ).toBe(true);
});
