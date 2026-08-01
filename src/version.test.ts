import { describe, expect, it } from "vitest";
import pkg from "../package.json";
import tauriConf from "../src-tauri/tauri.conf.json";
// `?raw` 是 Vite 的原样导入，省掉为了读一个文件而引 @types/node——
// 那个包会把 setTimeout 的返回类型从 number 换成 NodeJS.Timeout，
// 为一个版本号测试去动全项目的类型不值得
import cargoToml from "../src-tauri/Cargo.toml?raw";

/**
 * 版本号一致性
 *
 * 三份清单各自带一个版本号，谁都不引用谁：`package.json` 给前端和 npm 脚本用，
 * `tauri.conf.json` 决定安装包名和更新检查，`Cargo.toml` 决定 crate 版本。
 * 手工同步的结果是页脚一路显示 1.0.0 显示到 v1.4 发版——中间没有任何一道门会红。
 *
 * 所以把「三个数字必须相等」变成一个测试：发版时改漏一处，CI 立刻拦下来。
 */
describe("版本号", () => {
  it("三份清单里写的是同一个版本", () => {
    // Cargo.toml 只取 [package] 段里的第一个 version，别撞上依赖的版本号
    const cargo = cargoToml.match(/^\[package\][\s\S]*?^version = "([^"]+)"/m);

    expect(cargo).not.toBeNull();
    expect(tauriConf.version).toBe(pkg.version);
    expect(cargo?.[1]).toBe(pkg.version);
  });

  it("版本号是三段数字，不是占位符", () => {
    // `0.0.0` / `1.0.0-dev` 这类值能编过也能打包，但装出来的东西无法追溯
    expect(pkg.version).toMatch(/^\d+\.\d+\.\d+$/);
  });
});
