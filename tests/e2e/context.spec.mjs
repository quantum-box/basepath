import { expect, test } from "@playwright/test";

/**
 * Personal and organization, as two places rather than one filter.
 *
 * What these tests hold:
 *
 * 1. **You can tell where you are from text alone.** No colour, no icon —
 *    a label a screen reader reads out and a greyscale screen still shows.
 * 2. **The URL is the authority.** A reload, a deep link and the back button
 *    all land in the context the address names, not the last one on screen.
 * 3. **The menus are different trees.** Memory is not in an organization at
 *    all; alignment and members are not in a person's own plan.
 */
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

async function organization(request, name) {
  const created = await post(request, "/v1/workspaces", {
    name,
    scope: "組織",
  });
  return created.id;
}

function menu(page) {
  return page.getByRole("navigation", { name: "メインメニュー" });
}

/** Opens the sidebar without navigating, so the context is not thrown away. */
async function showMenu(page) {
  const opener = page.getByRole("button", {
    name: "メニューを開く",
    exact: true,
  });
  if (await opener.isVisible().catch(() => false)) await opener.click();
  await expect(menu(page)).toBeVisible();
}

async function openApp(page, path = "/") {
  await page.goto(path);
  await showMenu(page);
}

function places(page) {
  return page.getByRole("group", { name: "現在の場所" });
}

test("the screen says which of the two you are in, in words", async ({
  page,
  request,
}) => {
  const name = `E2E文脈${Date.now()}`;
  await organization(request, name);

  await openApp(page);
  await places(page).getByRole("button", { name: /個人/ }).click();
  // Not a colour and not an icon: the word, in the breadcrumb.
  await expect(page.locator(".context-breadcrumb")).toContainText("個人");
  await expect(page).toHaveURL(/\/personal\//);

  await showMenu(page);
  await places(page)
    .getByRole("button", { name: new RegExp(name) })
    .click();
  const breadcrumb = page.locator(".context-breadcrumb");
  await expect(breadcrumb).toContainText("組織");
  await expect(breadcrumb).toContainText(name);
  await expect(page).toHaveURL(/\/org\//);
});

test("the two have different menus, not one menu with rows hidden", async ({
  page,
  request,
}) => {
  const name = `E2Eメニュー${Date.now()}`;
  await organization(request, name);

  await openApp(page);
  await places(page).getByRole("button", { name: /個人/ }).click();
  await showMenu(page);
  // Memory lives only in a person's own workspace.
  await expect(menu(page).getByRole("button", { name: "記憶" })).toBeVisible();
  await expect(
    menu(page).getByRole("button", { name: "アラインメント" }),
  ).toHaveCount(0);
  // The workspace screen is in both, named for the question it answers here:
  // which organizations am I in, not who is in this one.
  await expect(
    menu(page).getByRole("button", { name: "ワークスペース" }),
  ).toBeVisible();
  await expect(
    menu(page).getByRole("button", { name: "メンバー", exact: true }),
  ).toHaveCount(0);

  await places(page)
    .getByRole("button", { name: new RegExp(name) })
    .click();
  await showMenu(page);
  await expect(
    menu(page).getByRole("button", { name: "アラインメント" }),
  ).toBeVisible();
  // An entry leading to an empty memory screen would suggest it could be here.
  await expect(menu(page).getByRole("button", { name: "記憶" })).toHaveCount(0);
});

test("a deep link lands in the context the URL names, after a reload", async ({
  page,
  request,
}) => {
  const name = `E2E直リンク${Date.now()}`;
  const id = await organization(request, name);

  await openApp(page, `/org/${id}/alignment`);
  await expect(page.locator(".context-breadcrumb")).toContainText(name);
  await expect(
    page.getByRole("heading", { name: "アラインメント", level: 1 }),
  ).toBeVisible();

  await page.reload();
  await expect(page.locator(".context-breadcrumb")).toContainText(name);
  await expect(page).toHaveURL(new RegExp(`/org/${id}/alignment`));

  await openApp(page, "/personal/memory");
  await expect(page.locator(".context-breadcrumb")).toContainText("個人");
  await expect(
    page.getByRole("heading", { name: "記憶", exact: true, level: 1 }),
  ).toBeVisible();
});

test("a link to a screen the context does not have lands on its home", async ({
  page,
  request,
}) => {
  const name = `E2E不在${Date.now()}`;
  const id = await organization(request, name);

  // /org/{id}/memory names something that is not there. An empty memory
  // screen would answer "is my memory in here?" with a maybe.
  await openApp(page, `/org/${id}/memory`);
  await expect(page.locator(".context-breadcrumb")).toContainText(name);
  await expect(
    page.getByRole("heading", { name: "記憶", exact: true, level: 1 }),
  ).toHaveCount(0);
  await expect(page).toHaveURL(new RegExp(`/org/${id}/home`));
});

test("the back button crosses back, menu and all", async ({
  page,
  request,
}) => {
  const name = `E2E戻る${Date.now()}`;
  const id = await organization(request, name);

  await openApp(page, "/personal/goals");
  await expect(page.locator(".context-breadcrumb")).toContainText("個人");
  await places(page)
    .getByRole("button", { name: new RegExp(name) })
    .click();
  await expect(page.locator(".context-breadcrumb")).toContainText(name);

  await page.goBack();
  // Back into the person's own Basepath: the label, the URL and the menu all
  // agree. A back button that moves the URL but not the menu is the failure.
  await expect(page.locator(".context-breadcrumb")).toContainText("個人");
  await expect(page).toHaveURL(new RegExp("/personal/"));
  await showMenu(page);
  await expect(menu(page).getByRole("button", { name: "記憶" })).toBeVisible();
  await expect(page).not.toHaveURL(new RegExp(`/org/${id}`));
});

test("switching contexts does not carry the search across", async ({
  page,
  request,
}) => {
  const name = `E2E持ち越し${Date.now()}`;
  await organization(request, name);

  await openApp(page, "/personal/goals");
  const search = page.getByLabel("目標やタスクを検索");
  await search.fill("持ち越されるはずのない語");
  await expect(search).toHaveValue("持ち越されるはずのない語");

  // The results panel sits over the sidebar while the box has focus.
  await search.press("Escape");
  await showMenu(page);
  await places(page)
    .getByRole("button", { name: new RegExp(name) })
    .click();
  // A search is a question about the context it was typed in.
  await expect(page.getByLabel("目標やタスクを検索")).toHaveValue("");
});
