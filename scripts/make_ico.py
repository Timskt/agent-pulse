#!/usr/bin/env python3
"""把若干张 PNG 打包成一个多尺寸 .ico。

Windows 的任务栏、Alt-Tab、资源管理器各自挑不同尺寸，只塞一张 256 的话
小尺寸全靠系统临时缩放，边缘就会毛。所以每个常用尺寸都单独渲染一张塞进去。

这里手写 ICONDIR 而不是引第三方库：机器上没有 Pillow，而 ICO 的容器格式
就是一个 6 字节头 + 每张 16 字节目录项 + 原样 PNG 数据，没必要为它装依赖。
"""

from __future__ import annotations

import struct
import sys
from pathlib import Path


def build(sources: list[Path], out: Path) -> None:
    payloads: list[tuple[int, bytes]] = []
    for src in sources:
        data = src.read_bytes()
        # PNG 的 IHDR 就在固定位置：8 字节签名 + 4 长度 + 4 类型，之后是宽高
        width, height = struct.unpack(">II", data[16:24])
        if width != height:
            raise SystemExit(f"{src} 不是正方形（{width}×{height}）")
        payloads.append((width, data))

    payloads.sort(key=lambda item: item[0])
    header = struct.pack("<HHH", 0, 1, len(payloads))  # reserved, type=icon, count
    offset = len(header) + 16 * len(payloads)

    entries = bytearray()
    for size, data in payloads:
        # 256 在目录项里写 0：这个字段只有一个字节，装不下 256
        dim = 0 if size >= 256 else size
        entries += struct.pack(
            "<BBBBHHII",
            dim,  # width
            dim,  # height
            0,  # 调色板数（PNG 不用）
            0,  # reserved
            1,  # color planes
            32,  # bits per pixel
            len(data),
            offset,
        )
        offset += len(data)

    out.write_bytes(bytes(header) + bytes(entries) + b"".join(d for _, d in payloads))


if __name__ == "__main__":
    if len(sys.argv) < 3:
        raise SystemExit("用法: make_ico.py <输出.ico> <输入1.png> [输入2.png ...]")
    build([Path(p) for p in sys.argv[2:]], Path(sys.argv[1]))
