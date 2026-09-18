// Minimal TypeScript loader for node:test.
//
// The shared modules are plain TypeScript with no JSX and no runtime-only
// syntax, so stripping the types is enough; this avoids adding a build step to
// run a unit test.
import { transform } from "esbuild";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

export async function load(url, context, nextLoad) {
  if (!url.endsWith(".ts")) return nextLoad(url, context);
  const source = await readFile(fileURLToPath(url), "utf8");
  const { code } = await transform(source, {
    loader: "ts",
    format: "esm",
    target: "node20",
  });
  return { format: "module", shortCircuit: true, source: code };
}

export function resolve(specifier, context, nextResolve) {
  if (specifier.startsWith(".") && !/\.[a-z]+$/.test(specifier)) {
    try {
      return nextResolve(`${specifier}.ts`, context);
    } catch {
      // fall through to the default resolution
    }
  }
  return nextResolve(specifier, context);
}
