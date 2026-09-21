/**
 * Build the MCP Apps widget into the self-contained HTML resource embedded by
 * the Rust MCP server. The iframe loads no external code or assets.
 */
import { build } from "vite";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const output = path.join(root, "api/ui/mcp-app.html");
const sizeBudget = 700 * 1024;

function escapeForScript(code) {
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
  if (css.trim()) {
    inlined = inlined.replace(
      "</head>",
      () => `<style>${css}</style></head>`,
    );
  }
  inlined = inlined.replace(
    "</body>",
    () => `<script type="module">${escapeForScript(js)}</script></body>`,
  );
  return `${inlined.trim()}\n`;
}

const built = await bundle();
const bytes = Buffer.byteLength(built);
if (bytes > sizeBudget) {
  console.error(`MCP App bundle is ${bytes} bytes, over the ${sizeBudget} byte budget.`);
  process.exit(1);
}

if (process.argv.includes("--check")) {
  const committed = await readFile(output, "utf8").catch(() => "");
  if (committed !== built) {
    console.error(
      "api/ui/mcp-app.html is out of date. Run `npm run build:mcp-app`.",
    );
    process.exit(1);
  }
  console.log(`MCP App bundle is current (${bytes} bytes).`);
} else {
  await mkdir(path.dirname(output), { recursive: true });
  await writeFile(output, built);
  console.log(`Wrote ${output} (${bytes} bytes).`);
}
