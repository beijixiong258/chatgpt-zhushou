import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  root: "ui",
  plugins: [react()],
  build: {
    outDir: "../dist",
    emptyOutDir: true,
    license: { fileName: "ui-notices.md" },
    assetsInlineLimit: 0,
    modulePreload: false,
    rollupOptions: {
      output: { entryFileNames: "app.js", assetFileNames: "[name][extname]" },
    },
  },
});
