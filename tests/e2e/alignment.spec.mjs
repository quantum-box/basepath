import { expect, test } from "@playwright/test";

/**
 * The alignment map, in the browser.
 *
 * The map has to make three things visible: whose goal each one is, that a
 * personal workspace is not part of it, and that structure and contribution
 * are different relationships.
 */
async function shared(request, name) {
  const response = await request.post("/api/v1/workspaces", {
    headers: {
      "idempotency-key": `e2e-al-${Date.now()}-${Math.random().toString(16).slice(2)}`,
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

async function openAlignment(page, workspaceName) {
  await page.goto("/");
  const menu = page.getByRole("button", {
    name: "メニューを開く",
    exact: true,
  });
  if (await menu.isVisible().catch(() => false)) await menu.click();
  const entry = page
    .getByRole("navigation", { name: "メインメニュー" })
    .getByRole("button", { name: "アラインメント", exact: true });
  await entry.focus();
  await entry.press("Enter");
  await expect(
    page.getByRole("heading", {
      name: "アラインメント",
      exact: true,
      level: 1,
    }),
  ).toBeVisible();
  if (workspaceName) {
    await page
      .getByRole("combobox")
      .first()
      .selectOption({ label: workspaceName });
    await expect(page.locator(".alignment-intro .eyebrow")).toHaveText(
      workspaceName,
    );
  }
}

/** A company goal, a team goal under it, and a person's goal under that. */
async function threeLevels(request, name) {
  const workspaceId = await shared(request, name);
  const company = await post(request, `/v1/workspaces/${workspaceId}/items`, {
    kind: "outcome",
    title: "E2E: 会社の目標",
    fields: { owner: { kind: "organization" } },
  });
  const team = await post(request, `/v1/workspaces/${workspaceId}/items`, {
    kind: "outcome",
    title: "E2E: チームの目標",
    fields: { owner: { kind: "team", id: "運営" } },
  });
  const mine = await post(request, `/v1/workspaces/${workspaceId}/items`, {
    kind: "outcome",
    title: "E2E: わたしの目標",
    fields: { owner: { kind: "person", id: "local-owner" } },
  });
  await post(request, `/v1/workspaces/${workspaceId}/relations`, {
    source_id: team.id,
    target_id: company.id,
    type: "part_of",
  });
  await post(request, `/v1/workspaces/${workspaceId}/relations`, {
    source_id: mine.id,
    target_id: team.id,
    type: "part_of",
  });
  return { workspaceId, company, team, mine };
}

test("the map reads downward from the company goal", async ({
  page,
  request,
}) => {
  const { workspaceId } = await threeLevels(request, "E2Eアライン");
  void workspaceId;
  await openAlignment(page, "E2Eアライン");

  const map = page.locator(".alignment-map");
  await expect(map.getByText("E2E: 会社の目標")).toBeVisible();
  await expect(map.getByText("E2E: チームの目標")).toBeVisible();
  await expect(map.getByText("E2E: わたしの目標")).toBeVisible();

  // Each goal says whose it is.
  await expect(map.getByText("組織", { exact: true })).toBeVisible();
  await expect(map.getByText("運営", { exact: true })).toBeVisible();
  await expect(map.getByText("local-owner", { exact: true })).toBeVisible();

  // An unassessed goal says so rather than showing 0%.
  await expect(map.getByText("評価未設定").first()).toBeVisible();
  await expect(map.getByText("自己評価 0%")).toBeHidden();
});

test("the lenses narrow to one owner without changing the graph", async ({
  page,
  request,
}) => {
  await threeLevels(request, "E2Eレンズ");
  await openAlignment(page, "E2Eレンズ");

  await page.getByRole("button", { name: "運営", exact: true }).click();
  const map = page.locator(".alignment-map");
  await expect(map.getByText("E2E: チームの目標")).toBeVisible();
  await expect(map.getByText("E2E: 会社の目標")).toBeHidden();

  await page.getByRole("button", { name: "すべて", exact: true }).click();
  await expect(map.getByText("E2E: 会社の目標")).toBeVisible();
});

test("the map is one workspace's graph and says so", async ({
  page,
  request,
}) => {
  await threeLevels(request, "E2E境界A");
  // Another workspace entirely, with a goal of its own. The real boundary
  // being protected is the personal workspace — `api/tests/alignment.rs`
  // covers that one, because writing a goal into the shared sample personal
  // workspace here would change what the onboarding test sees.
  const other = await shared(request, "E2E境界B");
  await post(request, `/v1/workspaces/${other}/items`, {
    kind: "outcome",
    title: "E2E: 別ワークスペースの目標",
    fields: { owner: { kind: "organization" } },
  });

  await openAlignment(page, "E2E境界A");
  await expect(
    page.locator(".alignment-map").getByText("E2E: 会社の目標"),
  ).toBeVisible();
  await expect(page.getByText("E2E: 別ワークスペースの目標")).toBeHidden();
  // And the screen states the boundary rather than leaving it implied.
  await expect(
    page.getByText("個人のワークスペースの目標はここには含まれません", {
      exact: false,
    }),
  ).toBeVisible();
});

test("contribution is shown as contribution, not as structure", async ({
  page,
  request,
}) => {
  const { workspaceId, team } = await threeLevels(request, "E2E貢献");
  const other = await post(request, `/v1/workspaces/${workspaceId}/items`, {
    kind: "outcome",
    title: "E2E: もう一つの会社目標",
    fields: { owner: { kind: "organization" } },
  });
  await post(request, `/v1/workspaces/${workspaceId}/relations`, {
    source_id: team.id,
    target_id: other.id,
    type: "contributes_to",
  });

  await openAlignment(page, "E2E貢献");
  // The team goal sits structurally under the company goal, and separately
  // reports that it contributes elsewhere. It appears under both, because
  // "what rolls up into this" includes contribution — that is the point.
  await expect(page.getByText("貢献先1件").first()).toBeVisible();
  expect(await page.getByText("貢献先1件").count()).toBe(2);
  // The second company goal is a root of its own, not a second parent.
  await expect(
    page.locator(".alignment-map").getByText("E2E: もう一つの会社目標"),
  ).toBeVisible();
});

test("the alignment map is usable at 390px", async ({ page }) => {
  // The workspace switcher is desktop chrome, so this checks the screen
  // itself rather than driving it through a selection.
  await page.setViewportSize({ width: 390, height: 844 });
  await openAlignment(page);
  expect(
    await page
      .locator(".alignment-screen")
      .evaluate((element) => element.scrollWidth <= element.clientWidth),
  ).toBe(true);
});
