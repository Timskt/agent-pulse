#!/usr/bin/env bash
# 变异检查：事件推送泵的 8 个测试
#
# 逐个把实现改坏，跑一遍测试，记下哪些用例红了，然后从 git 恢复。
# 目的是回答一个具体问题：这 8 个测试里，有没有哪个是**摆设**
# ——改坏了它该守的那行，它还是绿的。
#
# 必须在工作区干净时跑（脚本靠 git checkout 恢复）。
set -uo pipefail
cd "$(dirname "$0")/.."

F=src-tauri/src/monitor/mod.rs

if [[ -n "$(git status --short -- $F)" ]]; then
  echo "拒绝运行：$F 有未提交改动，恢复步骤会把它丢掉"
  exit 1
fi

# 无论怎么退出都把源文件恢复回去。
#
# 这条是踩过才加的：脚本自己在中途崩了一次（`set -u` 撞上一个没定义的变量），
# 于是仓库里留下了一个**故意改坏**的实现，而屏幕上只有一行报错——正好是这个
# 脚本在检查的那类失败。恢复动作只有写在 trap 里才对崩溃、Ctrl-C、被 kill
# 同样有效；写在循环末尾只对「一切顺利」有效。
trap 'git checkout -- "$F" 2>/dev/null || true' EXIT INT TERM

# 每项：描述////sed 表达式
#
# 开头带 `[等价]` 的是**故意应该活下来**的变异体：改了它不代表实现坏了，
# 而是这个数本身就没有对错（见下面那条的说明）。这种要单独标出来，否则
# 「有变异体活着」这个信号会被当成噪音，久了整个脚本就没人看了。
MUTANTS=(
  '计数改成跟长度走（原 bug）////s|self.events_pushed = self.events_pushed.saturating_add(1);|self.events_pushed = self.events.len() as u64;|'
  '计数干脆不增////s|self.events_pushed = self.events_pushed.saturating_add(1);||'
  '不裁剪环////s|self.events.drain(0..drain_count);||'
  'fresh_tail 不封顶////s|behind.min(ring_len as u64) as usize|behind as usize|'
  'fresh_tail 用裸减法////s|let behind = pushed.saturating_sub(sent);|let behind = if pushed >= sent { pushed - sent } else { u64::MAX };|'
  'fresh_tail 恒返回 0////s|behind.min(ring_len as u64) as usize|0|'
  'fresh_tail 恒返回环长////s|behind.min(ring_len as u64) as usize|ring_len|'
  # 上限是个调参，不是正确性约束：500 还是 499 都对，测试全都拿常量本身
  # 断言（而不是抄字面量 500），所以改这个数没有任何测试会红——这是对的。
  # 反过来才是坏味道：为了「杀死」这个变异体去写 `assert_eq!(cap, 500)`，
  # 等于把调参钉死，以后想改上限得先改测试。
  '[等价] 上限 500 改 499////s|pub const EVENT_RING_CAP: usize = 500;|pub const EVENT_RING_CAP: usize = 499;|'
)

echo "=== 变异检查：${#MUTANTS[@]} 个变异体 ==="
PROBLEMS=0
for m in "${MUTANTS[@]}"; do
  desc="${m%%////*}"
  expr="${m##*////}"
  # 标了 [等价] 的反过来判：活下来才是对的，被杀死说明有测试把调参钉死了
  should_survive=0
  [[ "$desc" == \[等价\]* ]] && should_survive=1

  sed -i '' "$expr" "$F"
  if git diff --quiet -- "$F"; then
    echo "!! [$desc] sed 没匹配到任何东西——变异体无效，需要修脚本"
    git checkout -- "$F"
    PROBLEMS=$((PROBLEMS + 1))
    continue
  fi

  out=$(cd src-tauri && cargo test monitor::tests 2>&1)
  if grep -q "error\[" <<<"$out"; then
    killed=1
    detail="编译失败（也算被杀死）"
  else
    failed=$(grep -oE '^test monitor::tests::\w+ \.\.\. FAILED' <<<"$out" \
             | sed 's/^test monitor::tests:://; s/ \.\.\. FAILED//' | paste -sd, -)
    if [[ -z "$failed" ]]; then
      killed=0
      detail="没有任何测试红"
    else
      killed=1
      detail="杀死于: $failed"
    fi
  fi

  if [[ $should_survive -eq 1 ]]; then
    if [[ $killed -eq 1 ]]; then
      verdict="!! 本该活下来却被杀死了（${detail}）——有测试把调参钉死了"
      PROBLEMS=$((PROBLEMS + 1))
    else
      verdict="按预期活下来（${detail}，这是对的）"
    fi
  else
    if [[ $killed -eq 1 ]]; then
      verdict="$detail"
    else
      verdict="** 活下来了 —— ${detail}，这个测试是摆设 **"
      PROBLEMS=$((PROBLEMS + 1))
    fi
  fi
  # 分两行输出，不对齐成列：`printf %-30s` 按**字节**补空格，中文一个字三字节，
  # 于是每行的缩进都不一样，越对越乱。
  echo "· ${desc}"
  echo "    ${verdict}"

  git checkout -- "$F"
done

if [[ $PROBLEMS -eq 0 ]]; then
  echo "=== 全部符合预期 ==="
else
  echo "=== 有 $PROBLEMS 项不符合预期 ==="
fi
git status --short -- "$F"
[[ $PROBLEMS -eq 0 ]]
