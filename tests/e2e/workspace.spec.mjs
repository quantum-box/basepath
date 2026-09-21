import { test, expect } from "@playwright/test";
import { openScreen } from "./navigate.mjs";

async function mockAuthenticatedTachyonApp(page, fieldFailure) {
  await page.route("**/api/**", async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    const respond = (status, value) =>
      route.fulfill({
        status,
        contentType: "application/json",
        body: JSON.stringify(value),
      });

    if (path === "/api/auth/status")
      return respond(200, {
        mode: "tachyon",
        configured: true,
        field_configured: true,
      });
    if (path === "/api/v1/me")
      return respond(200, {
        id: "user-1",
        name: "Test user",
        mode: "tachyon",
      });
    if (path === "/api/v1/settings")
      return respond(200, {
        compact: false,
        notifications: true,
        timezone: "Asia/Tokyo",
      });
    if (path === "/api/v1/workspaces")
      return respond(200, [
        {
          id: "personal",
          name: "Personal",
          scope: "個人",
          role: "owner",
          timezone: "Asia/Tokyo",
          local: false,
          version: 1,
        },
      ]);
    if (path === "/api/v1/workspaces/personal/snapshot")
      return respond(200, {
        workspace_id: "personal",
        items: [],
        relations: [],
        records: [],
        metrics: [],
        observations: [],
        views: [],
        changesets: [],
      });
    if (path === "/api/v1/invitations") return respond(200, []);
    if (path === "/api/v1/tenants")
      return respond(200, {
        tenants: [{ id: "tn_selected", name: "Selected tenant" }],
        selected_tenant_id: "tn_selected",
      });
    if (path === "/api/v1/integrations/field/tenants")
      return respond(fieldFailure.status, {
        code: fieldFailure.code,
        message: fieldFailure.message,
        details: null,
      });
    return respond(404, {
      code: "NOT_FOUND",
      message: "Not found",
      details: null,
    });
  });
}

test("tenant selection can return to the login screen", async ({ page }) => {
  let signedIn = true;
  let logoutRequests = 0;
  await page.route("**/api/**", async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    const respond = (status, value) =>
      route.fulfill({
        status,
        contentType: "application/json",
        body: JSON.stringify(value),
      });

    if (path === "/api/auth/status") {
      await respond(200, {
        mode: "tachyon",
        configured: true,
        field_configured: false,
      });
      return;
    }
    if (path === "/api/auth/logout" && request.method() === "POST") {
      signedIn = false;
      logoutRequests += 1;
      await respond(200, { signed_out: true });
      return;
    }
    if (path === "/api/v1/tenants" && signedIn) {
      await respond(200, {
        tenants: [{ id: "tn_example", name: "Example tenant" }],
        selected_tenant_id: null,
      });
      return;
    }
    await respond(signedIn ? 428 : 401, {
      code: signedIn ? "TENANT_SELECTION_REQUIRED" : "UNAUTHENTICATED",
      message: signedIn
        ? "利用するTachyonテナントを選択してください"
        : "ログインしてください",
      details: null,
    });
  });

  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "利用するテナント", exact: true }),
  ).toBeVisible();
  await expect(page).toHaveURL(/\/tenants\?tenant_id=tn_example$/);
  await page
    .getByRole("button", { name: "ログイン画面に戻る", exact: true })
    .click();

  await expect(
    page.getByLabel("Tachyonユーザー名またはメールアドレス"),
  ).toBeVisible();
  await expect(page).toHaveURL(/\/login$/);
  expect(logoutRequests).toBe(1);

  // A login render is not a context route; it must not erase the captured
  // return target while the person is authenticating.
  await page.goto(
    "/login?return_to=%2Fpersonal%2Fhome%3Ftenant_id%3Dtn_example",
  );
  await expect(page).toHaveURL(
    /\/login\?return_to=%2Fpersonal%2Fhome%3Ftenant_id%3Dtn_example/,
  );
});

test("forbidden tenant selection stays on the selection screen", async ({
  page,
}) => {
  await page.route("**/api/**", async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    const respond = (status, value) =>
      route.fulfill({
        status,
        contentType: "application/json",
        body: JSON.stringify(value),
      });
    if (path === "/api/auth/status")
      return respond(200, {
        mode: "tachyon",
        configured: true,
        field_configured: false,
      });
    if (path === "/api/v1/tenants")
      return respond(200, {
        tenants: [{ id: "tn_member", name: "Member tenant" }],
        selected_tenant_id: null,
      });
    if (path === "/api/v1/tenant-selection")
      return respond(403, {
        code: "FORBIDDEN",
        message: "このTachyonテナントを利用できません",
        details: null,
      });
    return respond(428, {
      code: "TENANT_SELECTION_REQUIRED",
      message: "利用するTachyonテナントを選択してください",
      details: null,
    });
  });

  await page.goto("/");
  await page
    .getByRole("button", { name: "このテナントで始める", exact: true })
    .click();

  await expect(page.getByRole("alert")).toContainText(
    "このTachyonテナントを利用できません",
  );
  await expect(
    page.getByRole("heading", { name: "利用するテナント", exact: true }),
  ).toBeVisible();
  await expect(page).toHaveURL(/\/tenants\?tenant_id=tn_member$/);
});

test("Tachyon home can open tenant selection and return", async ({ page }) => {
  let selectedTenant = "tn_first";
  let logoutRequests = 0;
  await page.route("**/api/**", async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    const respond = (status, value) =>
      route.fulfill({
        status,
        contentType: "application/json",
        body: JSON.stringify(value),
      });

    if (path === "/api/auth/status") {
      await respond(200, {
        mode: "tachyon",
        configured: true,
        field_configured: false,
      });
      return;
    }
    if (path === "/api/auth/logout") {
      logoutRequests += 1;
      await respond(200, { signed_out: true });
      return;
    }
    if (path === "/api/v1/me") {
      await respond(200, { id: "user-1", name: "Test user", mode: "tachyon" });
      return;
    }
    if (path === "/api/v1/settings") {
      await respond(200, {
        compact: false,
        notifications: true,
        timezone: "Asia/Tokyo",
      });
      return;
    }
    if (path === "/api/v1/workspaces") {
      await respond(200, [
        {
          id: "personal",
          name: "Personal",
          scope: "個人",
          role: "owner",
          timezone: "Asia/Tokyo",
          local: false,
          version: 1,
        },
      ]);
      return;
    }
    if (path === "/api/v1/workspaces/personal/snapshot") {
      await respond(200, {
        workspace_id: "personal",
        items: [],
        relations: [],
        records: [],
        metrics: [],
        observations: [],
        views: [],
        changesets: [],
      });
      return;
    }
    if (path === "/api/v1/invitations") {
      await respond(200, []);
      return;
    }
    if (path === "/api/v1/tenants" && request.method() === "GET") {
      await respond(200, {
        tenants: [
          { id: "tn_first", name: "First tenant" },
          { id: "tn_second", name: "Second tenant" },
        ],
        selected_tenant_id: selectedTenant,
      });
      return;
    }
    if (path === "/api/v1/tenant-selection" && request.method() === "POST") {
      selectedTenant = JSON.parse(request.postData()).tenant_id;
      await respond(200, {
        selected_tenant: { id: selectedTenant, name: "Second tenant" },
      });
      return;
    }
    await respond(404, {
      code: "NOT_FOUND",
      message: "Not found",
      details: null,
    });
  });

  await page.goto("/");
  const switcher = page.getByRole("button", {
    name: "テナント切替",
    exact: true,
  });
  await expect(switcher).toBeVisible();
  await expect(page).toHaveURL(/\/personal\/home\?tenant_id=tn_first$/);
  await switcher.click();
  await expect(
    page.getByRole("heading", { name: "利用するテナント", exact: true }),
  ).toBeVisible();
  await expect(page).toHaveURL(/\/tenants\?tenant_id=tn_first$/);
  await page.getByLabel("Tachyonテナント").selectOption("tn_second");
  const chooseTenant = page.getByRole("button", {
    name: "このテナントで始める",
    exact: true,
  });
  if (await chooseTenant.isVisible().catch(() => false))
    await chooseTenant.click();
  await expect(switcher).toBeVisible();
  await expect(page).toHaveURL(/\/personal\/home\?tenant_id=tn_second$/);
  expect(selectedTenant).toBe("tn_second");

  await page.reload();
  await expect(switcher).toBeVisible();
  await expect(page.getByText("Tachyonでログイン中")).toBeVisible();
  await expect(page).toHaveURL(/\/personal\/home\?tenant_id=tn_second$/);

  await switcher.click();
  await expect(page).toHaveURL(/\/tenants\?tenant_id=tn_second$/);
  await page.getByRole("button", { name: "ホームに戻る", exact: true }).click();
  await expect(switcher).toBeVisible();
  await expect(page).toHaveURL(/\/personal\/home\?tenant_id=tn_second$/);
  expect(logoutRequests).toBe(0);

  // A deep link may name a different tenant than the active session. If the
  // person cancels that selector, the candidate must not be sent back into
  // the gate; the server-selected tenant (or no tenant query) is safe.
  selectedTenant = "tn_first";
  await page.goto(
    "/tenants?tenant_id=tn_second&return_to=%2Fpersonal%2Fhome%3Ftenant_id%3Dtn_second",
  );
  await expect(
    page.getByRole("heading", { name: "利用するテナント", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "ホームに戻る", exact: true }).click();
  await expect(page).not.toHaveURL(/tenant_id=tn_second/);
  await expect(page.locator(".context-breadcrumb")).toContainText("個人");
});

for (const failure of [
  {
    status: 401,
    code: "FIELD_AUTH_REJECTED",
    message:
      "Fieldが現在のTachyon認証を受け付けませんでした。テナントとField権限を確認してください",
  },
  {
    status: 403,
    code: "FORBIDDEN",
    message: "Fieldのこの情報を閲覧する権限がありません",
  },
]) {
  test(`Field ${failure.status} keeps the PathBase session`, async ({
    page,
  }) => {
    await mockAuthenticatedTachyonApp(page, failure);
    await page.setViewportSize({ width: 1440, height: 1000 });
    await page.goto("/");
    await expect(page.getByText("Tachyonでログイン中")).toBeVisible();

    await page.getByRole("button", { name: "設定", exact: true }).click();
    await page
      .getByText("Fieldの営業タスク・成果指標", { exact: true })
      .click();
    await page
      .getByRole("button", { name: "アクセスできる組織を取得" })
      .click();

    await expect(page.getByRole("alert").first()).toContainText(
      failure.message,
    );
    await expect(page.getByText("Tachyonでログイン中")).toBeVisible();
    await expect(
      page.getByLabel("Tachyonユーザー名またはメールアドレス"),
    ).not.toBeVisible();
    await expect(page).toHaveURL(/\/personal\/home\?tenant_id=tn_selected$/);
  });
}

async function openApp(page) {
  await page.goto("/");
  // The URL names the context, and the app rewrites it as soon as it knows
  // which one it is in. Waiting for that is waiting for the app to be ready.
  await expect(page).toHaveURL(/\/(personal|org)\//);
}

/**
 * The workspace the app is currently showing.
 *
 * Read from the URL, which is where the context now lives: an organization
 * names its id in the path, and a person has exactly one of their own.
 */
async function currentWorkspaceId(page, request) {
  const match = /\/org\/([^/]+)/.exec(new URL(page.url()).pathname);
  if (match) return match[1];
  const workspaces = await (await request.get("/api/v1/workspaces")).json();
  return workspaces.find((workspace) => workspace.scope === "個人").id;
}

/** Crosses to another workspace through the top-level context switcher. */
async function switchTo(page, name) {
  const opener = page.getByRole("button", {
    name: "メニューを開く",
    exact: true,
  });
  if (await opener.isVisible().catch(() => false)) await opener.click();
  await page
    .getByRole("group", { name: "現在の場所" })
    .getByRole("button", { name: new RegExp(name) })
    .click();
}

test("empty workspace onboarding keeps a draft and saves only confirmed fields", async ({
  page,
  request,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openApp(page);
  await expect(
    page.getByRole("heading", { name: "最初の目標を、動ける形に" }),
  ).toBeVisible();
  const wizard = page.locator(".onboarding-wizard");

  await wizard.getByRole("button", { name: /個人/ }).click();
  await wizard.getByRole("button", { name: "次へ" }).click();
  await wizard
    .getByLabel("達成したいこと")
    .fill("週に一度、本を読む時間をつくる");
  await wizard.getByRole("button", { name: "次へ" }).click();
  await expect(wizard.getByLabel("目標タイトル 必須")).toHaveValue(
    "週に一度、本を読む時間をつくる",
  );
  await expect(wizard.getByText("設定しません")).toHaveCount(4);

  await wizard.getByRole("button", { name: "戻る" }).click();
  await expect(wizard.getByLabel("達成したいこと")).toHaveValue(
    "週に一度、本を読む時間をつくる",
  );
  await wizard.getByRole("button", { name: "次へ" }).click();
  await wizard.getByRole("button", { name: "この内容で作成" }).click();

  await expect(
    page.getByRole("heading", { name: "週に一度、本を読む時間をつくる" }),
  ).toBeVisible();
  const workspaceId = await currentWorkspaceId(page, request);
  const saved = await snapshot(request, workspaceId);
  const activeItems = saved.items.filter((item) => !item.archived_at);
  expect(activeItems).toHaveLength(1);
  expect(activeItems[0]).toMatchObject({
    title: "週に一度、本を読む時間をつくる",
    due_date: null,
    fields: { next_action_id: null },
  });
  expect(saved.metrics).toHaveLength(0);
});

async function snapshot(request, workspaceId) {
  const response = await request.get(
    `/api/v1/workspaces/${workspaceId}/snapshot`,
  );
  expect(response.ok()).toBeTruthy();
  return response.json();
}

async function createGoal(page, title, workspaceId) {
  const homeTemplate = page
    .locator("#templates")
    .getByRole("button", { name: /自由形式/ });
  let openedFullTemplateScreen = false;
  if (await homeTemplate.isVisible().catch(() => false)) {
    await homeTemplate.click();
  } else {
    // The home template chooser is onboarding content and disappears after
    // the workspace has its first saved item. The full template screen remains
    // the supported way to start another goal.
    await openScreen(page, "テンプレート");
    await page.getByRole("button", { name: /自由形式/ }).click();
    openedFullTemplateScreen = true;
  }
  const dialog = page.getByRole("dialog");
  await dialog.getByLabel("目標の名前").fill(title);
  if (workspaceId)
    await dialog.getByLabel("ワークスペース").selectOption(workspaceId);
  await dialog.getByRole("button", { name: "目標を作成", exact: true }).click();
  await expect(dialog).not.toBeVisible();
  if (openedFullTemplateScreen) {
    await openScreen(
      page,
      page.url().includes("/org/") ? "概要" : "ホーム",
      null,
    );
  }
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
  const workspaceId = await currentWorkspaceId(page, request);
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
  const workspaceId = await currentWorkspaceId(page, request);
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
  await openScreen(page, "ワークスペース", "メンバー");
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
  // Creating the second workspace moved the app into it, so this is now an
  // organization context — where the front page is called 概要.
  await page
    .getByRole("navigation", { name: "メインメニュー" })
    .getByRole("button", { name: "概要", exact: true })
    .click();
  const title = "E2E: 勉強会の目標";
  await createGoal(page, title);
  expect(
    (await snapshot(request, ids[1])).items.map((item) => item.title),
  ).toContain(title);
  expect((await snapshot(request, ids[0])).items).toHaveLength(0);

  // Each workspace is its own context with its own URL, so crossing between
  // them is the top-level switch rather than a dropdown inside one screen.
  await switchTo(page, "E2E: 読書会");
  await expect(page).toHaveURL(new RegExp(`/org/${ids[0]}/`));
  await switchTo(page, "E2E: 勉強会");
  await expect(page).toHaveURL(new RegExp(`/org/${ids[1]}/`));

  // And a reload stays where the URL says, rather than falling back to
  // whichever workspace answered first.
  await page.reload();
  await expect(page).toHaveURL(new RegExp(`/org/${ids[1]}/`));
  await page
    .getByRole("navigation", { name: "メインメニュー" })
    .getByRole("button", { name: "組織の目標", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: title, exact: true }),
  ).toBeVisible();
});

test("mobile navigation opens the workspace manager page and returns home", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openApp(page);
  await openScreen(page, "ワークスペース", "メンバー");
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
  await expect(
    page.getByRole("navigation", { name: "メインメニュー" }),
  ).toBeHidden();
  await expect(page.locator("main")).toBeFocused();
});

test("settings explains that an AI connection must be allowed first", async ({
  page,
}) => {
  await openApp(page);
  // The utility navigation can sit below the fold, so drive it the way the
  // sibling focus test does rather than scrolling.
  const settings = page
    .getByRole("navigation", { name: "ユーティリティ" })
    .getByRole("button", { name: "設定", exact: true });
  await settings.focus();
  await settings.press("Enter");
  const dialog = page.getByRole("dialog");
  await expect(
    dialog.getByRole("heading", { name: "AIクライアントの接続" }),
  ).toBeVisible();
  // The screen states the two things a person needs before deciding: an AI
  // client they have not allowed can read nothing, and allowing one still does
  // not let it change the plan on its own.
  await expect(
    dialog.getByText("許可していないAIクライアントは何も読み取れません", {
      exact: false,
    }),
  ).toBeVisible();
  await expect(
    dialog.getByText("あなたが差分を確認して承認するまで反映されません", {
      exact: false,
    }),
  ).toBeVisible();
});

test("closing settings returns keyboard focus to its trigger", async ({
  page,
}) => {
  await openApp(page);
  const settings = page
    .getByRole("navigation", { name: "ユーティリティ" })
    .getByRole("button", { name: "設定", exact: true });
  await settings.focus();
  await settings.press("Enter");
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toBeHidden();
  await expect(settings).toBeFocused();
});

test("weekly review saves, finalizes, and remains usable at 390px", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openApp(page);
  await openScreen(page, "振り返り", "振り返り");
  await expect(
    page.getByRole("heading", { name: "週次レビュー", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText("行動完了率とは別指標", { exact: true }),
  ).toBeVisible();
  await page
    .getByLabel("学び", { exact: true })
    .fill("E2E: 小さな完了を確認できた");
  await page.getByLabel("課題", { exact: true }).fill("E2E: 未計測を埋める");
  await page
    .getByLabel("次週の重点", { exact: true })
    .fill("E2E: 観測を続ける");
  await page.getByRole("button", { name: "下書き保存", exact: true }).click();
  await expect(page.getByText("下書き保存済み", { exact: true })).toBeVisible();
  await page
    .getByRole("button", { name: "レビューを確定", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: /確定済みレビュー/ }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "訂正版を作成", exact: true }),
  ).toBeVisible();
  expect(
    await page
      .locator(".weekly-review-screen")
      .evaluate((element) => element.scrollWidth <= element.clientWidth),
  ).toBe(true);
});
