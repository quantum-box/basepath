import { expect, test } from "@playwright/test";

/**
 * Personal memory, in the browser.
 *
 * The two distinctions the screen exists to keep: an AI's suggestion is not
 * something the person said, and a memory that stopped being current is not
 * wrong. Plus the boundary: a shared workspace has no memory at all.
 */
async function openMemory(page) {
  await page.goto("/");
  const menu = page.getByRole("button", {
    name: "メニューを開く",
    exact: true,
  });
  if (await menu.isVisible().catch(() => false)) await menu.click();
  const entry = page
    .getByRole("navigation", { name: "メインメニュー" })
    .getByRole("button", { name: "記憶", exact: true });
  await entry.focus();
  await entry.press("Enter");
  await expect(
    page.getByRole("heading", { name: "記憶", exact: true, level: 1 }),
  ).toBeVisible();
}

async function personalWorkspaceId(request) {
  const response = await request.get("/api/v1/workspaces");
  const workspaces = await response.json();
  return workspaces.find((workspace) => workspace.scope === "個人").id;
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

test("a suggestion is labelled as a suggestion until it is confirmed", async ({
  page,
  request,
}) => {
  const personal = await personalWorkspaceId(request);
  await post(request, `/v1/workspaces/${personal}/memories/proposals`, {
    kind: "context",
    title: "E2E: 移動の多い週は進みが遅いようだ",
    source: "観測: 完了率",
    confidence: 0.6,
  });

  await openMemory(page);
  const row = page
    .locator(".memory-list li")
    .filter({ hasText: "E2E: 移動の多い週" });
  await expect(row.getByText("AIの候補", { exact: false })).toBeVisible();
  await expect(row.getByText("確度 60%", { exact: false })).toBeVisible();

  await row.getByRole("button", { name: "自分の記憶として確認" }).click();
  await expect(row.getByText("本人が確認", { exact: true })).toBeVisible();
  // Once the person has said it, the machine's estimate of it is gone.
  await expect(row.getByText("確度", { exact: false })).toBeHidden();
});

test("a fact with no source is refused, and says what to do instead", async ({
  page,
}) => {
  await openMemory(page);
  const form = page.locator(".memory-form");
  await form.getByLabel("種類").selectOption("fact");
  await form.getByLabel("タイトル").fill("E2E: 出典のない事実");
  await form.getByRole("button", { name: "記録する", exact: true }).click();

  await expect(page.getByRole("alert")).toContainText("出典");
  // The screen already said what to do with an unsourced guess.
  await expect(
    page.getByText("根拠のない推測は「背景」や「学び」として", {
      exact: false,
    }),
  ).toBeVisible();
});

test("a memory that stopped being current is not called wrong", async ({
  page,
  request,
}) => {
  const personal = await personalWorkspaceId(request);
  await post(request, `/v1/workspaces/${personal}/memories`, {
    kind: "preference",
    title: "E2E: 前職では夜型だった",
    source: "本人",
    valid_to: "2024-12-31T00:00:00Z",
  });

  await openMemory(page);
  // Not in what stands now.
  await expect(page.getByText("E2E: 前職では夜型だった")).toBeHidden();
  await page.getByLabel("過去の記憶も表示").check();
  const row = page
    .locator(".memory-list li")
    .filter({ hasText: "E2E: 前職では夜型だった" });
  await expect(row).toBeVisible();
  await expect(row.getByText("この期間は過ぎました")).toBeVisible();
  await expect(row.getByText("誤り")).toBeHidden();
});

test("excluding a memory from AI retrieval keeps it and says so", async ({
  page,
  request,
}) => {
  const personal = await personalWorkspaceId(request);
  const memory = await post(request, `/v1/workspaces/${personal}/memories`, {
    kind: "context",
    title: "E2E: AIには渡したくない事情",
    source: "本人",
  });

  await openMemory(page);
  const row = page
    .locator(".memory-list li")
    .filter({ hasText: "E2E: AIには渡したくない事情" });
  await row
    .getByRole("button", { name: "AIには渡さない", exact: true })
    .click();
  await expect(row.getByText("AIには渡さない", { exact: true })).toBeVisible();

  // Still the person's, still there.
  const listed = await request.get(
    `/api/v1/workspaces/${personal}/memories/${memory.id}`,
  );
  expect(listed.ok()).toBeTruthy();
  expect((await listed.json()).excluded_from_retrieval).toBe(true);
});

test("similar memories are reported as a question, not merged", async ({
  page,
  request,
}) => {
  const personal = await personalWorkspaceId(request);
  for (const title of [
    "E2E: 毎週金曜に振り返りをする",
    "E2E: 毎週金曜に振り返りをする習慣",
  ]) {
    await post(request, `/v1/workspaces/${personal}/memories`, {
      kind: "preference",
      title,
      source: "本人",
    });
  }

  await openMemory(page);
  const duplicates = page.locator(".memory-duplicates");
  await expect(duplicates).toBeVisible();
  await expect(
    duplicates.getByText("自動では統合も削除もしません", { exact: false }),
  ).toBeVisible();
  // Both are still in the list.
  const listed = await request.get(`/api/v1/workspaces/${personal}/memories`);
  const titles = (await listed.json()).items.map((entry) => entry.title);
  expect(titles).toContain("E2E: 毎週金曜に振り返りをする");
  expect(titles).toContain("E2E: 毎週金曜に振り返りをする習慣");
});

test("a shared workspace has no memory, and says so plainly", async ({
  page,
  request,
}) => {
  const created = await request.post("/api/v1/workspaces", {
    headers: {
      "idempotency-key": `e2e-mem-ws-${Date.now()}`,
      "content-type": "application/json",
    },
    data: { name: "E2E記憶なし", scope: "チーム" },
  });
  expect(created.ok()).toBeTruthy();

  await openMemory(page);
  await page
    .getByRole("combobox")
    .first()
    .selectOption({ label: "E2E記憶なし" });
  await expect(
    page.getByText("記憶は個人のワークスペースにだけ保存されます", {
      exact: false,
    }),
  ).toBeVisible();
  await expect(page.locator(".memory-form")).toBeHidden();
});

test("the screen shows what an AI would actually retrieve, and why", async ({
  page,
  request,
}) => {
  const personal = await personalWorkspaceId(request);
  await post(request, `/v1/workspaces/${personal}/memories`, {
    kind: "preference",
    title: "E2E検索: 打ち合わせは30分までにしたい",
    source: "本人",
    topics: ["進め方"],
  });
  // Excluded from retrieval, with wording that would match the same query.
  await post(request, `/v1/workspaces/${personal}/memories`, {
    kind: "context",
    title: "E2E検索: 打ち合わせ中の体調のこと",
    source: "本人",
    excluded_from_retrieval: true,
  });

  await openMemory(page);
  const panel = page.locator(".memory-retrieval");
  await panel.getByLabel("試したい質問").fill("打ち合わせ");
  await panel.getByRole("button", { name: "この質問で試す" }).click();

  const results = panel.locator(".memory-retrieval-results li");
  await expect(
    results.filter({ hasText: "E2E検索: 打ち合わせは30分までにしたい" }),
  ).toHaveCount(1);
  // The excluded one is not a candidate, so it is not here — the screen and
  // the AI see the same index.
  await expect(
    results.filter({ hasText: "E2E検索: 打ち合わせ中の体調のこと" }),
  ).toHaveCount(0);
  // The ranking says what it used rather than showing a bare number.
  await expect(panel.getByTestId("retrieval-signals")).toContainText("keyword");
  await expect(panel.getByText("語句が一致", { exact: false })).toBeVisible();
});

test("a question with no match stays empty instead of reaching elsewhere", async ({
  page,
  request,
}) => {
  const personal = await personalWorkspaceId(request);
  await post(request, `/v1/workspaces/${personal}/memories`, {
    kind: "learning",
    title: "E2E無関係: 朝の30分が一番続く",
    source: "本人",
  });

  await openMemory(page);
  const panel = page.locator(".memory-retrieval");
  await panel.getByLabel("試したい質問").fill("四半期の売上見込み");
  await panel.getByRole("button", { name: "この質問で試す" }).click();
  await expect(
    panel.getByText("組織側の記録を代わりに探すことはしません", {
      exact: false,
    }),
  ).toBeVisible();
});
