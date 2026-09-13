import { invoke, isTauri } from "@tauri-apps/api/core";
export type Recurrence = {
  mode: "period_quota" | "fixed_schedule";
  times_per_week: number;
  timezone: string;
  weekdays: number[];
};
export type Item = {
  id: string;
  workspace_id: string;
  kind: "idea" | "outcome" | "initiative" | "action" | "milestone";
  title: string;
  description: string;
  state: "draft" | "active" | "paused" | "done" | "abandoned";
  version: number;
  created_at: string;
  updated_at: string;
  archived_at: string | null;
  start_date: string | null;
  due_date: string | null;
  scheduled_date: string | null;
  scheduled_time: string | null;
  fields: {
    assignee_id?: string | null;
    priority?: "low" | "medium" | "high" | "urgent" | null;
    field_reference?: {
      tenant_id: string;
      platform_id: string;
      external_id: string;
      source_updated_at: string;
      source_status: string;
    };
    icon?: string;
    subtitle?: string;
    memo?: string;
    next_action_id?: string | null;
    self_assessment?: number | null;
    assessed_at?: string | null;
    template?: string | null;
    template_version?: number;
    recurrence?: Recurrence | null;
    external_url?: string | null;
  };
};
export type Notification = {
  id: string;
  workspace_id: string;
  recipient: string;
  item_id: string;
  kind:
    | "assignment"
    | "due_date"
    | "due_today"
    | "due_soon"
    | "overdue"
    | "completion";
  title: string;
  created_at: string;
  read_at: string | null;
};
export type Workspace = {
  id: string;
  name: string;
  scope: "個人" | "チーム" | "組織";
  role: "owner" | "editor" | "viewer";
  timezone: string;
  local: boolean;
  version: number;
};
export type Member = { actor: string; role: Workspace["role"] };
export type Invitation = {
  id: string;
  workspace_id: string;
  workspace_name: string;
  target_actor: string;
  role: "editor" | "viewer";
  status: "pending" | "accepted" | "declined" | "revoked" | "expired";
  created_by: string;
  created_at: string;
  expires_at: string;
  version: number;
};
export type Memberships = {
  workspace: Workspace;
  members: Member[];
  invitations: Invitation[];
};
export type Relation = {
  id: string;
  workspace_id: string;
  source_id: string;
  target_id: string;
  type: "part_of" | "contributes_to" | "depends_on" | "relates_to";
  rationale: string;
  version: number;
};
export type RecordEntry = {
  id: string;
  workspace_id: string;
  item_ids: string[];
  record_type: string;
  body: string;
  happened_at: string;
  created_at: string;
  author: string;
  occurrence_key?: string;
  supersedes_id?: string | null;
};
export type Metric = {
  id: string;
  workspace_id: string;
  item_id: string;
  name: string;
  unit: string;
  baseline: number;
  target: number;
  direction: "increase" | "decrease" | "threshold";
  period_start?: string;
  period_end?: string;
  version: number;
};
export type Observation = {
  id: string;
  workspace_id: string;
  metric_id: string;
  value: number;
  unit: string;
  source: string;
  observed_at: string;
  created_at: string;
  supersedes_id: string | null;
};
export type Operation = { method: string; path: string; body: unknown };
export type ChangeSet = {
  id: string;
  workspace_id: string;
  title: string;
  status: "pending" | "approved" | "applied";
  operations: Operation[];
  expires_at: string;
  created_at: string;
};
export type Snapshot = {
  workspace_id: string;
  items: Item[];
  relations: Relation[];
  records: RecordEntry[];
  metrics: Metric[];
  observations: Observation[];
  views: { id: string; type: string; name: string }[];
  changesets: ChangeSet[];
  notifications: Notification[];
};
export type Settings = {
  compact: boolean;
  notifications: boolean;
  timezone: string;
};
export class ApiError extends Error {
  constructor(
    public code: string,
    message: string,
    public details: unknown = null,
    public status: number = 0,
  ) {
    super(message);
  }
}
export const uiId = (item: { workspace_id: string; id: string }) =>
  `${item.workspace_id}~${item.id}`;
export const itemPath = (item: Item) =>
  `/v1/workspaces/${encodeURIComponent(item.workspace_id)}/items/${encodeURIComponent(item.id)}`;
export function localDate(timezone = "Asia/Tokyo", date = new Date()) {
  return new Intl.DateTimeFormat("sv-SE", {
    timeZone: timezone,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).format(date);
}
export function dateLabel(date?: string | null) {
  return date
    ? new Intl.DateTimeFormat("ja-JP", {
        month: "long",
        day: "numeric",
        weekday: "short",
      }).format(new Date(`${date}T12:00:00`))
    : "日付未定";
}
export async function request<T>(
  method: string,
  path: string,
  body?: unknown,
  key: string = crypto.randomUUID(),
): Promise<T> {
  if (
    isTauri() &&
    (location.protocol === "tauri:" ||
      location.hostname === "tauri.localhost" ||
      location.port === "1420")
  ) {
    try {
      return await invoke<T>("pathbase_request", {
        method,
        path,
        body: body ?? {},
        key,
      });
    } catch (e) {
      const error = e as { code?: string; message?: string; details?: unknown };
      throw new ApiError(
        error.code ?? "CONNECTION_ERROR",
        error.message ?? String(e),
        error.details,
        error.code === "NOT_FOUND"
          ? 404
          : error.code === "FORBIDDEN" || error.code === "OWNER_REQUIRED"
            ? 403
            : 0,
      );
    }
  }
  let response: Response;
  try {
    response = await fetch(`/api${path}`, {
      method,
      headers: {
        "Content-Type": "application/json",
        "X-PathBase-Request": "1",
        ...(method === "GET" ? {} : { "Idempotency-Key": key }),
      },
      body: method === "GET" ? undefined : JSON.stringify(body ?? {}),
      signal: AbortSignal.timeout(15000),
      cache: "no-store",
    });
  } catch {
    throw new ApiError(
      "CONNECTION_ERROR",
      "接続できません。入力は保持されています。接続を確認して再試行してください。",
    );
  }
  let value: unknown;
  try {
    value = await response.json();
  } catch {
    throw new ApiError(
      "CONNECTION_ERROR",
      "APIに接続できません。入力は保持されています。",
    );
  }
  if (!response.ok) {
    const e = value as { code: string; message: string; details: unknown };
    throw new ApiError(e.code, e.message, e.details, response.status);
  }
  return value as T;
}
