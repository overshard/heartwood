import { resolve } from "path";
import { defineConfig } from "vite";

// Vite output goes to ../dist; Rust serves it at /static/.
// Single entry point, no per-page scripts to split.
export default defineConfig({
  base: "/static/",
  publicDir: resolve(__dirname, "static_src/public"),
  build: {
    outDir: resolve(__dirname, "../dist"),
    emptyOutDir: true,
    manifest: true,
    rollupOptions: {
      input: {
        index: resolve(__dirname, "static_src/index.js"),
      },
      output: {
        entryFileNames: "assets/[name]-[hash].js",
        chunkFileNames: "assets/[name]-[hash].js",
        assetFileNames: "assets/[name]-[hash][extname]",
      },
    },
  },
  css: {
    preprocessorOptions: {
      scss: {
        quietDeps: true,
      },
    },
  },
});
