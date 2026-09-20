import { expect, test } from "@playwright/test";

async function openMenu(page) {
  const opener = page.getByRole("button", {
    name: "メニューを開く",
    exact: true,
  });
  if (await opener.isVisible().catch(() => false)) await opener.click();
}

async function openScreen(page, label, heading = label) {
  const routes = { 分解: "breakdown", 目標マップ: "goals" };
  const url = new URL(page.url());
  url.pathname = url.pathname.replace(/\/[^/]+$/, `/${routes[label]}`);
  await page.goto(url.toString());
  await expect(
    page.getByRole("heading", { name: heading, exact: true, level: 1 }),
  ).toBeVisible();
}

async function createWorkspace(page, name) {
  await page.goto("/personal/members");
  const manager = page.locator(".members-page-content");
  const creator = manager.locator("details").filter({
    has: page.locator("summary", { hasText: "新しいワークスペースを作成" }),
  });
  await creator.locator("summary").click();
  await creator.getByLabel("名前", { exact: true }).fill(name);
  await creator.getByLabel("領域").selectOption("チーム");
  await creator.getByRole("button", { name: "作成する", exact: true }).click();
  await expect(manager.getByLabel("管理するワークスペース").locator("option:checked")).toContainText(name);
  await page.getByRole("navigation", { name: "メインメニュー" }).getByRole("button", { name: "概要", exact: true }).click();
}

async function createRoot(page, title) {
  await page.locator("#templates").getByRole("button", { name: /自由形式/ }).click();
  const dialog = page.getByRole("dialog");
  await dialog.getByLabel("目標の名前").fill(title);
  await dialog.getByRole("button", { name: "目標を作成", exact: true }).click();
  await expect(dialog).not.toBeVisible();
  await expect(page.getByRole("heading", { name: title, exact: true })).toBeVisible();
}

async function addChild(page, parent, kind, title) {
  const tree = page.locator(".breakdown-tree");
  const add = tree.getByRole("button", {
    name: `${parent}に子項目を追加`,
    exact: true,
  });
  await expect(add).toBeVisible();
  await add.click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toContainText(parent);
  await dialog.getByLabel("種類").selectOption(kind);
  await dialog.getByLabel("取り組みの名前").fill(title);
  await dialog.getByRole("button", { name: "追加する", exact: true }).click();
  await expect(dialog).not.toBeVisible();
  await expect(tree.getByText(title, { exact: true })).toBeVisible();
}

test("creates, edits, and completes an eight-level plan through the UI", async ({
  page,
}) => {
  const levels = [
    ["outcome", "UI深掘り: 年間目標"],
    ["outcome", "UI深掘り: 四半期目標"],
    ["initiative", "UI深掘り: 取り組み"],
    ["milestone", "UI深掘り: 節目1"],
    ["milestone", "UI深掘り: 節目2"],
    ["milestone", "UI深掘り: 節目3"],
    ["milestone", "UI深掘り: 節目4"],
    ["action", "UI深掘り: 次の一歩"],
  ];
  await createWorkspace(page, `UI深掘りworkspace-${Date.now()}`);
  await createRoot(page, levels[0][1]);
  await openScreen(page, "分解");

  let parent = levels[0][1];
  for (const [kind, title] of levels.slice(1)) {
    await addChild(page, parent, kind, title);
    parent = title;
  }

  // The deepest Action is visible and terminal: it has no child affordance.
  const tree = page.locator(".breakdown-tree");
  await expect(
    tree.getByRole("button", { name: "UI深掘り: 次の一歩に子項目を追加", exact: true }),
  ).toHaveCount(0);
  await expect(
    tree.getByRole("button", { name: "UI深掘り: 次の一歩の兄弟項目を追加", exact: true }),
  ).toBeVisible();

  await openScreen(page, "目標マップ");
  const action = page.getByRole("button", {
    name: "UI深掘り: 次の一歩",
    exact: true,
  });
  await expect(action).toBeVisible();
  await action.click();

  const detail = page.getByRole("dialog");
  await expect(detail).toContainText("行動");
  await detail.getByLabel("名前").fill("UI深掘り: 完了する一歩");
  await detail.getByRole("button", { name: "変更を保存", exact: true }).click();
  await expect(detail).not.toBeVisible();

  await page.getByRole("button", { name: "UI深掘り: 完了する一歩", exact: true }).click();
  const edited = page.getByRole("dialog");
  await edited.getByRole("button", { name: "この日の完了を記録", exact: true }).click();
  await expect(edited).toContainText("完了");
});
