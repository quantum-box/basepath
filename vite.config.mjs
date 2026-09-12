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
    host: "0.0.0.0",
    allowedHosts: ["terminal.local"],
    warmup: {
      clientFiles: ["./src/main.tsx"],
    },
    watch: { ignored: ["**/src-tauri/**"] },
  },
  plugins: [react()],
});
