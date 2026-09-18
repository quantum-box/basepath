// Single source of truth for which Rust binary the local tooling runs.
//
// `api/Cargo.toml` builds two binaries from the same crate:
//   - `pathbase-api`        plain HTTP server, used by local dev, MCP stdio,
//                           preflight, and the browser tests
//   - `lambda-pathbase-api` AWS Lambda runtime entrypoint, used in production
//
// `cargo run` without `--bin` is ambiguous once a crate has more than one
// binary, so every caller names the binary explicitly.
export const API_BINARY = "pathbase-api";
export const LAMBDA_BINARY = "lambda-pathbase-api";
export const MANIFEST_PATH = "api/Cargo.toml";

/** Arguments for `cargo run` that start the plain HTTP API. */
export function cargoRunArgs(
  extra = [],
  { quiet = false, locked = false } = {},
) {
  const args = ["run"];
  if (quiet) args.push("--quiet");
  if (locked) args.push("--locked");
  args.push("--manifest-path", MANIFEST_PATH, "--bin", API_BINARY);
  if (extra.length) args.push("--", ...extra);
  return args;
}
