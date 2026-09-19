import { expect } from "@playwright/test";

/**
 * Moving around an app with two top-level contexts.
 *
 * A workspace is no longer a dropdown inside a screen: it is the context, with
 * its own URL and its own menu. So "show me this screen for that workspace" is
 * two steps — cross to the context, then pick the screen — and every spec does
 * it the same way from here.
 */

/** Opens the sidebar without navigating, so the context is not thrown away. */
export async function showMenu(page) {
  const opener = page.getByRole("button", {
    name: "メニューを開く",
    exact: true,
  });
  if (await opener.isVisible().catch(() => false)) await opener.click();
}

/** Crosses to a workspace by name, through the top-level switcher. */
export async function switchTo(page, name) {
  await showMenu(page);
  await page
    .getByRole("group", { name: "現在の場所" })
    .getByRole("button", { name: new RegExp(escapeForName(name)) })
    .click();
}

/** Opens a screen in the context the app is already in. */
export async function openScreen(page, label, heading = label) {
  await showMenu(page);
  const entry = page
    .getByRole("navigation", { name: "メインメニュー" })
    .getByRole("button", { name: label, exact: true });
  await entry.focus();
  await entry.press("Enter");
  await expect(
    page.getByRole("heading", { name: heading, exact: true, level: 1 }),
  ).toBeVisible();
}

/**
 * The whole move: load the app, cross to a workspace if one is named, then
 * open the screen.
 */
export async function open(page, { workspace, screen, heading } = {}) {
  await page.goto("/");
  if (workspace) await switchTo(page, workspace);
  if (screen) await openScreen(page, screen, heading);
}

/** Test workspace names carry a timestamp, not regex syntax — but be safe. */
function escapeForName(name) {
  return name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
