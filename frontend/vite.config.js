import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const __dirname = dirname(fileURLToPath(import.meta.url));
const appVersion = readFileSync(resolve(__dirname, "../../VERSION"), "utf8").trim();

export default defineConfig({
  plugins: [react()],
  define: {
    __APP_VERSION__: JSON.stringify(appVersion),
  },
  server: {
    port: 5173,
  },
  // 产物到仓库 `hsr-gacha-web/dist/`，与 Rust 静态挂载路径一致（不再使用 frontend/dist）
  build: {
    outDir: "../dist",
    emptyOutDir: true,
  },
});
