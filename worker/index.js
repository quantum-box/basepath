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

    if (
      response.status !== 404 ||
      !acceptsHtml ||
      !["GET", "HEAD"].includes(request.method)
    ) {
      return response;
    }

    const indexUrl = new URL(request.url);
    indexUrl.pathname = "/index.html";
    indexUrl.search = "";
    return env.ASSETS.fetch(new Request(indexUrl, request));
  },
};
