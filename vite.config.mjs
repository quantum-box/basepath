import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  build: {
    outDir: "dist/client",
    rollupOptions: {
      output: { manualChunks: { "react-flow": ["@xyflow/react"] } },
    },
  },
  optimizeDeps: {
    include: ["react", "react-dom/client"],
  },
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
    allowedHosts: ["terminal.local"],
    warmup: {
      clientFiles: ["./src/main.tsx"],
    },
    watch: { ignored: ["**/src-tauri/**", "**/api/**", "**/data/**"] },
    proxy: {
      "/api": {
        target: `http://127.0.0.1:${process.env.PATHBASE_API_PORT || 1431}`,
        rewrite: (path) => path.replace(/^\/api/, ""),
        configure(proxy) {
          proxy.on("proxyReq", (proxyReq, req) => {
            // Prevent cross-origin requests from using the local owner credential.
            const origin = req.headers.origin;
            if (
              (origin && origin !== `http://${req.headers.host}`) ||
              req.headers["sec-fetch-site"] === "cross-site"
            ) {
              proxyReq.removeHeader("authorization");
              return;
            }
            // The Tachyon login endpoint validates the browser's exact Origin.
            // Keep it intact and never attach the local-preview credential.
            if (/\/auth\/login(?:\?|$)/.test(req.url || "")) {
              proxyReq.removeHeader("authorization");
              return;
            }
            proxyReq.removeHeader("origin");
            proxyReq.setHeader(
              "authorization",
              `Bearer ${process.env.PATHBASE_API_TOKEN || ""}`,
            );
          });
        },
      },
    },
  },
  plugins: [react()],
});
