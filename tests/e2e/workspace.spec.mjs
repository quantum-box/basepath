import { test, expect } from "@playwright/test";

async function openApp(page) {
  await page.goto("/");
  await expect(page.getByLabel("現在のワークスペース")).toHaveValue(/.+/);
}

async function snapshot(request, workspaceId) {
  const response = await request.get(
    `/api/v1/workspaces/${workspaceId}/snapshot`,
  );
  expect(response.ok()).toBeTruthy();
  return response.json();
}

async function createGoal(page, title, workspaceId) {
  await page
    .locator("#templates")
    .getByRole("button", { name: /自由形式/ })
    .click();
  const dialog = page.getByRole("dialog");
  await dialog.getByLabel("目標の名前").fill(title);
  if (workspaceId)
    await dialog.getByLabel("ワークスペース").selectOption(workspaceId);
  await dialog.getByRole("button", { name: "目標を作成", exact: true }).click();
  await expect(dialog).not.toBeVisible();
  await expect(
    page.getByRole("heading", { name: title, exact: true }),
  ).toBeVisible();
}

test("title-only goal and memo survive a fresh browser context", async ({
  page,
  request,
  browser,
}) => {
  await openApp(page);
  const workspaceId = await page
    .getByLabel("現在のワークスペース")
    .inputValue();
  const title = "E2E: 読書の時間をつくる";
  const memo = "通勤中に一章ずつ読み進める";
  await createGoal(page, title, workspaceId);
  await page.getByLabel("目標のメモ").fill(memo);
  await page.getByRole("button", { name: "メモを保存" }).click();
  await expect(
    page.getByRole("button", { name: "メモを保存" }),
  ).not.toBeVisible();

  const saved = (await snapshot(request, workspaceId)).items.filter(
    (item) => item.title === title,
  );
  expect(saved).toHaveLength(1);
  expect(saved[0]).toMatchObject({
    start_date: null,
    due_date: null,
    fields: { memo },
  });
  expect(saved[0].fields.self_assessment ?? null).toBeNull();

  // A new context has no localStorage draft: the memo must come from Rust/SQLite.
  const fresh = await browser.newContext();
  try {
    const reloaded = await fresh.newPage();
    await reloaded.goto(page.url());
    await expect(
      reloaded.getByRole("heading", { name: title, exact: true }),
    ).toBeVisible();
    await expect(reloaded.getByLabel("目標のメモ")).toHaveValue(memo);
  } finally {
    await fresh.close();
  }
});

test("action completion persists once without fabricating an outcome assessment", async ({
  page,
  request,
}) => {
  await openApp(page);
  const workspaceId = await page
    .getByLabel("現在のワークスペース")
    .inputValue();
  const goalTitle = "E2E: 毎日の学び";
  const actionTitle = "E2E: 本を一章読む";
  await createGoal(page, goalTitle, workspaceId);
  await page
    .getByRole("button", { name: "行動を追加", exact: true })
    .first()
    .click();
  const dialog = page.getByRole("dialog");
  await dialog.getByLabel("どんな行動をしますか？").fill(actionTitle);
  await dialog.getByLabel("ワークスペース").selectOption(workspaceId);
  await dialog.getByRole("button", { name: "追加する", exact: true }).click();
  await expect(dialog).not.toBeVisible();

  const checkbox = page
    .getByRole("checkbox", { name: `${actionTitle}を完了`, exact: true })
    .first();
  await checkbox.click();
  await expect(checkbox).toBeChecked();
  await page.reload();
  await expect(checkbox).toBeChecked();
  const state = await snapshot(request, workspaceId);
  const action = state.items.find((item) => item.title === actionTitle);
  const goal = state.items.find((item) => item.title === goalTitle);
  expect(action.state).toBe("done");
  expect(
    state.records.filter(
      (record) =>
        record.record_type === "completion" &&
        record.item_ids.includes(action.id),
    ),
  ).toHaveLength(1);
  expect(goal.fields.self_assessment ?? null).toBeNull();
});

test("two team workspaces keep goals in the selected workspace", async ({
  page,
  request,
}) => {
  await openApp(page);
  await page
    .getByRole("navigation", { name: "メインメニュー" })
    .getByRole("button", { name: "メンバー", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "メンバー", exact: true, level: 1 }),
  ).toBeVisible();
  const manager = page.locator(".members-page-content");
  const creator = manager.locator("details").filter({
    has: page.locator("summary", { hasText: "新しいワークスペースを作成" }),
  });
  await creator.locator("summary").click();
  const ids = [];
  for (const name of ["E2E: 読書会", "E2E: 勉強会"]) {
    await creator.getByLabel("名前", { exact: true }).fill(name);
    await creator.getByLabel("領域").selectOption("チーム");
    await creator
      .getByRole("button", { name: "作成する", exact: true })
      .click();
    await expect(
      manager.getByLabel("管理するワークスペース").locator("option:checked"),
    ).toContainText(name);
    ids.push(await manager.getByLabel("管理するワークスペース").inputValue());
  }
  expect(ids[0]).not.toBe(ids[1]);
  await page
    .getByRole("navigation", { name: "メインメニュー" })
    .getByRole("button", { name: "ホーム", exact: true })
    .click();
  const title = "E2E: 勉強会の目標";
  await createGoal(page, title);
  expect(
    (await snapshot(request, ids[1])).items.map((item) => item.title),
  ).toContain(title);
  expect((await snapshot(request, ids[0])).items).toHaveLength(0);

  const chooser = page.getByLabel("現在のワークスペース");
  await chooser.selectOption(ids[0]);
  await expect(chooser).toHaveValue(ids[0]);
  await chooser.selectOption(ids[1]);
  await page.reload();
  await expect(chooser).toHaveValue(ids[1]);
  await expect(
    page.getByRole("heading", { name: title, exact: true }),
  ).toBeVisible();
});

test("mobile navigation opens the workspace manager page and returns home", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openApp(page);
  await page
    .getByRole("button", { name: "メニューを開く", exact: true })
    .click();
  await page
    .getByRole("navigation", { name: "メインメニュー" })
    .getByRole("button", { name: "メンバー", exact: true })
    .click();
  await expect(
    page.getByRole("heading", {
      name: "メンバー",
      exact: true,
      level: 1,
    }),
  ).toBeVisible();
  await expect(page.getByLabel("管理するワークスペース")).toBeVisible();
  await page
    .getByRole("button", { name: "メニューを開く", exact: true })
    .click();
  await page
    .getByRole("navigation", { name: "メインメニュー" })
    .getByRole("button", { name: "ホーム", exact: true })
    .click();
  await expect(
    page.getByRole("heading", {
      name: "やりたいことを、動ける形に。",
      exact: true,
      level: 1,
    }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "メニューを開く", exact: true }),
  ).toBeVisible();
});
