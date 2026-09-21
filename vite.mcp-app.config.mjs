import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

/** Build the one self-contained MCP Apps plan surface. */
export default defineConfig({
  root: "mcp-app",
  build: {
    outDir: "../dist/mcp-app",
    emptyOutDir: true,
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
