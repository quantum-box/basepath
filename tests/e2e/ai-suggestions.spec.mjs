import { test, expect } from "@playwright/test";

test("AI proposal stays read-only until preview approval and apply", async ({
  page,
  request,
}) => {
  await page.goto("/");
  await expect(page.getByLabel("現在のワークスペース")).toHaveValue(/.+/);
  const workspaceId = await page
    .getByLabel("現在のワークスペース")
    .inputValue();
  await page
    .locator("#templates")
    .getByRole("button", { name: /自由形式/ })
    .click();
  const createDialog = page.getByRole("dialog");
  await createDialog.getByLabel("目標の名前").fill("E2E: AI提案を試す目標");
  await createDialog
    .getByRole("button", { name: "目標を作成", exact: true })
    .click();
  await expect(createDialog).not.toBeVisible();
  const beforeResponse = await request.get(
    `/api/v1/workspaces/${workspaceId}/snapshot`,
  );
  expect(beforeResponse.ok()).toBeTruthy();
  const before = await beforeResponse.json();
  const actionCount = before.items.filter(
    (item) => item.kind === "action",
  ).length;

  await page.getByRole("button", { name: "AIと次の一歩を考える" }).click();
  const dialog = page.getByRole("dialog");
  await expect(
    dialog.getByRole("heading", { name: "AIによる次の行動・振り返り提案" }),
  ).toBeVisible();
  await dialog.getByRole("button", { name: "行動と振り返りを提案" }).click();
  await expect(dialog.getByText("変更差分を確認")).not.toBeVisible();
  await expect(
    dialog.getByRole("button", { name: "差分を確認" }).first(),
  ).toBeVisible();

  await dialog.getByRole("button", { name: "差分を確認" }).first().click();
  await expect(
    dialog.getByRole("heading", { name: "変更差分を確認" }),
  ).toBeVisible();
  const pendingResponse = await request.get(
    `/api/v1/workspaces/${workspaceId}/snapshot`,
  );
  const pending = await pendingResponse.json();
  expect(pending.items.filter((item) => item.kind === "action")).toHaveLength(
    actionCount,
  );

  await dialog.getByRole("button", { name: "この差分を承認" }).click();
  await dialog.getByRole("button", { name: "承認済みの差分を適用" }).click();
  await expect(dialog).not.toBeVisible();
  const afterResponse = await request.get(
    `/api/v1/workspaces/${workspaceId}/snapshot`,
  );
  const after = await afterResponse.json();
  expect(after.items.filter((item) => item.kind === "action")).toHaveLength(
    actionCount + 1,
  );

  // The browser suite shares one isolated database. Archive only the records
  // created by this test so later cases can still exercise an empty workspace.
  const originalIds = new Set(before.items.map((item) => item.id));
  const createdItems = after.items.filter(
    (item) =>
      !originalIds.has(item.id) || item.title === "E2E: AI提案を試す目標",
  );
  for (const item of createdItems) {
    const cleanupResponse = await request.patch(
      `/api/v1/workspaces/${workspaceId}/items/${item.id}`,
      {
        headers: { "Idempotency-Key": `ai-e2e-cleanup-${item.id}` },
        data: {
          expected_version: item.version,
          archived_at: new Date().toISOString(),
        },
      },
    );
    expect(cleanupResponse.ok()).toBeTruthy();
  }
});
