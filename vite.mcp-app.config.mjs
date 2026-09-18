import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

/**
 * Build configuration for the MCP App UI resource.
 *
 * The output is inlined into a single HTML file by scripts/build-mcp-app.mjs.
 * One file means the resource needs no CSP allowance for external origins, and
 * the Lambda can serve it without a CDN or a second request.
 */
export default defineConfig({
  root: "mcp-app",
  build: {
    outDir: "../dist/mcp-app",
    emptyOutDir: true,
    // One chunk, no code splitting: the resource is a single document.
    cssCodeSplit: false,
    modulePreload: { polyfill: false },
    rollupOptions: {
      output: {
        inlineDynamicImports: true,
        entryFileNames: "app.js",
        assetFileNames: "app.[ext]",
      },
    },
  },
  plugins: [react()],
});
