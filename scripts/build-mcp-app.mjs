/**
 * Builds the MCP App UI resource into one self-contained HTML file.
 *
 * The Lambda that serves `ui://` has no Node and no CDN, so the bundle is
 * committed at `api/ui/mcp-app.html` and embedded into the binary. Running this
 * script rewrites that file; CI runs it with `--check` and fails if the
 * committed file has drifted from the source.
 *
 * Inlining also settles the CSP question: the document loads nothing from
 * anywhere, so the resource asks for no external origins at all.
 */
import { build } from "vite";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const output = path.join(root, "api/ui/mcp-app.html");
/** A conversation pane should not wait on a megabyte of JavaScript. */
const SIZE_BUDGET_BYTES = 700 * 1024;

function escapeForScript(code) {
  // `</script>` inside a string literal would close the tag early.
  return code.replaceAll("</script>", "<\\/script>");
}

async function bundle() {
  await build({
    configFile: path.join(root, "vite.mcp-app.config.mjs"),
    logLevel: "warn",
  });
  const dist = path.join(root, "dist/mcp-app");
  const [html, js, css] = await Promise.all([
    readFile(path.join(dist, "index.html"), "utf8"),
    readFile(path.join(dist, "app.js"), "utf8"),
    readFile(path.join(dist, "app.css"), "utf8").catch(() => ""),
  ]);

  let inlined = html
    .replace(/<script[^>]*src="[^"]*app\.js"[^>]*><\/script>/, "")
    .replace(/<link[^>]*href="[^"]*app\.css"[^>]*>/, "");
  // Replacement *functions*, not strings: a bundle full of `$\`` and `$'`
  // sequences would otherwise have parts of the document spliced into it by
  // String.replace's dollar-sign substitution.
  if (css.trim()) {
    inlined = inlined.replace("</head>", () => `<style>${css}</style></head>`);
  }
  inlined = inlined.replace(
    "</body>",
    () => `<script type="module">${escapeForScript(js)}</script></body>`,
  );
  return `${inlined.trim()}\n`;
}

const built = await bundle();
if (Buffer.byteLength(built) > SIZE_BUDGET_BYTES) {
  console.error(
    `MCP App bundle is ${Buffer.byteLength(built)} bytes, over the ${SIZE_BUDGET_BYTES} byte budget.`,
  );
  process.exit(1);
}

if (process.argv.includes("--check")) {
  const committed = await readFile(output, "utf8").catch(() => "");
  if (committed !== built) {
    console.error(
      `api/ui/mcp-app.html is out of date. Run \`npm run build:mcp-app\` and commit the result.`,
    );
    process.exit(1);
  }
  console.log(`MCP App bundle is current (${Buffer.byteLength(built)} bytes).`);
} else {
  await mkdir(path.dirname(output), { recursive: true });
  await writeFile(output, built);
  console.log(`Wrote ${output} (${Buffer.byteLength(built)} bytes).`);
}
