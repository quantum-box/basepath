import { expect, test } from "@playwright/test";

/**
 * The connection consent screen, on Basepath's own origin.
 *
 * This is where a delegation to an AI client is granted, and the only place it
 * can be: the decision reaches the server with the person's own session. The
 * tests drive it the way a host does — register a client, send the person to
 * the authorization endpoint, and read what comes back on the redirect.
 */
/**
 * Registers a client, optionally under its own name.
 *
 * The browser suite shares one database, so every connection registered as
 * "E2E AI host" lands in the same settings list. A test that needs to act on
 * *its* connection names it, rather than picking one of several identical rows
 * and hoping.
 */
async function registerClient(request, redirect, name = "E2E AI host") {
  const response = await request.post("/api/oauth/register", {
    headers: { "content-type": "application/json" },
    data: {
      client_name: name,
      redirect_uris: [redirect],
      token_endpoint_auth_method: "none",
    },
  });
  expect(response.status(), await response.text()).toBe(201);
  return (await response.json()).client_id;
}

/** A PKCE pair, computed the way a client does. */
async function pkce(page, seed) {
  return page.evaluate(async (value) => {
    // At least 43 characters, as RFC 7636 requires of a verifier.
    const verifier = `${value}-verifier-0123456789abcdefghijklmnopqrstuvwxyz`;
    const digest = await crypto.subtle.digest(
      "SHA-256",
      new TextEncoder().encode(verifier),
    );
    const challenge = btoa(String.fromCharCode(...new Uint8Array(digest)))
      .replace(/\+/g, "-")
      .replace(/\//g, "_")
      .replace(/=+$/, "");
    return { verifier, challenge };
  }, seed);
}

function authorizeUrl(clientId, redirect, challenge, scope) {
  const query = new URLSearchParams({
    client_id: clientId,
    redirect_uri: redirect,
    response_type: "code",
    code_challenge: challenge,
    code_challenge_method: "S256",
    scope,
    state: "e2e-state",
  });
  return `/oauth/authorize?${query}`;
}

/** The host's callback, which only has to exist for the browser to land on. */
const REDIRECT = "http://127.0.0.1:1425/e2e-oauth-callback";

test("the screen names the client and what it is asking for", async ({
  page,
  request,
}) => {
  const clientId = await registerClient(request, REDIRECT);
  await page.goto("/");
  const { challenge } = await pkce(page, "describe");
  await page.goto(
    authorizeUrl(
      clientId,
      REDIRECT,
      challenge,
      "pathbase.read pathbase.propose",
    ),
  );

  await expect(
    page.getByRole("heading", { name: /E2E AI host を許可しますか/ }),
  ).toBeVisible();
  await expect(page.getByText("目標と行動を読む")).toBeVisible();
  await expect(page.getByText("変更案を作る")).toBeVisible();
  // A scope that was not requested must not appear as something to grant.
  await expect(page.getByText("承認済みの変更を反映する")).toBeHidden();
  // The screen says what granting does not do.
  await expect(
    page.getByText("実際に反映されるのは", { exact: false }),
  ).toBeVisible();
});

test("granting returns a code, and it exchanges for a scoped token", async ({
  page,
  request,
}) => {
  const clientId = await registerClient(request, REDIRECT);
  await page.goto("/");
  const { verifier, challenge } = await pkce(page, "grant");
  await page.goto(
    authorizeUrl(
      clientId,
      REDIRECT,
      challenge,
      "pathbase.read pathbase.propose",
    ),
  );

  // The person narrows what was asked for.
  await page.getByRole("checkbox", { name: /変更案を作る/ }).uncheck();
  await page.getByRole("button", { name: "許可する", exact: true }).click();

  await page.waitForURL((url) => url.pathname === "/e2e-oauth-callback");
  const returned = new URL(page.url());
  const code = returned.searchParams.get("code");
  expect(code).toBeTruthy();
  expect(returned.searchParams.get("state")).toBe("e2e-state");
  // RFC 9207: the client can tell which authorization server answered.
  expect(returned.searchParams.get("iss")).toBe("http://127.0.0.1:1425");

  const tokens = await request.post("/api/oauth/token", {
    form: {
      grant_type: "authorization_code",
      code,
      code_verifier: verifier,
      redirect_uri: REDIRECT,
      client_id: clientId,
    },
  });
  expect(tokens.ok(), await tokens.text()).toBeTruthy();
  const issued = await tokens.json();
  // Only what the person left checked.
  expect(issued.scope).toBe("pathbase.read");
  expect(issued.token_type).toBe("Bearer");
  expect(issued.refresh_token).toBeTruthy();
  expect(tokens.headers()["cache-control"]).toBe("no-store");

  // The same code cannot be redeemed twice.
  const replay = await request.post("/api/oauth/token", {
    form: {
      grant_type: "authorization_code",
      code,
      code_verifier: verifier,
      redirect_uri: REDIRECT,
      client_id: clientId,
    },
  });
  expect(replay.status()).toBe(400);
  expect((await replay.json()).error).toBe("invalid_grant");
});

test("declining sends the client away empty-handed", async ({
  page,
  request,
}) => {
  const clientId = await registerClient(request, REDIRECT);
  await page.goto("/");
  const { challenge } = await pkce(page, "decline");
  await page.goto(authorizeUrl(clientId, REDIRECT, challenge, "pathbase.read"));
  await page.getByRole("button", { name: "許可しない", exact: true }).click();

  await page.waitForURL((url) => url.pathname === "/e2e-oauth-callback");
  const returned = new URL(page.url());
  expect(returned.searchParams.get("error")).toBe("access_denied");
  expect(returned.searchParams.get("code")).toBeNull();
  expect(returned.searchParams.get("state")).toBe("e2e-state");
});

test("an expired or unknown request says so instead of granting anything", async ({
  page,
}) => {
  await page.goto("/oauth/authorize?client_id=mcpclient_nope&redirect_uri=x");
  await expect(
    page.getByRole("heading", { name: "接続できません" }),
  ).toBeVisible();
  await expect(page.getByRole("alert")).toBeVisible();
  // There is no way forward from here except back into Basepath.
  await expect(
    page.getByRole("button", { name: "許可する", exact: true }),
  ).toBeHidden();
});

test("discovery tells a host everything it needs before it connects", async ({
  request,
}) => {
  const protectedResource = await request.get(
    "/api/.well-known/oauth-protected-resource/api/mcp",
  );
  expect(protectedResource.ok()).toBeTruthy();
  const resource = await protectedResource.json();
  expect(resource.resource).toBe("http://127.0.0.1:1425/api/mcp");
  expect(resource.authorization_servers).toEqual(["http://127.0.0.1:1425"]);

  const server = await request.get(
    "/api/.well-known/oauth-authorization-server",
  );
  expect(server.ok()).toBeTruthy();
  const metadata = await server.json();
  // The two fields whose absence makes an authorization server unusable for
  // MCP: PKCE, and a way for a host to register a callback of its own.
  expect(metadata.code_challenge_methods_supported).toEqual(["S256"]);
  expect(metadata.registration_endpoint).toBe(
    "http://127.0.0.1:1425/api/oauth/register",
  );
  expect(metadata.token_endpoint_auth_methods_supported).toEqual(["none"]);
});

test("the consent screen is usable on a phone", async ({ page, request }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const clientId = await registerClient(request, REDIRECT);
  await page.goto("/");
  const { challenge } = await pkce(page, "narrow-screen");
  await page.goto(authorizeUrl(clientId, REDIRECT, challenge, "pathbase.read"));
  await expect(
    page.getByRole("button", { name: "許可する", exact: true }),
  ).toBeVisible();
  expect(
    await page
      .locator(".consent-screen")
      .evaluate((element) => element.scrollWidth <= element.clientWidth),
  ).toBe(true);
});

test("a granted connection is listed in settings and can be disconnected there", async ({
  page,
  request,
}) => {
  const clientId = await registerClient(request, REDIRECT);
  await page.goto("/");
  const { challenge } = await pkce(page, "settings");
  await page.goto(authorizeUrl(clientId, REDIRECT, challenge, "pathbase.read"));
  await page.getByRole("button", { name: "許可する", exact: true }).click();
  await page.waitForURL((url) => url.pathname === "/e2e-oauth-callback");

  await page.goto("/");
  const settings = page
    .getByRole("navigation", { name: "ユーティリティ" })
    .getByRole("button", { name: "設定", exact: true });
  await settings.focus();
  await settings.press("Enter");
  // Scoped to the connections list: the same panel now also lists this client
  // under the ranges below it, and that is a different question.
  const dialog = page.getByRole("dialog");
  const entry = dialog
    .locator(".mcp-connections > ul > li", { hasText: "E2E AI host" })
    .last();
  await expect(entry).toBeVisible();
  await expect(entry.getByText("接続中", { exact: false })).toBeVisible();
  // Only the scope the person granted is checked.
  await expect(
    entry.getByRole("checkbox", { name: /目標と行動を読む/ }),
  ).toBeChecked();
  await expect(
    entry.getByRole("checkbox", { name: /変更案を作る/ }),
  ).not.toBeChecked();

  await entry.getByRole("button", { name: "接続を解除", exact: true }).click();
  await expect(entry.getByText("解除済みです", { exact: false })).toBeVisible();
});

test("a person can set, narrow and remove a range that skips per-change approval", async ({
  page,
  request,
}) => {
  // The point of this test is where the decision is made. Every apply that
  // later proceeds without a per-change approval proceeds because of a row
  // written on this screen, with this session — and for no other reason.
  const clientId = await registerClient(request, REDIRECT, "E2E 範囲テスト");
  await page.goto("/");
  const { challenge } = await pkce(page, "range");
  await page.goto(
    authorizeUrl(
      clientId,
      REDIRECT,
      challenge,
      "pathbase.read pathbase.propose pathbase.apply",
    ),
  );
  await page.getByRole("button", { name: "許可する", exact: true }).click();
  await page.waitForURL((url) => url.pathname === "/e2e-oauth-callback");

  await page.goto("/");
  const settings = page
    .getByRole("navigation", { name: "ユーティリティ" })
    .getByRole("button", { name: "設定", exact: true });
  await settings.focus();
  await settings.press("Enter");

  const ranges = page.getByRole("dialog").locator(".auto-apply");
  await expect(ranges).toBeVisible();
  // The edges are stated while the permission is being granted, not in a
  // document somewhere else.
  await expect(
    ranges.getByText("削除は、どの設定でも自動反映されません", {
      exact: false,
    }),
  ).toBeVisible();

  const entry = ranges.locator("ul > li", { hasText: "E2E 範囲テスト" }).last();
  await expect(entry.getByText("設定なし", { exact: false })).toBeVisible();
  // Adding is offered by default; rewriting what the person wrote, and the
  // values that read afterwards as their decisions, are not.
  await expect(
    entry.getByRole("checkbox", { name: /追加を自動で反映する/ }),
  ).toBeChecked();
  await expect(
    entry.getByRole("checkbox", { name: /更新も自動で反映する/ }),
  ).not.toBeChecked();
  await expect(
    entry.getByRole("checkbox", { name: /期限・担当・目標値なども含める/ }),
  ).not.toBeChecked();

  await entry.getByRole("button", { name: "この範囲で許可する" }).click();
  const granted = entry.locator(".auto-apply-current > li");
  await expect(granted).toHaveCount(1);
  await expect(granted.getByText("追加", { exact: false })).toBeVisible();

  // Nothing is indefinite, and removing it is one press.
  await expect(
    granted.getByRole("button", { name: "この範囲を解除" }),
  ).toBeVisible();
  await granted.getByRole("button", { name: "この範囲を解除" }).click();
  await expect(entry.getByText("設定なし", { exact: false })).toBeVisible();
});

test("a second workspace's range is visible and revocable, not hidden behind the first", async ({
  page,
  request,
}) => {
  // Ranges are keyed by workspace, so one connection can hold several. The
  // screen used to collapse them to one, which left a live permission nobody
  // could see — and therefore nobody could take back.
  const second = await request.post("/api/v1/workspaces", {
    headers: {
      "idempotency-key": `e2e-range-ws-${Date.now()}`,
      "content-type": "application/json",
    },
    data: { name: `E2E範囲B-${Date.now()}`, scope: "チーム" },
  });
  expect(second.ok(), await second.text()).toBeTruthy();
  const secondId = (await second.json()).id;

  const clientId = await registerClient(request, REDIRECT, "E2E 二つの範囲");
  await page.goto("/");
  const { challenge } = await pkce(page, "two-ranges");
  await page.goto(
    authorizeUrl(
      clientId,
      REDIRECT,
      challenge,
      "pathbase.read pathbase.propose pathbase.apply",
    ),
  );
  await page.getByRole("button", { name: "許可する", exact: true }).click();
  await page.waitForURL((url) => url.pathname === "/e2e-oauth-callback");

  await page.goto("/");
  const settings = page
    .getByRole("navigation", { name: "ユーティリティ" })
    .getByRole("button", { name: "設定", exact: true });
  await settings.focus();
  await settings.press("Enter");

  const ranges = page.getByRole("dialog").locator(".auto-apply");
  const entry = ranges.locator("ul > li", { hasText: "E2E 二つの範囲" }).last();
  const picker = entry.getByLabel("ワークスペース");
  // The save control specifically: the granted ranges above it each carry a
  // button of their own.
  const save = entry.locator(".auto-apply-actions button");
  const live = entry.locator(".auto-apply-current > li");

  const first = await picker.locator("option").first().getAttribute("value");
  await picker.selectOption(first);
  await save.click();
  await expect(live).toHaveCount(1);

  await picker.selectOption(secondId);
  await save.click();
  // Both are shown, and each has its own way off.
  await expect(live).toHaveCount(2);
  await expect(
    live.getByRole("button", { name: "この範囲を解除" }),
  ).toHaveCount(2);

  await live.first().getByRole("button", { name: "この範囲を解除" }).click();
  await expect(live).toHaveCount(1);
});
