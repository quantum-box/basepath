import assert from "node:assert/strict";
import { access } from "node:fs/promises";
import test from "node:test";
import worker from "../worker/index.js";

test("serves existing static assets without a fallback", async () => {
  const calls = [];
  const response = await worker.fetch(
    new Request("https://example.test/assets/app.js"),
    {
      ASSETS: {
        fetch: async (request) => {
          calls.push(new URL(request.url).pathname);
          return new Response("asset", { status: 200 });
        },
      },
    },
  );

  assert.equal(response.status, 200);
  assert.deepEqual(calls, ["/assets/app.js"]);
});

test("falls back to index.html for an unknown app route", async () => {
  const calls = [];
  const response = await worker.fetch(
    new Request("https://example.test/flow/step-two?source=share", {
      headers: { accept: "text/html" },
    }),
    {
      ASSETS: {
        fetch: async (request) => {
          const url = new URL(request.url);
          calls.push(url.pathname + url.search);
          return new Response(
            url.pathname === "/index.html" ? "app" : "missing",
            {
              status: url.pathname === "/index.html" ? 200 : 404,
            },
          );
        },
      },
    },
  );

  assert.equal(response.status, 200);
  assert.deepEqual(calls, ["/flow/step-two?source=share", "/index.html"]);
});

test("does not turn missing API or write requests into the app shell", async () => {
  for (const request of [
    new Request("https://example.test/flow", {
      method: "POST",
      headers: { accept: "text/html" },
    }),
  ]) {
    let calls = 0;
    const response = await worker.fetch(request, {
      ASSETS: {
        fetch: async () => {
          calls += 1;
          return new Response("missing", { status: 404 });
        },
      },
    });

    assert.equal(response.status, 404);
    assert.equal(calls, 1);
  }
});

test("proxies same-origin API requests to the Lambda origin without the /api prefix", async () => {
  const originalFetch = globalThis.fetch;
  let forwarded;
  globalThis.fetch = async (request) => {
    forwarded = request;
    return Response.json({ status: "ok" });
  };

  try {
    const response = await worker.fetch(
      new Request("https://pathbase-v2.txcloud.app/api/v1/items?limit=5", {
        method: "POST",
        headers: { cookie: "pathbase_session=test", "x-pathbase-request": "1" },
        body: "{}",
      }),
      { PATHBASE_API_ORIGIN: "https://pathbase-api.txcloud.app" },
    );

    assert.equal(response.status, 200);
    assert.equal(
      forwarded.url,
      "https://pathbase-api.txcloud.app/v1/items?limit=5",
    );
    assert.equal(forwarded.method, "POST");
    assert.equal(forwarded.headers.get("cookie"), "pathbase_session=test");
    assert.equal(await forwarded.text(), "{}");
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("returns a service error when the Lambda origin is missing", async () => {
  const response = await worker.fetch(
    new Request("https://example.test/api/health"),
    {},
  );
  assert.equal(response.status, 503);
  assert.equal((await response.json()).code, "API_ORIGIN_MISSING");
});

test("emits the files required by Sites packaging", async () => {
  await access(new URL("../dist/client/index.html", import.meta.url));
  await access(new URL("../dist/server/index.js", import.meta.url));
  await access(new URL("../dist/.openai/hosting.json", import.meta.url));
});

test("forwards OAuth protected resource metadata to the API unchanged", async () => {
  const originalFetch = globalThis.fetch;
  let forwarded;
  globalThis.fetch = async (request) => {
    forwarded = request;
    return new Response('{"resource":"x"}', { status: 200 });
  };
  try {
    const response = await worker.fetch(
      new Request(
        "https://pathbase-v2.example/.well-known/oauth-protected-resource/api/mcp",
      ),
      { PATHBASE_API_ORIGIN: "https://pathbase-api.example" },
    );

    assert.equal(response.status, 200);
    // The path is not rewritten: the API answers on the same well-known path
    // the client asked for, so the document keeps describing the real MCP URL.
    assert.equal(
      forwarded.url,
      "https://pathbase-api.example/.well-known/oauth-protected-resource/api/mcp",
    );
  } finally {
    globalThis.fetch = originalFetch;
  }
});
