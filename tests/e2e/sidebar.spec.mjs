import { expect, test } from "@playwright/test";

/**
 * The sidebar in a window shorter than its contents.
 *
 * Two lists live there — the places a person can be, and the screens inside
 * the one they are in — and both grow: an organization each time someone is
 * added to one, a screen whenever a context gains a capability. Once they
 * scrolled separately, each shrank against the other, so a list was cut off
 * mid-entry while there was still room below it, and a person had to find a
 * second scrollbar to reach an entry that was sitting right there.
 *
 * What these tests hold: one scroll, everything reachable, and the way out of
 * the sidebar always on screen.
 *
 * The window is made short rather than the account given many organizations.
 * It is the same overflow either way, and this suite shares its database with
 * every other one in the run — workspaces left behind here would turn up in
 * somebody else's assertion about how many exist.
 */
const SHORT = { width: 1280, height: 420 };

function places(page) {
  return page.getByRole("group", { name: "現在の場所" });
}

function menu(page) {
  return page.getByRole("navigation", { name: "メインメニュー" });
}

async function openShort(page) {
  await page.setViewportSize(SHORT);
  await page.goto("/");
  await expect(menu(page)).toBeVisible();
}

test("the places are not a scroll area of their own", async ({ page }) => {
  await openShort(page);

  // Every place at its full height: the switcher hands its overflow to the
  // one scroller around it rather than clipping the list inside itself.
  const clipped = await places(page).evaluate(
    (el) => el.scrollHeight - el.clientHeight,
  );
  expect(clipped).toBeLessThanOrEqual(1);
});

test("the last screen in the menu is reachable in a short window", async ({
  page,
}) => {
  await openShort(page);

  const last = menu(page).getByRole("button").last();
  await last.scrollIntoViewIfNeeded();
  await expect(last).toBeInViewport({ ratio: 1 });
  await last.click();
  await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
});

test("the first place is reachable again after scrolling to the menu", async ({
  page,
}) => {
  await openShort(page);

  await menu(page).getByRole("button").last().scrollIntoViewIfNeeded();
  const personal = places(page).getByRole("button").first();
  await personal.scrollIntoViewIfNeeded();
  await expect(personal).toBeInViewport({ ratio: 1 });
});

test("the utility navigation stays on screen however long the lists get", async ({
  page,
}) => {
  await openShort(page);

  // It is the way out of the sidebar — notifications, search, settings.
  // Pushed below the fold by a long list of places, it is gone.
  const utility = page.getByRole("navigation", { name: "ユーティリティ" });
  await expect(utility).toBeInViewport({ ratio: 1 });
  await menu(page).getByRole("button").last().scrollIntoViewIfNeeded();
  await expect(utility).toBeInViewport({ ratio: 1 });
});
