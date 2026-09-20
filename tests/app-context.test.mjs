// Which of the two the person is in.
//
// Personal and organization are separate stores with a security boundary
// between them. The interface has to say so in words, in the URL, and in the
// menu — not in a colour, and not as a filter. These are the rules that make
// that true, tested without a browser.
import assert from "node:assert/strict";
import test from "node:test";
import { register } from "node:module";

register("./ts-loader.mjs", import.meta.url);

const {
  PERSONAL_NAV,
  ORGANIZATION_NAV,
  clearedOnSwitch,
  contextLabel,
  contextOf,
  emptyStateFor,
  landingFor,
  navFor,
  parsePath,
  pathFor,
  screenBelongs,
  stillAvailable,
  tenantReturnWithSelection,
} = await import("../src/shared/appContext.ts");

const personal = contextOf({ id: "ws_me", name: "個人", scope: "個人" });
const org = contextOf({ id: "ws_acme", name: "Acme", scope: "組織" });
const team = contextOf({ id: "ws_team", name: "開発", scope: "チーム" });

test("a workspace's scope decides the context, and a team is an organization", () => {
  assert.equal(personal.kind, "personal");
  assert.equal(org.kind, "organization");
  // A team workspace is somebody else's too. It is on the organization side
  // of the boundary, not a third thing with its own rules.
  assert.equal(team.kind, "organization");
});

test("the two have different menus, not one menu filtered", () => {
  const personalScreens = PERSONAL_NAV.map((item) => item.screen);
  const orgScreens = ORGANIZATION_NAV.map((item) => item.screen);

  // Memory exists only in a person's own workspace. An entry leading to an
  // empty one in an organization would suggest it could be there.
  assert.ok(personalScreens.includes("memory"));
  assert.ok(!orgScreens.includes("memory"));

  // Alignment, the dashboard and goal review are questions about an
  // organization. Offering them on a personal plan would imply it rolls up
  // into one.
  for (const screen of ["alignment", "dashboard", "goal-review"]) {
    assert.ok(orgScreens.includes(screen), `organization: ${screen}`);
    assert.ok(!personalScreens.includes(screen), `personal: ${screen}`);
  }

  // The workspace screen is in both, under different names, because it is
  // also the way out of a personal-only Basepath into an organization.
  assert.ok(personalScreens.includes("members"));
  assert.notEqual(
    PERSONAL_NAV.find((item) => item.screen === "members").label,
    ORGANIZATION_NAV.find((item) => item.screen === "members").label,
  );

  // Neither is a subset of the other — which is what makes it two trees
  // rather than one with rows hidden.
  assert.ok(personalScreens.some((screen) => !orgScreens.includes(screen)));
  assert.ok(orgScreens.some((screen) => !personalScreens.includes(screen)));
});

test("the URL says which context it is, and an organization's names itself", () => {
  assert.equal(pathFor(personal, "goals"), "/personal/goals");
  assert.equal(pathFor(org, "goals"), "/org/ws_acme/goals");
  // Two organizations are two URLs, so a reload or a back button cannot land
  // in the wrong one.
  assert.notEqual(pathFor(org, "goals"), pathFor(team, "goals"));
});

test("a context route reads back exactly as it was written", () => {
  for (const context of [personal, org, team]) {
    for (const item of navFor(context.kind)) {
      const parsed = parsePath(pathFor(context, item.screen));
      assert.equal(parsed.kind, context.kind, item.screen);
      assert.equal(parsed.screen, item.screen);
      if (context.kind === "organization") {
        assert.equal(parsed.orgId, context.workspaceId);
      }
    }
  }
});

test("a path that is not a context route is null, never a default", () => {
  // A default would land someone in a context they did not ask for, which is
  // the one thing this must not do on a reload of an unknown URL.
  for (const path of [
    "/",
    "/login",
    "/tenants",
    "/changes/a/b",
    "/org",
    "/personal/nonsense",
    "/org/ws_acme/nonsense",
  ]) {
    assert.equal(parsePath(path), null, path);
  }
  // A bare context is its home screen.
  assert.equal(parsePath("/personal").screen, "home");
  assert.equal(parsePath("/org/ws_acme").screen, "home");
});

test("a deep link to a screen the context does not have is refused", () => {
  // /org/{id}/memory is a link to something that is not there. Rendering an
  // empty memory screen would answer the question wrongly.
  assert.equal(screenBelongs("organization", "memory"), false);
  assert.equal(screenBelongs("personal", "memory"), true);
  assert.equal(screenBelongs("personal", "alignment"), false);
  // And the landing is home, not the nearest equivalent: guessing would carry
  // the person's place across the boundary.
  assert.equal(landingFor(), "home");
});

test("tenant selection replaces stale tenant context in a return URL", () => {
  assert.equal(
    tenantReturnWithSelection(
      "/org/ws_acme/goals?tenant_id=tenant_a&item=ws_acme~goal_1",
      "tenant_b",
    ),
    "/org/ws_acme/goals?tenant_id=tenant_b&item=ws_acme~goal_1",
  );
});

test("nothing is carried across a switch", () => {
  const cleared = clearedOnSwitch();
  assert.equal(cleared.selected, "");
  assert.equal(cleared.search, "");
  assert.equal(cleared.modal, null);
});

test("a context the person is no longer in stops being available", () => {
  const mine = [
    { id: "ws_me", name: "個人", scope: "個人" },
    { id: "ws_acme", name: "Acme", scope: "組織" },
  ];
  assert.equal(stillAvailable(org, mine), true);
  // Removed from the organization: the workspace list is the authority, and
  // the context stops existing at once rather than at the next reload.
  assert.equal(
    stillAvailable(org, [{ id: "ws_me", name: "個人", scope: "個人" }]),
    false,
  );
});

test("where you are is readable as text, not only as a colour", () => {
  assert.equal(contextLabel("personal"), "個人");
  assert.equal(contextLabel("organization"), "組織");
  // An empty personal plan and an empty organization plan mean different
  // things, and that is where someone is most likely to lose track of which
  // one they are looking at.
  assert.notEqual(emptyStateFor("personal"), emptyStateFor("organization"));
  assert.match(emptyStateFor("personal"), /個人/);
  assert.match(emptyStateFor("organization"), /組織/);
});
