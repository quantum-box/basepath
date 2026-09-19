import { expect, test } from "@playwright/test";

/**
 * The approval screen on Basepath's own origin.
 *
 * This is where approving actually happens, because only here does the request
 * carry the person's own session. The test drives it the way a deep link from
 * a conversation would: straight to /changes/{workspace}/{id}.
 */
/**
 * A workspace of this test's own.
 *
 * The browser suite shares one database, and these tests create and apply
 * plan items. Doing that in the sample personal workspace would change what
 * the onboarding tests see.
 */
async function ownWorkspace(request, name) {
  // The idempotency key travels in a header, so it stays ASCII.
  const response = await request.post("/api/v1/workspaces", {
    headers: {
      "idempotency-key": `e2e-ws-${Date.now()}-${Math.random().toString(16).slice(2)}`,
      "content-type": "application/json",
    },
    data: { name, scope: "チーム" },
  });
  expect(response.ok(), await response.text()).toBeTruthy();
  return (await response.json()).id;
}

async function proposeChange(request, workspaceId) {
  // The AI-facing proposal path: a change set that creates one action.
  const response = await request.post(
    `/api/v1/workspaces/${workspaceId}/changesets/preview`,
    {
      headers: {
        "idempotency-key": `e2e-preview-${Date.now()}`,
        "content-type": "application/json",
      },
      data: {
        title: "E2E: 承認画面の変更案",
        operations: [
          {
            method: "POST",
            path: `/v1/workspaces/${workspaceId}/items`,
            body: { kind: "action", title: "E2E: 承認で作られる行動" },
          },
        ],
      },
    },
  );
  expect(response.ok(), await response.text()).toBeTruthy();
  return response.json();
}

test("a deep link approves the shown content, and approving is what applies it", async ({
  page,
  request,
}) => {
  const workspaceId = await ownWorkspace(request, "E2E承認A");
  const change = await proposeChange(request, workspaceId);

  await page.goto(`/changes/${workspaceId}/${change.id}`);
  const review = page.getByRole("region", { name: "変更案" });
  await expect(review).toBeVisible();
  await expect(
    review.getByRole("heading", { name: "E2E: 承認画面の変更案" }),
  ).toBeVisible();
  // The diff shows what would be created, before anything is.
  await expect(review.getByText("追加1・更新0・削除0")).toBeVisible();
  await expect(
    review.getByText("E2E: 承認で作られる行動").first(),
  ).toBeVisible();
  await expect(review.getByText("承認待ち")).toBeVisible();

  // Nothing exists yet.
  const before = await (
    await request.get(`/api/v1/workspaces/${workspaceId}/snapshot`)
  ).json();
  expect(
    before.items.filter((item) => item.title === "E2E: 承認で作られる行動"),
  ).toHaveLength(0);

  await review
    .getByRole("button", { name: "この内容で承認して反映する" })
    .click();
  await expect(
    page.getByText("計画へ反映しました", { exact: false }),
  ).toBeVisible();

  // The person is done. Nothing is left for them to come back and press —
  // which is what used to let an approval expire having written nothing.
  const after = await (
    await request.get(`/api/v1/workspaces/${workspaceId}/snapshot`)
  ).json();
  expect(
    after.items.filter((item) => item.title === "E2E: 承認で作られる行動"),
  ).toHaveLength(1);
  await expect(review.getByText("適用済み")).toBeVisible();
  await expect(
    review.getByRole("button", { name: "承認済みの内容を適用する" }),
  ).toBeHidden();
});

test("withdrawing a proposal leaves the plan untouched", async ({
  page,
  request,
}) => {
  const workspaceId = await ownWorkspace(request, "E2E承認B");
  const change = await proposeChange(request, workspaceId);

  await page.goto(`/changes/${workspaceId}/${change.id}`);
  const review = page.getByRole("region", { name: "変更案" });
  await review.getByRole("button", { name: "この案を取り下げる" }).click();
  await expect(page.getByText("取り下げました")).toBeVisible();
  await expect(review.getByText("却下済み")).toBeVisible();
  // A withdrawn proposal offers no way forward.
  await expect(
    review.getByRole("button", { name: "この内容で承認して反映する" }),
  ).toBeHidden();

  const after = await (
    await request.get(`/api/v1/workspaces/${workspaceId}/snapshot`)
  ).json();
  expect(
    after.items.filter((item) => item.title === "E2E: 承認で作られる行動"),
  ).toHaveLength(0);
});

test("an unknown change id says so instead of showing an empty approval", async ({
  page,
  request,
}) => {
  const workspaceId = await ownWorkspace(request, "E2E承認C");
  await page.goto(`/changes/${workspaceId}/change_does_not_exist`);
  await expect(
    page.getByText("この変更案は見つかりません", { exact: false }),
  ).toBeVisible();
  await expect(page.getByRole("region", { name: "変更案" })).toBeHidden();
});

test("a committing value is shown with where it came from, next to the diff", async ({
  page,
  request,
}) => {
  const workspaceId = await ownWorkspace(request, "E2E提案の根拠");
  const response = await request.post(
    `/api/v1/workspaces/${workspaceId}/changesets/preview`,
    {
      headers: {
        "idempotency-key": `e2e-basis-${Date.now()}`,
        "content-type": "application/json",
      },
      data: {
        title: "E2E: 根拠つきの分解案",
        assumptions: ["E2E前提: 英語圏から先に着手する想定で並べています"],
        operations: [
          {
            method: "POST",
            path: `/v1/workspaces/${workspaceId}/items`,
            body: {
              kind: "initiative",
              title: "E2E: 期限つきの取り組み",
              due_date: "2027-03-31",
            },
            basis: "E2E根拠: 本人が「2027年度の期初までに」と話していたため",
          },
        ],
      },
    },
  );
  expect(response.ok(), await response.text()).toBeTruthy();
  const change = await response.json();

  await page.goto(`/changes/${workspaceId}/${change.id}`);
  const review = page.getByRole("region", { name: "変更案" });
  // The date is not just one row in a table of fields: it is named, with the
  // sentence saying where it came from, because after approval it reads as
  // something the person decided.
  await expect(review.getByText("due_date", { exact: false })).toBeVisible();
  await expect(
    review.getByText("本人が「2027年度の期初までに」と話していたため", {
      exact: false,
    }),
  ).toBeVisible();
  // And the reasoning behind the whole proposal, since approving it is
  // agreeing to that too.
  await expect(
    review.getByText("英語圏から先に着手する想定で並べています", {
      exact: false,
    }),
  ).toBeVisible();
});
