import { expect, test } from "@playwright/test";

/**
 * Breaking a goal down, in the browser.
 *
 * The screen has to survive a plan that is deeper than anyone designed for,
 * has to load it a piece at a time, and has to keep "what makes this happen"
 * and "why am I doing this" as separate answers.
 */
/** Opens the sidebar, without navigating away from the current context. */
async function showMenu(page) {
  const opener = page.getByRole("button", {
    name: "メニューを開く",
    exact: true,
  });
  if (await opener.isVisible().catch(() => false)) await opener.click();
}

async function goToBreakdown(page) {
  await showMenu(page);
  const entry = page
    .getByRole("navigation", { name: "メインメニュー" })
    .getByRole("button", { name: "分解", exact: true });
  await entry.focus();
  await entry.press("Enter");
  await expect(
    page.getByRole("heading", { name: "分解", exact: true, level: 1 }),
  ).toBeVisible();
}

async function openBreakdown(page) {
  await page.goto("/");
  await goToBreakdown(page);
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

/** A workspace of its own, so one spec's plan is not another's noise. */
async function workspace(request, name) {
  const created = await post(request, "/v1/workspaces", {
    name,
    scope: "チーム",
  });
  return created.id;
}

async function chain(request, w, steps) {
  const ids = [];
  let parent;
  for (const [kind, title] of steps) {
    const body = { kind, title };
    if (parent) body.parent_id = parent;
    const item = await post(request, `/v1/workspaces/${w}/items`, body);
    parent = item.id;
    ids.push(item.id);
  }
  return ids;
}

/** Crosses to the workspace this test made, then picks a root inside it. */
async function pick(page, name, root) {
  await showMenu(page);
  await page
    .getByRole("group", { name: "現在の場所" })
    .getByRole("button", { name: new RegExp(name) })
    .click();
  // Crossing contexts lands on that context's home, so the screen is chosen
  // again rather than reloaded — a reload would throw the context away.
  await goToBreakdown(page);
  if (root) await page.getByLabel("起点").selectOption({ label: root });
}

test("a plan deeper than the screen expects still renders, a level at a time", async ({
  page,
  request,
}) => {
  const name = `E2E分解${Date.now()}`;
  const w = await workspace(request, name);
  await chain(request, w, [
    ["outcome", "10年: 選べる状態にする"],
    ["outcome", "年間: 柱を2本にする"],
    ["outcome", "四半期: 最初の10社"],
    ["initiative", "施策: 紹介の経路"],
    ["milestone", "節目: 毎月5件"],
    ["milestone", "今月: 手順を決める"],
    ["milestone", "今週: 20社に声をかける"],
    ["action", "行動: 3社に連絡する"],
  ]);

  await openBreakdown(page);
  await pick(page, name);

  const tree = page.locator(".breakdown-tree");
  await expect(tree.getByText("10年: 選べる状態にする")).toBeVisible();
  // Two levels arrive with the first request; the third is a handle.
  await expect(tree.getByText("四半期: 最初の10社")).toBeVisible();
  await expect(tree.getByText("施策: 紹介の経路")).toHaveCount(0);

  // Opening the edge fetches the next piece, and keeps doing so all the way
  // down. Nothing in the screen knows how many levels there are; each request
  // brings two, so the walk opens whatever is still at the edge.
  for (const [edge, reveal] of [
    ["四半期: 最初の10社", "節目: 毎月5件"],
    ["節目: 毎月5件", "今週: 20社に声をかける"],
    ["今週: 20社に声をかける", "行動: 3社に連絡する"],
  ]) {
    await tree.getByLabel(`${edge} の内訳`).click();
    await expect(tree.getByText(reveal)).toBeVisible();
  }
});

test("an action says why it exists, in the words that were recorded", async ({
  page,
  request,
}) => {
  const name = `E2E理由${Date.now()}`;
  const w = await workspace(request, name);
  const goal = await post(request, `/v1/workspaces/${w}/items`, {
    kind: "outcome",
    title: "目標: 解約を減らす",
  });
  const step = await post(request, `/v1/workspaces/${w}/items`, {
    kind: "milestone",
    title: "節目: 初月の伴走",
  });
  const action = await post(request, `/v1/workspaces/${w}/items`, {
    kind: "action",
    title: "行動: 初週に面談する",
  });
  await post(request, `/v1/workspaces/${w}/relations`, {
    source_id: step.id,
    target_id: goal.id,
    type: "part_of",
    rationale: "離脱は初月に集中しているため",
  });
  await post(request, `/v1/workspaces/${w}/relations`, {
    source_id: action.id,
    target_id: step.id,
    type: "part_of",
    rationale: "最初の一週間で詰まりが出るため",
  });

  await openBreakdown(page);
  await pick(page, name);
  const tree = page.locator(".breakdown-tree");
  await tree.getByRole("button", { name: "行動: 初週に面談する" }).click();

  const why = page.locator(".breakdown-why");
  // Top first, with the reasons someone actually wrote on each link.
  await expect(why.getByText("離脱は初月に集中しているため")).toBeVisible();
  await expect(why.getByText("最初の一週間で詰まりが出るため")).toBeVisible();
});

test("a link with no recorded reason says so instead of inventing one", async ({
  page,
  request,
}) => {
  const name = `E2E無理由${Date.now()}`;
  const w = await workspace(request, name);
  await chain(request, w, [
    ["outcome", "理由なし: 目標"],
    ["action", "理由なし: 行動"],
  ]);

  await openBreakdown(page);
  await pick(page, name);
  await page
    .locator(".breakdown-tree")
    .getByRole("button", { name: "理由なし: 行動" })
    .click();
  await expect(
    page.locator(".breakdown-why").getByText("理由は記録されていません"),
  ).toBeVisible();
});

test("gaps are listed as questions and nothing is filled in", async ({
  page,
  request,
}) => {
  const name = `E2E未分解${Date.now()}`;
  const w = await workspace(request, name);
  await post(request, `/v1/workspaces/${w}/items`, {
    kind: "outcome",
    title: "未分解: 何も下にない目標",
  });

  await openBreakdown(page);
  await pick(page, name);
  const gaps = page.locator(".breakdown-gaps");
  await expect(gaps.getByText("未分解: 何も下にない目標")).toBeVisible();
  await expect(gaps.getByText("分解されていません")).toBeVisible();
  await expect(
    gaps.getByText("こちらで勝手に埋めることはしません", { exact: false }),
  ).toBeVisible();
});

test("moving a branch takes what is under it and loses nothing", async ({
  page,
  request,
}) => {
  const name = `E2E移動${Date.now()}`;
  const w = await workspace(request, name);
  const first = await post(request, `/v1/workspaces/${w}/items`, {
    kind: "outcome",
    title: "移動元: 旧い柱",
  });
  const second = await post(request, `/v1/workspaces/${w}/items`, {
    kind: "outcome",
    title: "移動先: 新しい柱",
  });
  const branch = await post(request, `/v1/workspaces/${w}/items`, {
    kind: "initiative",
    title: "移動する施策",
    parent_id: first.id,
  });
  await post(request, `/v1/workspaces/${w}/items`, {
    kind: "action",
    title: "ついてくる行動",
    parent_id: branch.id,
  });

  await openBreakdown(page);
  await pick(page, name, "移動元: 旧い柱");
  await page
    .locator(".breakdown-tree")
    .getByRole("button", { name: "移動する施策", exact: true })
    .click();
  await page
    .locator(".breakdown-why")
    .getByLabel("この項目の位置")
    .selectOption({ label: "移動先: 新しい柱" });

  // The branch and everything under it are now in the new place, with the
  // second root selectable and the old one empty.
  // The chain upward now names the new parent.
  await expect(
    page.locator(".breakdown-chain").getByText("移動先: 新しい柱"),
  ).toBeVisible();
  const after = await request.get(
    `/api/v1/workspaces/${w}/items/${branch.id}/ancestry`,
  );
  const chainIds = (await after.json()).ancestors.map((step) => step.id);
  expect(chainIds).toEqual([second.id]);
  const still = await request.get(`/api/v1/workspaces/${w}/items`);
  const titles = (await still.json()).items.map((entry) => entry.title);
  expect(titles).toContain("ついてくる行動");
});
