/**
 * Headers API Gateway renames on the way out.
 *
 * A Lambda behind API Gateway cannot return `WWW-Authenticate` directly: the
 * integration rewrites it to `x-amzn-remapped-www-authenticate`. That header
 * is how an MCP client discovers where to get a token, so a response that
 * arrives without it looks to the client like a server that simply refuses,
 * with nothing to act on. The Rust API sets the real header; API Gateway
 * renames it; this puts it back before the client sees the response.
 */
const REMAPPED = [["x-amzn-remapped-www-authenticate", "www-authenticate"]];

/**
 * Paths that must be served from the origin root rather than under `/api`.
 *
 * OAuth discovery is defined by the URL a client already has: the protected
 * resource metadata for `https://host/api/mcp` lives at
 * `https://host/.well-known/oauth-protected-resource/api/mcp`, and the
 * authorization server metadata for issuer `https://host` lives at
 * `https://host/.well-known/oauth-authorization-server`. Neither can move, so
 * the path is forwarded unchanged and keeps describing the URLs the client
 * actually calls.
 */
const ROOT_PREFIXES = [
  "/.well-known/oauth-protected-resource",
  "/.well-known/oauth-authorization-server",
];

function restoreHeaders(response) {
  let restored = null;
  for (const [from, to] of REMAPPED) {
    const value = response.headers.get(from);
    if (value === null || response.headers.has(to)) continue;
    restored ??= new Response(response.body, response);
    restored.headers.set(to, value);
    restored.headers.delete(from);
  }
  return restored ?? response;
}

/**
 * Serves the single-page app for a URL that names no asset.
 *
 * `/index.html` is asked for first, and `/` second, because the asset layer
 * canonicalises one to the other: with html handling on, a request for
 * `/index.html` is itself answered with a redirect to `/`. Returning that
 * redirect is how the deep link was lost in the first place, so a redirect
 * here is followed rather than passed on.
 */
async function appShell(request, env) {
  for (const pathname of ["/index.html", "/"]) {
    const url = new URL(request.url);
    url.pathname = pathname;
    url.search = "";
    const response = await env.ASSETS.fetch(new Request(url, request));
    if (response.status < 300 || response.status >= 400) return response;
  }
  return new Response("Not found", { status: 404 });
}

export default {
  async fetch(request, env) {
    const requestUrl = new URL(request.url);
    const rootPath = ROOT_PREFIXES.some((prefix) =>
      requestUrl.pathname.startsWith(prefix),
    );
    if (
      rootPath ||
      requestUrl.pathname === "/api" ||
      requestUrl.pathname.startsWith("/api/")
    ) {
      if (!env.PATHBASE_API_ORIGIN) {
        return Response.json(
          {
            code: "API_ORIGIN_MISSING",
            message: "PathBase API origin is not configured",
          },
          { status: 503 },
        );
      }
      const upstreamUrl = new URL(env.PATHBASE_API_ORIGIN);
      upstreamUrl.pathname = rootPath
        ? requestUrl.pathname
        : requestUrl.pathname.slice(4) || "/";
      upstreamUrl.search = requestUrl.search;
      return restoreHeaders(await fetch(new Request(upstreamUrl, request)));
    }

    const response = await env.ASSETS.fetch(request);
    const acceptsHtml = request.headers.get("accept")?.includes("text/html");

    // "There is no such asset" does not always arrive as a 404. Asked for a
    // path it cannot serve, the asset binding answers `/oauth/authorize` with
    // `307 Location: /` — which, returned as-is, sends the browser to the home
    // screen and throws away the path *and the query string*. Every deep link
    // in this app carries its meaning there: which change set to approve,
    // which authorization request to consent to. So a redirect out of the
    // asset binding is treated as the miss it is, and the app shell is served
    // for the original URL instead.
    const missing =
      response.status === 404 ||
      (response.status >= 300 && response.status < 400);

    if (!missing || !acceptsHtml || !["GET", "HEAD"].includes(request.method)) {
      return response;
    }

    return appShell(request, env);
  },
};
