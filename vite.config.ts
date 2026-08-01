import { readFileSync } from "node:fs";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const host = process.env.TAURI_DEV_HOST;

// 版本号只有一个来源：package.json。页脚以前硬写 `const APP_VERSION = "1.0.0"`
// 并附一句「和 package.json / tauri.conf.json 保持一致」——结果它一路显示 1.0.0
// 显示到 v1.4 发版。注进来之后就没得漂了；三份清单之间的一致性由
// `src/version.test.ts` 兜着。
const { version } = JSON.parse(
  readFileSync(new URL("./package.json", import.meta.url), "utf8"),
) as { version: string };

export default defineConfig(async () => ({
  plugins: [react()],
  clearScreen: false,
  define: {
    __APP_VERSION__: JSON.stringify(version),
  },
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? { protocol: "ws", host, port: 1421 }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));
