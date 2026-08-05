import { describe, expect, it } from "vitest";
import { tickIndices } from "./BarChart";

/**
 * 坐标轴刻度
 *
 * 这个函数存在的全部理由是用户那句「柱状图下面不写时间，这个不能让用户
 * 一眼可以看出来数据哇」。原来的轴只有首尾两个日期，中间二十八根柱子
 * 没有参照。
 *
 * 最要紧的一条是**最后一根柱子必须有刻度**：最右边是「今天」，
 * 是读这张图第一个要找的坐标。正着数（`i += stride`）的话 30 根柱子配
 * 步长 5，最后一个刻度落在第 25 根上，今天反而没标注——所以实现是
 * 倒着数的，下面第一条测试盯的就是这件事。
 */
describe("tickIndices", () => {
  it("最后一根柱子一定有刻度", () => {
    for (const count of [1, 2, 7, 13, 30, 31, 90]) {
      expect(tickIndices(count), `${count} 根柱子`).toContain(count - 1);
    }
  });

  it("刻度数量不超过上限", () => {
    for (const count of [7, 13, 30, 90, 365]) {
      expect(tickIndices(count).size).toBeLessThanOrEqual(6);
    }
  });

  it("柱子少于上限时每根都标", () => {
    expect([...tickIndices(4)].sort((a, b) => a - b)).toEqual([0, 1, 2, 3]);
  });

  it("30 天按 5 格步长，从右往左", () => {
    expect([...tickIndices(30)].sort((a, b) => a - b)).toEqual([
      4, 9, 14, 19, 24, 29,
    ]);
  });

  it("没有数据就没有刻度", () => {
    expect(tickIndices(0).size).toBe(0);
    expect(tickIndices(-1).size).toBe(0);
  });

  it("刻度下标都在范围内", () => {
    for (const count of [1, 5, 30, 90]) {
      for (const i of tickIndices(count)) {
        expect(i).toBeGreaterThanOrEqual(0);
        expect(i).toBeLessThan(count);
      }
    }
  });

  it("上限可以调，仍然保住最后一根", () => {
    const ticks = tickIndices(30, 3);
    expect(ticks.size).toBeLessThanOrEqual(3);
    expect(ticks).toContain(29);
  });
});
