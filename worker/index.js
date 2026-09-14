export default {
  async fetch(request, env) {
    const requestUrl = new URL(request.url);
    if (requestUrl.pathname === "/api" || requestUrl.pathname.startsWith("/api/")) {
      if (!env.PATHBASE_API_ORIGIN) {
        return Response.json({ code: "API_ORIGIN_MISSING", message: "PathBase API origin is not configured" }, { status: 503 });
      }
      const upstreamUrl = new URL(env.PATHBASE_API_ORIGIN);
      upstreamUrl.pathname = requestUrl.pathname.slice(4) || "/";
      upstreamUrl.search = requestUrl.search;
      return fetch(new Request(upstreamUrl, request));
    }

    const response = await env.ASSETS.fetch(request);
    const acceptsHtml = request.headers.get("accept")?.includes("text/html");

    if (response.status !== 404 || !acceptsHtml || !["GET", "HEAD"].includes(request.method)) {
      return response;
    }

    const indexUrl = new URL(request.url);
    indexUrl.pathname = "/index.html";
    indexUrl.search = "";
    return env.ASSETS.fetch(new Request(indexUrl, request));
  },
};
