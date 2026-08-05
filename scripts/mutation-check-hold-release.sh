#!/usr/bin/env bash
#
# 变异检查：保持窗口的「看见它动了就放手」这条
#
# 验收标准不是「测试通过」，是**改坏实现能让它变红**。三条新测试
# （a_recovered_session_lets_the_hold_go / a_finished_session_lets_the_hold_go /
# an_unsure_verdict_keeps_holding）如果有一条改坏了还绿，那条就是摆设。
#
# 跟另外两个脚本一样刻意不进 CI：它要反复编译整个 crate。
set -uo pipefail
cd "$(dirname "$0")/.."

TARGET="src-tauri/src/detector/mod.rs"
BACKUP="$(mktemp)"

if ! git diff --quiet -- "$TARGET"; then
  echo "✗ $TARGET 有未提交改动，先提交或 stash——脚本要改这个文件" >&2
  exit 1
fi

cp "$TARGET" "$BACKUP"
restore() { cp "$BACKUP" "$TARGET"; rm -f "$BACKUP"; }
trap restore EXIT INT TERM

# 原样这一行是放手的判据
ORIG='if matches!(verdict, Verdict::Running | Verdict::TaskCompleted) {'

expected=0
surprises=0

# 每条：说明 | 替换成什么 | 期望哪个测试变红（空 = 期望活下来，[等价]）
run_mutant() {
  local name="$1" replacement="$2" expect="$3"

  cp "$BACKUP" "$TARGET"
  if ! perl -0pi -e "s/\Q$ORIG\E/$replacement/" "$TARGET"; then
    echo "  ✗ $name：perl 出错" >&2
    surprises=$((surprises + 1))
    return
  fi
  if ! grep -qF "$replacement" "$TARGET"; then
    # 这一条最要紧：没改到任何东西的「检查」会报假绿
    echo "  ✗ $name：替换没生效，这条检查是假的" >&2
    surprises=$((surprises + 1))
    return
  fi

  local out
  out="$(cd src-tauri && cargo test --quiet --lib detector:: 2>&1)"
  local failed=""
  if grep -q "test result: FAILED" <<<"$out"; then
    failed="$(grep -oE '^ *[a-z_]+ ' <<<"$(sed -n '/failures:/,/^$/p' <<<"$out")" | tr -d ' ' | tr '\n' ' ')"
  fi

  if [[ -z "$expect" ]]; then
    if [[ -z "$failed" ]]; then
      echo "  ○ $name：[等价] 如预期没人红"
      expected=$((expected + 1))
    else
      echo "  ✗ $name：[等价] 但有人红了 → $failed" >&2
      surprises=$((surprises + 1))
    fi
  elif grep -q "$expect" <<<"$failed"; then
    echo "  ● $name：如预期红了（$expect）"
    expected=$((expected + 1))
  else
    echo "  ✗ $name：期望 $expect 变红，实际红的是「${failed:-没人红}」" >&2
    surprises=$((surprises + 1))
  fi
}

echo "变异检查：保持窗口的放手条件"

run_mutant "整条放手判据删掉（回到 v1.8 刚落地的行为）" \
  'if false {' "lets_the_hold_go"

run_mutant "只在跑起来时放手，干完了不放" \
  'if matches!(verdict, Verdict::Running) {' "a_finished_session_lets_the_hold_go"

run_mutant "只在干完时放手，跑起来了不放" \
  'if matches!(verdict, Verdict::TaskCompleted) {' "a_recovered_session_lets_the_hold_go"

run_mutant "把「说不清」也当成恢复信号" \
  'if matches!(verdict, Verdict::Running | Verdict::TaskCompleted | Verdict::Suspicious) {' \
  "an_unsure_verdict_keeps_holding"

run_mutant "确认中断也放手（等于这个功能整个失效）" \
  'if !matches!(verdict, Verdict::Suspicious) {' "still_holds_the_line"

echo
echo "结果：$expected 条如预期，$surprises 条要看"
[[ "$surprises" -eq 0 ]]
