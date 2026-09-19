import { expect, test } from "@playwright/test";

/**
 * The MCP App, driven through a real MCP Apps host.
 *
 * `tests/harness/host.html` runs the published AppBridge against the committed
 * bundle in a sandboxed iframe, so this exercises the actual postMessage
 * protocol: initialization, capability negotiation, tool calls and errors.
 */
const HARNESS = "/tests/harness/host.html";

async function openHarness(page, { fixtures, capabilities } = {}) {
  await page.addInitScript(
    ({ fixtures, capabilities }) => {
      if (fixtures) window.__fixtures = fixtures;
      if (capabilities) window.__hostCapabilities = capabilities;
    },
    { fixtures, capabilities },
  );
  await page.goto(HARNESS);
  await page.waitForFunction(() => window.__bridgeReady === true);
  return page.frameLocator("#app");
}

test("renders the plan from tool results inside the host", async ({ page }) => {
  const app = await openHarness(page);

  await expect(app.getByRole("heading", { name: "個人" })).toBeVisible();
  await expect(app.getByText("年間目標")).toBeVisible();
  await expect(app.getByText("今日の行動").first()).toBeVisible();
  // A truncated graph says so rather than looking complete.
  await expect(
    app.getByText("これは計画の一部です", { exact: false }),
  ).toBeVisible();
  await expect(app.getByText("2026-09-18の行動")).toBeVisible();

  // The app asked the host for exactly the data it needed, through the host.
  const calls = await page.evaluate(() => window.__calls);
  expect(calls).toContain("pathbase_get_context");
  expect(calls).toContain("pathbase_get_graph");
  expect(calls).toContain("pathbase_get_today");
});

test("explains a refusal instead of rendering an empty plan", async ({
  page,
}) => {
  const app = await openHarness(page, {
    fixtures: {
      "pathbase_get_context:error": {
        code: "INSUFFICIENT_SCOPE",
        message: "この接続には pathbase.read の権限がありません",
        status: 403,
      },
    },
  });

  await expect(app.getByRole("alert")).toContainText("権限が足りません");
  await expect(
    app.getByText("目標と行動を読む", { exact: false }),
  ).toBeVisible();
  await expect(app.getByRole("button", { name: "再試行" })).toBeVisible();
});

test("a host without tool support still says why nothing is shown", async ({
  page,
}) => {
  const app = await openHarness(page, { capabilities: {} });
  await expect(app.getByRole("alert")).toContainText(
    "このホストでは表示できません",
  );
});

test("an empty workspace reads as empty, not as a failure", async ({
  page,
}) => {
  const app = await openHarness(page, {
    fixtures: {
      pathbase_get_context: {
        workspaces: [{ id: "personal", name: "個人", scope: "個人" }],
      },
      pathbase_get_graph: {
        items: [],
        relations: [],
        truncated: false,
        limit: 200,
      },
      pathbase_get_today: { local_date: "2026-09-18", items: [] },
    },
  });
  await expect(app.getByText("まだ目標がありません")).toBeVisible();
  await expect(
    app.getByText("この日に予定された行動はありません"),
  ).toBeVisible();
});

test("the app is readable in a narrow conversation pane", async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 720 });
  const app = await openHarness(page);
  await expect(app.getByRole("heading", { name: "個人" })).toBeVisible();
  const overflow = await page
    .frameLocator("#app")
    .locator("body")
    .evaluate((body) => body.scrollWidth - body.clientWidth);
  expect(overflow).toBeLessThanOrEqual(1);
});

test("the bundle loads nothing from another origin", async ({
  page,
  baseURL,
}) => {
  // The resource declares an empty CSP, so anything it tried to fetch from
  // elsewhere would simply be blocked by the host. Catching it here means the
  // bundle never grows a dependency on a CDN in the first place.
  const local = new URL(baseURL).host;
  const external = [];
  page.on("request", (request) => {
    const url = new URL(request.url());
    if (
      url.protocol !== "data:" &&
      url.protocol !== "blob:" &&
      url.host !== local
    ) {
      external.push(request.url());
    }
  });
  const app = await openHarness(page);
  await expect(app.getByRole("heading", { name: "個人" })).toBeVisible();
  expect(external).toEqual([]);
});

test("shows the goal map, folding, selection detail, today and the week", async ({
  page,
}) => {
  const app = await openHarness(page);

  // The week the workspace is in, with its timezone stated.
  await expect(
    app.getByText("今週（2026-09-14 〜 2026-09-20 / Asia/Tokyo）"),
  ).toBeVisible();
  await expect(app.getByText("毎日の習慣")).toBeVisible();
  await expect(app.getByText("未実施")).toBeVisible();
  await expect(app.getByText("日付のない項目が1件あります")).toBeVisible();
  // The action carries who it is for and when it is due.
  await expect(app.getByText("担当 us_me")).toBeVisible();
  await expect(app.getByText("期限 2026-09-20")).toBeVisible();

  // Folding a branch hides its children and says how many are hidden.
  const toggle = app.getByRole("button", { name: "年間目標の下位を閉じる" });
  await expect(app.getByText("今日の行動").first()).toBeVisible();
  await toggle.click();
  await expect(app.getByText("下位1件")).toBeVisible();
  await expect(app.getByRole("button", { name: "すべて開く" })).toBeVisible();
  await app.getByRole("button", { name: "すべて開く" }).click();
  await expect(app.getByText("下位1件")).toBeHidden();

  // Selecting a node opens its detail; selecting again closes it.
  const node = app.getByRole("button", { name: "年間目標の詳細" });
  await node.click();
  const detail = app.getByRole("region", { name: "選択中の項目" });
  await expect(detail).toBeVisible();
  await expect(detail.getByText("自己評価")).toBeVisible();
  await node.click();
  await expect(detail).toBeHidden();
});

test("completing an action is reported as a proposal, never as done", async ({
  page,
}) => {
  const app = await openHarness(page);
  await app.getByRole("button", { name: "完了を提案" }).click();

  // The wording must not claim the plan changed.
  await expect(
    app.getByText("変更案を作成しました", { exact: false }),
  ).toBeVisible();
  // The checkbox stays unchecked: the server still says it is not done.
  await expect(app.locator("[data-done='false']").first()).toBeVisible();

  // The view is reloaded from the server rather than guessed at.
  const calls = await page.evaluate(() => window.__calls);
  expect(
    calls.filter((name) => name === "pathbase_complete_action"),
  ).toHaveLength(1);
  expect(calls.lastIndexOf("pathbase_get_today")).toBeGreaterThan(
    calls.indexOf("pathbase_complete_action"),
  );
});

test("a refused proposal says why instead of looking like it worked", async ({
  page,
}) => {
  const app = await openHarness(page, {
    fixtures: {
      pathbase_get_context: {
        workspaces: [
          {
            id: "personal",
            name: "個人",
            scope: "個人",
            timezone: "Asia/Tokyo",
            role: "owner",
          },
        ],
      },
      pathbase_get_graph: {
        items: [],
        relations: [],
        truncated: false,
        limit: 200,
      },
      pathbase_get_today: {
        local_date: "2026-09-18",
        items: [
          {
            item: { id: "a1", title: "衝突する行動", version: 3, fields: {} },
            completed: false,
            occurrence_key: "a1:2026-09-18",
          },
        ],
      },
      pathbase_get_week: {
        start: "2026-09-14",
        end: "2026-09-20",
        timezone: "Asia/Tokyo",
        days: [],
        unscheduled: [],
      },
      "pathbase_complete_action:error": {
        code: "VERSION_CONFLICT",
        message: "別の操作で更新されています",
        status: 409,
      },
    },
  });

  await app.getByRole("button", { name: "完了を提案" }).click();
  await expect(
    app.getByText("別の操作で更新されています", { exact: false }),
  ).toBeVisible();
  // Nothing may look like it succeeded.
  await expect(
    app.getByText("変更案を作成しました", { exact: false }),
  ).toBeHidden();
  await expect(app.locator("[data-done='false']").first()).toBeVisible();
});

test("switching workspace does not carry folding or selection across", async ({
  page,
}) => {
  const app = await openHarness(page, {
    fixtures: {
      pathbase_get_context: {
        workspaces: [
          {
            id: "personal",
            name: "個人",
            scope: "個人",
            timezone: "Asia/Tokyo",
            role: "owner",
          },
          {
            id: "team",
            name: "チーム",
            scope: "チーム",
            timezone: "UTC",
            role: "viewer",
          },
        ],
      },
      pathbase_get_graph: {
        items: [
          {
            id: "g1",
            title: "個人の目標",
            kind: "outcome",
            state: "active",
            fields: {},
          },
        ],
        relations: [],
        truncated: false,
        limit: 200,
      },
      pathbase_get_today: { local_date: "2026-09-18", items: [] },
      pathbase_get_week: {
        start: "2026-09-14",
        end: "2026-09-20",
        timezone: "Asia/Tokyo",
        days: [],
        unscheduled: [],
      },
    },
  });

  await expect(app.getByRole("heading", { name: "個人" })).toBeVisible();
  await app.getByRole("button", { name: "個人の目標の詳細" }).click();
  await expect(app.getByRole("region", { name: "選択中の項目" })).toBeVisible();

  await app.getByLabel("ワークスペース").selectOption("team");
  await expect(app.getByRole("heading", { name: "チーム" })).toBeVisible();
  // The previous workspace's selection must not survive the switch.
  await expect(app.getByRole("region", { name: "選択中の項目" })).toBeHidden();
  // The viewer's role is shown, so it is clear what they may do.
  await expect(app.getByText("viewer")).toBeVisible();
});

test("a deep tree and long Japanese titles stay readable at 320px", async ({
  page,
}) => {
  const deep = [];
  const relations = [];
  const long =
    "とても長い日本語のタイトルで折り返しの挙動を確認するための項目名です";
  for (let depth = 0; depth < 8; depth += 1) {
    deep.push({
      id: `n${depth}`,
      title: `${long}${depth}`,
      kind: depth === 0 ? "outcome" : "initiative",
      state: "active",
      fields: {},
    });
    if (depth > 0) {
      relations.push({
        id: `r${depth}`,
        source_id: `n${depth}`,
        target_id: `n${depth - 1}`,
        type: "part_of",
      });
    }
  }
  await page.setViewportSize({ width: 320, height: 800 });
  const app = await openHarness(page, {
    fixtures: {
      pathbase_get_context: {
        workspaces: [
          {
            id: "personal",
            name: "個人",
            scope: "個人",
            timezone: "Asia/Tokyo",
            role: "owner",
          },
        ],
      },
      pathbase_get_graph: {
        items: deep,
        relations,
        truncated: false,
        limit: 200,
      },
      pathbase_get_today: { local_date: "2026-09-18", items: [] },
      pathbase_get_week: {
        start: "2026-09-14",
        end: "2026-09-20",
        timezone: "Asia/Tokyo",
        days: [],
        unscheduled: [],
      },
    },
  });

  await expect(app.getByText(`${long}7`)).toBeVisible();
  const overflow = await page
    .frameLocator("#app")
    .locator("body")
    .evaluate((body) => body.scrollWidth - body.clientWidth);
  expect(overflow).toBeLessThanOrEqual(1);
});

test("a change set is shown as a diff that cannot be approved in the app", async ({
  page,
}) => {
  const app = await openHarness(page);

  const review = app.getByRole("region", { name: "変更案" });
  await expect(review).toBeVisible();
  await expect(
    review.getByRole("heading", { name: "AIからの計画変更" }),
  ).toBeVisible();
  await expect(review.getByText("追加1・更新0・削除1")).toBeVisible();
  // Who proposed it, and through which connection.
  await expect(
    review.getByText("接続 mcpconn_1", { exact: false }),
  ).toBeVisible();
  // A deletion is called a deletion.
  await expect(review.getByText("この項目は削除されます")).toBeVisible();
  // The new item's fields are shown before/after.
  await expect(review.getByRole("row", { name: /タイトル/ })).toBeVisible();

  // Approval is not offered here, and the reason is stated.
  await expect(
    review.getByRole("button", { name: "この内容で承認して反映する" }),
  ).toBeHidden();
  await expect(
    review.getByText("ここでの操作は本人確認の代わりになりません", {
      exact: false,
    }),
  ).toBeVisible();
  await expect(
    review.getByRole("button", { name: "Basepathで承認する" }),
  ).toBeVisible();
  await expect(
    review.getByText("https://basepath.example/changes/personal/change_1"),
  ).toBeVisible();
});

test("withdrawing a proposal goes through the server and reloads", async ({
  page,
}) => {
  const app = await openHarness(page);
  await app.getByRole("button", { name: "この案を取り下げる" }).click();
  await expect(app.getByText("変更案を取り下げました")).toBeVisible();

  const calls = await page.evaluate(() => window.__calls);
  expect(calls).toContain("pathbase_reject_change");
  // The state is re-read rather than assumed.
  expect(calls.lastIndexOf("pathbase_list_changes")).toBeGreaterThan(
    calls.indexOf("pathbase_reject_change"),
  );
});

test("applying is offered only once someone has approved", async ({ page }) => {
  const approved = await page.evaluate(() => null);
  void approved;
  const app = await openHarness(page, {
    fixtures: {
      pathbase_get_context: {
        workspaces: [
          {
            id: "personal",
            name: "個人",
            scope: "個人",
            timezone: "Asia/Tokyo",
            role: "owner",
          },
        ],
        basepath_url: "https://basepath.example",
      },
      pathbase_get_graph: {
        items: [],
        relations: [],
        truncated: false,
        limit: 200,
      },
      pathbase_get_today: { local_date: "2026-09-18", items: [] },
      pathbase_get_week: {
        start: "2026-09-14",
        end: "2026-09-20",
        timezone: "Asia/Tokyo",
        days: [],
        unscheduled: [],
      },
      pathbase_list_changes: {
        items: [
          {
            id: "change_2",
            workspace_id: "personal",
            title: "承認済みの案",
            status: "approved",
            hash: "digest-2",
            approved_by: "us_me",
            approved_at: "2026-09-18T01:00:00Z",
            created_at: "2026-09-18T00:00:00Z",
            expires_at: "2099-01-01T00:00:00Z",
            changes: [],
          },
        ],
      },
      pathbase_apply_changes: {
        changeset: { id: "change_2", status: "applied" },
      },
    },
  });

  const review = app.getByRole("region", { name: "変更案" });
  // Approving applies, so this one was approved before that was so: it is
  // still not in the plan, and the label says that rather than sounding done.
  await expect(review.getByText("承認済み・未反映")).toBeVisible();
  await expect(review.getByText("承認 us_me", { exact: false })).toBeVisible();
  await review
    .getByRole("button", { name: "承認済みの内容を適用する" })
    .click();
  await expect(app.getByText("承認済みの内容を適用しました")).toBeVisible();
  const calls = await page.evaluate(() => window.__calls);
  expect(calls).toContain("pathbase_apply_changes");
});

test("an expired proposal offers nothing but a rebuild", async ({ page }) => {
  const app = await openHarness(page, {
    fixtures: {
      pathbase_get_context: {
        workspaces: [
          {
            id: "personal",
            name: "個人",
            scope: "個人",
            timezone: "Asia/Tokyo",
            role: "owner",
          },
        ],
        basepath_url: "https://basepath.example",
      },
      pathbase_get_graph: {
        items: [],
        relations: [],
        truncated: false,
        limit: 200,
      },
      pathbase_get_today: { local_date: "2026-09-18", items: [] },
      pathbase_get_week: {
        start: "2026-09-14",
        end: "2026-09-20",
        timezone: "Asia/Tokyo",
        days: [],
        unscheduled: [],
      },
      pathbase_list_changes: {
        items: [
          {
            id: "change_3",
            workspace_id: "personal",
            title: "期限切れの案",
            status: "pending",
            hash: "digest-3",
            created_at: "2020-01-01T00:00:00Z",
            expires_at: "2020-01-01T00:30:00Z",
            changes: [],
          },
        ],
      },
    },
  });

  const review = app.getByRole("region", { name: "変更案" });
  await expect(review.getByText("期限切れ").first()).toBeVisible();
  await expect(
    review.getByText("もう一度作り直してください", { exact: false }),
  ).toBeVisible();
  await expect(
    review.getByRole("button", { name: "Basepathで承認する" }),
  ).toBeHidden();
});

/**
 * The weekly review, in the conversation.
 *
 * The plan is what is intended; this is what happened. The tests below are
 * mostly about what must *not* appear: an invented number, a completion rate
 * over an empty week, or anything that would let the app finish a review on
 * the person's behalf.
 */
async function openWeekly(page, options) {
  const app = await openHarness(page, options);
  await app.getByRole("button", { name: "週次レビュー", exact: true }).click();
  return app;
}

test("the weekly review shows the server's numbers, not recomputed ones", async ({
  page,
}) => {
  const app = await openWeekly(page);

  await expect(
    app.getByRole("heading", { name: "週次レビュー", exact: true }),
  ).toBeVisible();
  await expect(app.getByText("2026-09-14〜2026-09-20").first()).toBeVisible();
  // Four occurrences were planned; only two rows came back. The totals are the
  // server's, so the rate is 75%, not something derived from what is listed.
  await expect(app.getByText("4件", { exact: true })).toBeVisible();
  await expect(app.getByText("75%", { exact: true })).toBeVisible();
  await expect(app.getByText("2026-09-15 · 完了 · 記録あり")).toBeVisible();
  await expect(app.getByText("2026-09-16 · 見送り")).toBeVisible();

  const calls = await page.evaluate(() => window.__calls);
  expect(calls).toContain("pathbase_get_weekly_review");
});

test("an unmeasured metric reads as unmeasured, never as zero", async ({
  page,
}) => {
  const app = await openWeekly(page);

  await expect(app.getByText("12 km", { exact: true })).toBeVisible();
  await expect(app.getByText("前週差 +3 km", { exact: true })).toBeVisible();
  // Nothing was observed, so nothing is claimed.
  await expect(app.getByText("未計測", { exact: true })).toBeVisible();
  await expect(
    app.getByText("観測がありません", { exact: true }),
  ).toBeVisible();
  // A measurement with nothing to compare against says so rather than "+0".
  await expect(
    app.getByText("古い観測（2週間以上前）", { exact: true }),
  ).toBeVisible();
  await expect(app.getByText("+0", { exact: false })).toBeHidden();
  // Assessment and completion are shown as separate things.
  await expect(app.getByText("行動完了率とは別指標")).toBeVisible();
  await expect(app.getByText("自己評価 40%", { exact: true })).toBeVisible();
  await expect(app.getByText("評価未設定", { exact: true })).toBeVisible();
});

test("an empty week says it is empty instead of showing a 0% week", async ({
  page,
}) => {
  const app = await openWeekly(page, {
    fixtures: {
      pathbase_get_context: {
        workspaces: [
          {
            id: "personal",
            name: "個人",
            scope: "個人",
            timezone: "Asia/Tokyo",
            role: "owner",
          },
        ],
      },
      pathbase_get_graph: { items: [], relations: [] },
      pathbase_get_today: { local_date: "2026-09-18", items: [] },
      pathbase_get_week: {
        start: "2026-09-14",
        end: "2026-09-20",
        timezone: "Asia/Tokyo",
        days: [],
        unscheduled: [],
      },
      pathbase_list_changes: { items: [] },
      pathbase_get_weekly_review: {
        workspace_id: "personal",
        timezone: "Asia/Tokyo",
        week_start: "2026-09-14",
        week_end: "2026-09-20",
        actions: {
          total: 0,
          completed: 0,
          skipped: 0,
          incomplete: 0,
          items: [],
        },
        goals: [],
        metrics: [],
        members: [],
        review: null,
        history: [],
      },
    },
  });

  await expect(
    app.getByText("この週はまだ集計できるデータがありません"),
  ).toBeVisible();
  await expect(app.getByText("推測値は作らず", { exact: false })).toBeVisible();
  // A rate over nothing is not 0%.
  await expect(app.getByText("—", { exact: true })).toBeVisible();
  await expect(app.getByText("0%", { exact: true })).toBeHidden();
  await expect(app.getByText("予定された行動はありません。")).toBeVisible();
  await expect(app.getByText("成果指標はありません。")).toBeVisible();
});

test("writing the review here is a proposal, and it says so", async ({
  page,
}) => {
  const app = await openWeekly(page);

  // The app cannot finish a week: it has no save and no finalize at all.
  await expect(
    app.getByRole("button", { name: "レビューを確定", exact: true }),
  ).toBeHidden();
  await expect(
    app.getByRole("button", { name: "下書き保存", exact: true }),
  ).toBeHidden();
  await expect(
    app.getByText("確定はBasepathで本人が承認したときだけ行われます", {
      exact: false,
    }),
  ).toBeVisible();

  const propose = app.getByRole("button", {
    name: "変更案にする",
    exact: true,
  });
  // Nothing written yet, so there is nothing to propose.
  await expect(propose).toBeDisabled();

  await app.getByLabel("学び", { exact: true }).fill("観測: 3回完了した");
  await app
    .getByLabel("課題", { exact: true })
    .fill("推測: 移動時間が原因かも");
  await app
    .getByLabel("次週の重点", { exact: true })
    .fill("質問: 朝に動かせますか");
  await expect(propose).toBeEnabled();
  await propose.click();

  await expect(
    app.getByText("この週はまだ確定していません", { exact: false }),
  ).toBeVisible();

  const [request] = await page.evaluate(() =>
    window.__requests.filter(
      (call) => call.name === "pathbase_preview_changes",
    ),
  );
  expect(request).toBeTruthy();
  const [operation] = request.arguments.operations;
  expect(operation.method).toBe("POST");
  expect(operation.path).toBe("/v1/workspaces/personal/weekly-reviews/draft");
  expect(operation.body.week_start).toBe("2026-09-14");
  expect(operation.body.learnings).toBe("観測: 3回完了した");
  // No draft exists yet, so there is no version this could overwrite.
  expect("expected_version" in operation.body).toBe(false);
  // An idempotency key has to survive an HTTP header.
  expect(request.arguments.idempotency_key).toMatch(/^[\x20-\x7e]+$/);
});

test("resending the same review text does not become a second proposal", async ({
  page,
}) => {
  const app = await openWeekly(page);
  const propose = app.getByRole("button", {
    name: "変更案にする",
    exact: true,
  });
  const proposals = () =>
    page.evaluate(() =>
      window.__requests
        .filter((call) => call.name === "pathbase_preview_changes")
        .map((call) => call.arguments.idempotency_key),
    );
  const sendOnce = async (text, count) => {
    await app.getByLabel("学び", { exact: true }).fill(text);
    await expect(propose).toBeEnabled();
    await propose.click();
    await page.waitForFunction(
      (expected) =>
        window.__requests.filter(
          (call) => call.name === "pathbase_preview_changes",
        ).length === expected,
      count,
    );
  };

  await sendOnce("同じ文章", 1);
  await sendOnce("同じ文章", 2);
  const keys = await proposals();
  // The same text is the same proposal; the key lets the server collapse it.
  expect(keys[0]).toBe(keys[1]);

  // Edited text is a different proposal and must be able to reach the person.
  await sendOnce("書き直した文章", 3);
  const edited = await proposals();
  expect(edited[2]).not.toBe(edited[0]);
});

test("stepping to another week asks the server for that week", async ({
  page,
}) => {
  const app = await openWeekly(page);
  await app.getByRole("button", { name: "前の週", exact: true }).click();

  await page.waitForFunction(() =>
    window.__requests.some(
      (call) =>
        call.name === "pathbase_get_weekly_review" &&
        call.arguments.week_start === "2026-09-07",
    ),
  );
  // The week is always a Monday, because that is what the server accepts.
  const weeks = await page.evaluate(() =>
    window.__requests
      .filter((call) => call.name === "pathbase_get_weekly_review")
      .map((call) => call.arguments.week_start),
  );
  for (const week of weeks) {
    expect(new Date(`${week}T00:00:00Z`).getUTCDay()).toBe(1);
  }
});

test("a refused weekly review explains itself instead of showing a blank week", async ({
  page,
}) => {
  const app = await openWeekly(page, {
    fixtures: {
      pathbase_get_context: {
        workspaces: [
          {
            id: "personal",
            name: "個人",
            scope: "個人",
            timezone: "Asia/Tokyo",
            role: "owner",
          },
        ],
      },
      pathbase_get_graph: { items: [], relations: [] },
      pathbase_get_today: { local_date: "2026-09-18", items: [] },
      pathbase_get_week: {
        start: "2026-09-14",
        end: "2026-09-20",
        timezone: "Asia/Tokyo",
        days: [],
        unscheduled: [],
      },
      pathbase_list_changes: { items: [] },
      "pathbase_get_weekly_review:error": {
        code: "INSUFFICIENT_SCOPE",
        message: "scope不足",
        status: 403,
      },
    },
  });

  await expect(app.getByRole("alert")).toContainText("権限が足りません");
  await expect(
    app.getByText("目標と行動を読む", { exact: false }),
  ).toBeVisible();
});

test("the weekly review stays readable in a narrow conversation pane", async ({
  page,
}) => {
  await page.setViewportSize({ width: 320, height: 720 });
  const app = await openWeekly(page);
  await expect(app.getByText("走行距離", { exact: true })).toBeVisible();

  const overflow = await app
    .locator(".weekly-panel")
    .evaluate((element) => element.scrollWidth - element.clientWidth);
  expect(overflow).toBeLessThanOrEqual(1);
});

/**
 * A quiet workspace, so a pushed proposal is the only thing on screen.
 *
 * The tests below are about the moment a proposal arrives and what a person
 * can do about it there. Fixtures for the plan itself would only add noise.
 */
const QUIET = {
  pathbase_get_context: {
    workspaces: [
      {
        id: "personal",
        name: "個人",
        scope: "個人",
        timezone: "Asia/Tokyo",
        role: "owner",
      },
    ],
    basepath_url: "https://basepath.example",
  },
  pathbase_get_graph: {
    items: [],
    relations: [],
    truncated: false,
    limit: 200,
  },
  pathbase_get_today: { local_date: "2026-09-18", items: [] },
  pathbase_get_week: {
    start: "2026-09-14",
    end: "2026-09-20",
    timezone: "Asia/Tokyo",
    days: [],
    unscheduled: [],
  },
  pathbase_list_changes: { items: [] },
};

/** A change set as the server returns it from a proposal. */
function proposal(overrides = {}) {
  return {
    id: "change_live",
    workspace_id: "personal",
    title: "朝の習慣を足す",
    status: "pending",
    hash: "digest-live",
    created_at: "2026-09-19T08:50:00Z",
    expires_at: "2099-01-01T00:00:00Z",
    proposed_by_connection: "mcpconn_b3578e78",
    approval_url: "https://basepath.example/changes/personal/change_live",
    auto_apply_eligible: false,
    changes: [
      {
        id: "i9",
        collection: "items",
        title: "朝の散歩",
        effect: "created",
        before: null,
        after: { title: "朝の散歩", kind: "action" },
        guarded_values: [],
      },
    ],
    ...overrides,
  };
}

test("a proposal is rendered as a diff the moment the model makes it", async ({
  page,
}) => {
  // The failure this replaces: the change set existed on the server, the
  // conversation showed a paragraph about it, and it expired unread.
  const app = await openHarness(page, { fixtures: QUIET });
  await expect(app.getByRole("region", { name: "変更案" })).toBeHidden();

  await page.evaluate((change) => window.__pushToolResult(change), proposal());

  const review = app.getByRole("region", { name: "変更案" });
  await expect(review).toBeVisible();
  await expect(
    review.getByRole("heading", { name: "朝の習慣を足す" }),
  ).toBeVisible();
  // Once as the row's title, once in the before/after table.
  await expect(review.getByText("朝の散歩").first()).toBeVisible();
  await expect(review.getByRole("row", { name: /タイトル/ })).toBeVisible();
  // No round trip was needed: the result that created it was the render.
  const calls = await page.evaluate(() => window.__calls);
  expect(calls).not.toContain("pathbase_get_change");
});

test("a proposal outside every range still leads somewhere", async ({
  page,
}) => {
  const app = await openHarness(page, { fixtures: QUIET });
  await page.evaluate((change) => window.__pushToolResult(change), proposal());

  const review = app.getByRole("region", { name: "変更案" });
  // No trigger, because nothing authorized one.
  await expect(
    review.getByRole("button", { name: "この内容を反映する" }),
  ).toBeHidden();
  // And no dead end: the way onward is named and the URL is readable even if
  // the host cannot open a link.
  await expect(
    review.getByRole("button", { name: "Basepathで承認する" }),
  ).toBeVisible();
  await expect(
    review.getByText("https://basepath.example/changes/personal/change_live"),
  ).toBeVisible();
});

test("a proposal inside a range the person set can be reflected from here", async ({
  page,
}) => {
  const applied = proposal({ status: "applied", auto_applied: true });
  const app = await openHarness(page, {
    fixtures: {
      ...QUIET,
      pathbase_apply_changes: {
        changeset: applied,
        results: [],
        auto_applied: true,
      },
    },
  });
  await page.evaluate(
    (change) => window.__pushToolResult(change),
    proposal({ auto_apply_eligible: true }),
  );

  const review = app.getByRole("region", { name: "変更案" });
  // The button is a trigger, and it says so: the decision was made earlier, in
  // Basepath, and this is not standing in for it.
  await expect(
    review.getByText("事前に決めた範囲に入っています", { exact: false }),
  ).toBeVisible();
  await review.getByRole("button", { name: "この内容を反映する" }).click();

  // What happened, where the button was. Not "sent" — reflected.
  await expect(
    app.getByText("事前に決めた範囲としてBasepathに反映しました", {
      exact: false,
    }),
  ).toBeVisible();
  await expect(app.getByText("自動反映済み")).toBeVisible();
  const calls = await page.evaluate(() => window.__calls);
  expect(calls).toContain("pathbase_apply_changes");
});

test("a refused reflection says the plan did not change, and where to go", async ({
  page,
}) => {
  const app = await openHarness(page, {
    fixtures: {
      ...QUIET,
      // The range was revoked between the proposal and the press. The server
      // re-reads it on every apply, which is the point.
      "pathbase_apply_changes:error": {
        code: "APPROVAL_REQUIRED",
        message: "画面での差分確認と承認が必要です",
        status: 403,
      },
    },
  });
  await page.evaluate(
    (change) => window.__pushToolResult(change),
    proposal({ auto_apply_eligible: true }),
  );

  const review = app.getByRole("region", { name: "変更案" });
  await review.getByRole("button", { name: "この内容を反映する" }).click();

  await expect(
    app.getByText("反映されていません", { exact: false }),
  ).toBeVisible();
  await expect(
    app
      .getByText("https://basepath.example/changes/personal/change_live", {
        exact: false,
      })
      .first(),
  ).toBeVisible();
});
