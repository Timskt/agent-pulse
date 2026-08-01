/// <reference types="vite/client" />

/** 构建时注入的版本号，来源是 package.json（见 vite.config.ts 的 define） */
declare const __APP_VERSION__: string;
