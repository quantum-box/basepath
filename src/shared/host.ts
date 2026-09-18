/**
 * Where the data comes from.
 *
 * The same view components run in three places, and only this differs:
 *
 * - **MCP App** — the host proxies `tools/call` to the PathBase MCP server.
 * - **Web / Tauri** — the browser calls the same-origin HTTP API.
 * - **Fixture** — static data, so the UI can be built and tested before a host
 *   or a database exists.
 *
 * Nothing here decides what a person is allowed to do. Authorization is the
 * Rust service's, on every call, in every host.
 */
import {
  buildPlanView,
  emptyPlanView,
  localDateIn,
  weekBounds,
  type PlanView,
} from "./viewModel";
import { weeklyReviewFrom, type WeeklyReviewView } from "./weeklyView";

export type HostKind = "mcp" | "web" | "fixture";

export type HostCapabilities = {
  /** The host can proxy tool calls to the MCP server. */
  serverTools: boolean;
};

export interface PlanHost {
  readonly kind: HostKind;
  readonly capabilities: HostCapabilities;
  /** Calls a PathBase MCP tool and returns its structured result. */
  call(tool: string, args: Record<string, unknown>): Promise<unknown>;
}

export class HostError extends Error {
  constructor(
    message: string,
    readonly code: string,
    readonly status?: number,
  ) {
    super(message);
    this.name = "HostError";
  }
}

/** Reads the structured payload out of an MCP tool result. */
export function structuredResult(result: unknown): unknown {
  const value = result as
    | {
        isError?: boolean;
        structuredContent?: unknown;
        content?: { type?: string; text?: string }[];
      }
    | undefined;
  const structured = value?.structuredContent;
  if (value?.isError) {
    const failure = structured as
      { code?: string; message?: string; status?: number } | undefined;
    throw new HostError(
      failure?.message ?? "操作できませんでした",
      failure?.code ?? "UNKNOWN",
      failure?.status,
    );
  }
  if (structured !== undefined) return structured;
  // A host that only passes text back still has to be usable.
  const text = value?.content?.find((block) => block.type === "text")?.text;
  if (typeof text === "string") {
    try {
      return JSON.parse(text);
    } catch {
      return { text };
    }
  }
  return undefined;
}

/** Static data, for development and tests. */
export class FixtureHost implements PlanHost {
  readonly kind = "fixture" as const;
  readonly capabilities: HostCapabilities = { serverTools: true };
  constructor(private readonly responses: Record<string, unknown>) {}
  async call(tool: string): Promise<unknown> {
    if (!(tool in this.responses)) {
      throw new HostError(`no fixture for ${tool}`, "NOT_FOUND", 404);
    }
    return this.responses[tool];
  }
}

/** The MCP App host: every call is proxied by the AI host to the server. */
export class McpAppHost implements PlanHost {
  readonly kind = "mcp" as const;
  constructor(
    private readonly callServerTool: (params: {
      name: string;
      arguments: Record<string, unknown>;
    }) => Promise<unknown>,
    readonly capabilities: HostCapabilities,
  ) {}
  async call(tool: string, args: Record<string, unknown>): Promise<unknown> {
    if (!this.capabilities.serverTools) {
      throw new HostError(
        "このホストはツール呼び出しに対応していません",
        "HOST_UNSUPPORTED",
      );
    }
    return structuredResult(
      await this.callServerTool({ name: tool, arguments: args }),
    );
  }
}

/** Same-origin HTTP, used by the web app and the Tauri debug window. */
export class WebHost implements PlanHost {
  readonly kind = "web" as const;
  readonly capabilities: HostCapabilities = { serverTools: true };
  constructor(
    private readonly request: (
      method: string,
      path: string,
    ) => Promise<unknown>,
  ) {}
  async call(tool: string, args: Record<string, unknown>): Promise<unknown> {
    const workspace = String(args.workspace_id ?? "");
    const query = (params: Record<string, unknown>) => {
      const search = new URLSearchParams();
      for (const [key, value] of Object.entries(params)) {
        if (value !== undefined && value !== null)
          search.set(key, String(value));
      }
      const rendered = search.toString();
      return rendered ? `?${rendered}` : "";
    };
    switch (tool) {
      case "pathbase_get_context":
        return { workspaces: await this.request("GET", "/v1/workspaces") };
      case "pathbase_get_graph":
        return this.request(
          "GET",
          `/v1/workspaces/${workspace}/graph${query({ limit: args.limit })}`,
        );
      case "pathbase_get_today":
        return this.request(
          "GET",
          `/v1/workspaces/${workspace}/today${query({
            local_date: args.local_date,
          })}`,
        );
      case "pathbase_list_changes":
        return this.request("GET", `/v1/workspaces/${workspace}/changesets`);
      case "pathbase_get_change":
        return this.request(
          "GET",
          `/v1/workspaces/${workspace}/changesets/${String(args.preview_id ?? "")}`,
        );
      case "pathbase_get_weekly_review":
        return this.request(
          "GET",
          `/v1/workspaces/${workspace}/weekly-review${query({
            week_start: args.week_start,
          })}`,
        );
      case "pathbase_get_week":
        return this.request(
          "GET",
          `/v1/workspaces/${workspace}/calendar${query({
            start: args.start,
            end: args.end,
            timezone: args.timezone,
          })}`,
        );
      default:
        throw new HostError(`unsupported tool ${tool}`, "NOT_FOUND", 404);
    }
  }
}

/**
 * Loads everything one plan view needs.
 *
 * Failures are returned rather than thrown so the caller can render the
 * specific reason — an expired connection reads differently from an empty
 * workspace.
 */
export async function loadPlanView(
  host: PlanHost,
  options: {
    workspaceId?: string;
    localDate?: string;
    limit?: number;
    /** Also load the week around `localDate`. */
    includeWeek?: boolean;
  } = {},
): Promise<{ view: PlanView; error: HostError | null }> {
  try {
    const context = await host.call("pathbase_get_context", {});
    const view = buildPlanView({ context, workspaceId: options.workspaceId });
    const workspace = view.workspace;
    if (!workspace) return { view, error: null };
    // "Today" belongs to the workspace, not to the viewer's device.
    const localDate = options.localDate || localDateIn(workspace.timezone);
    const bounds = weekBounds(localDate);
    const [graph, today, week] = await Promise.all([
      host.call("pathbase_get_graph", {
        workspace_id: workspace.id,
        ...(options.limit ? { limit: String(options.limit) } : {}),
      }),
      host.call("pathbase_get_today", {
        workspace_id: workspace.id,
        local_date: localDate,
      }),
      options.includeWeek
        ? host.call("pathbase_get_week", {
            workspace_id: workspace.id,
            start: bounds.start,
            end: bounds.end,
            timezone: workspace.timezone,
          })
        : Promise.resolve(undefined),
    ]);
    return {
      view: buildPlanView({
        context,
        graph,
        today,
        week,
        workspaceId: workspace.id,
      }),
      error: null,
    };
  } catch (error) {
    return {
      view: emptyPlanView,
      error:
        error instanceof HostError
          ? error
          : new HostError(String(error), "UNKNOWN"),
    };
  }
}

/**
 * Loads one week's review.
 *
 * Separate from `loadPlanView` because it is a separate question: the plan is
 * what is intended, this is what happened. Failures are returned rather than
 * thrown so the caller can say which week could not be read, and an empty week
 * is a successful answer, not an error.
 */
export async function loadWeeklyReview(
  host: PlanHost,
  workspaceId: string,
  weekStart: string,
): Promise<{ review: WeeklyReviewView | null; error: HostError | null }> {
  try {
    const value = await host.call("pathbase_get_weekly_review", {
      workspace_id: workspaceId,
      week_start: weekStart,
    });
    return { review: weeklyReviewFrom(value), error: null };
  } catch (error) {
    return {
      review: null,
      error:
        error instanceof HostError
          ? error
          : new HostError(String(error), "UNKNOWN"),
    };
  }
}
