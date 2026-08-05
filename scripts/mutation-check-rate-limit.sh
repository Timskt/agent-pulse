#!/usr/bin/env bash
# 变异检查：限流识别与保持窗口（v1.8）
#
# 验收标准不是「测试通过」，是「把实现改坏，确认它会红」。一条测试如果
# 改坏实现之后照样绿，它就没有在守任何东西——绿灯只是在骗人。
#
# 用法：./scripts/mutation-check-rate-limit.sh
# 前提：目标文件是干净的（脚本会拒绝在有未提交改动时运行，免得把你的
#       改动跟变异混在一起，然后靠 git checkout 一起丢掉）。

set -euo pipefail

cd "$(dirname "$0")/.."
DETECTOR="src-tauri/src/detector/mod.rs"
RATE_LIMIT="src-tauri/src/detector/rate_limit.rs"

for f in "$DETECTOR" "$RATE_LIMIT"; do
  if ! git diff --quiet -- "$f"; then
    echo "✗ $f 有未提交改动，先提交或 stash——脚本要反复改写它" >&2
    exit 1
  fi
done

# 崩了也要把文件还原。上一版没有这个 trap，脚本中途挂掉之后
# 留了一个变异在源码里，差点被一起提交。
restore() { git checkout -- "$DETECTOR" "$RATE_LIMIT" 2>/dev/null || true; }
trap restore EXIT

# 格式：描述////文件////sed 表达式
# 描述以「[等价]」开头的，表示这个变异在语义上等价、活下来才是对的，
# 判定反转。不标出来的话「有变异活下来」就变成了噪音，而一份没人看的
# 报告等于没做检查。
#
# 表达式走的是 **perl 正则**（`perl -0777 -pi -e`），不是 sed 的那套：
# `(` `)` `|` `.` `[` 全是元字符，要写字面量就得转义。第一版没转义，于是
# 十八条里有八条根本没改到任何东西——而「没改到」和「改了但没测试发现」
# 在结果上长得一模一样，都是静默通过。下面那道 `git diff --quiet` 检查
# 就是为这个装的：变异没生效要当成失败报出来，不然这份报告是假的。
MUTANTS=(
  '限流保持窗口的下限降到 0 秒////'"$DETECTOR"'////s/pub const RATE_LIMIT_HOLD_FLOOR_SECS: u64 = 60;/pub const RATE_LIMIT_HOLD_FLOOR_SECS: u64 = 0;/'
  '保持窗口取最小值而不是最大值////'"$DETECTOR"'////s/\.max\(cooldown\)/.min(cooldown)/'
  '窗口过期判断反向（到点了才按住）////'"$DETECTOR"'////s/Ok\(deadline\) => now < deadline,/Ok(deadline) => now > deadline,/'
  '存坏的时间戳当成一直按住////'"$DETECTOR"'////s/Err\(_\) => false,/Err(_) => true,/'
  '没有窗口时当成正在按住////'"$DETECTOR"'////s/let Some\(until\) = until else \{\n        return false;/let Some(until) = until else {\n        return true;/'
  '上游拒绝改成敲字////'"$DETECTOR"'////s/InterruptReason::RateLimited \| InterruptReason::UpstreamRejected => ResumeTactic::Wait,/InterruptReason::RateLimited => ResumeTactic::Wait,\n            InterruptReason::UpstreamRejected => ResumeTactic::Nudge,/'
  '兜底形状识别整段失效////'"$DETECTOR"'////s/if let Some\(shape\) = rate_limit::upstream_rejection\(&lower\)/if let Some(shape) = Option::<rate_limit::RejectionShape>::None/'
  '窗口内不再复用当初那个原因////'"$DETECTOR"'////s/return \(hold\.reason, Some\(hold\.clone\(\)\)\);/return (classified, Some(hold.clone()));/'
  '普通停顿也起保持窗口////'"$DETECTOR"'////s/InterruptReason::RateLimited \| InterruptReason::UpstreamRejected\n        \)/InterruptReason::RateLimited | InterruptReason::UpstreamRejected | InterruptReason::Stalled\n        )/'
  '状态码表里去掉 429////'"$RATE_LIMIT"'////s/"429", "529"/"529"/'
  '把 500 也算成限流形状////'"$RATE_LIMIT"'////s/"504"\]/"504", "500"]/'
  '中文限流说法失效////'"$RATE_LIMIT"'////s/"上游负载",/"__never_matches__",/'
  # 这一条守的是一个真实踩过的坑：`contains_keyword` 对 ASCII 关键词要求词
  # 边界，所以写词干（`throttl`）永远匹配不上任何真实单词（`throttled` 的
  # `e` 紧跟其后）。表里第一版就是词干，白占一行、一个都认不出来。
  '短语退回成永不命中的词干////'"$RATE_LIMIT"'////s/"throttled",/"throttl",/'
  '等待时间不再封顶////'"$RATE_LIMIT"'////s/pub const MAX_WAIT_HINT_SECS: u64 = 3600;/pub const MAX_WAIT_HINT_SECS: u64 = u64::MAX;/'
  '毫秒当成秒////'"$RATE_LIMIT"'////s/"ms" => 0,/"ms" => value,/'
  '分钟不再换算成秒////'"$RATE_LIMIT"'////s/value\.saturating_mul\(60\)/value/'
  '中文时长不要求跟着「后」或「再」////'"$RATE_LIMIT"'////s/let is_wait = after\.starts_with\("后"\) \|\| after\.starts_with\("再"\);/let is_wait = true;/'
  '跨过字母去抓远处的数字////'"$RATE_LIMIT"'////s/if bytes\[i\]\.is_ascii_alphabetic\(\) \{\n            return None;\n        \}/if false {\n            return None;\n        }/'
  '[等价] 短语匹配用裸 contains 替代////'"$RATE_LIMIT"'////s/super::contains_keyword\(lower_errors, phrase\)/lower_errors.contains(phrase)/'
)

pass=0
fail=0

for entry in "${MUTANTS[@]}"; do
  desc="${entry%%////*}"
  rest="${entry#*////}"
  file="${rest%%////*}"
  expr="${rest#*////}"

  equivalent=0
  case "${desc}" in
    '[等价]'*) equivalent=1 ;;
  esac

  perl -0777 -pi -e "${expr}" "${file}"

  if git diff --quiet -- "${file}"; then
    echo "· ${desc}"
    echo "  ⚠ sed 表达式没改到任何东西——变异根本没生效，这条检查是假的"
    fail=$((fail + 1))
    restore
    continue
  fi

  if (cd src-tauri && cargo test --lib detector >/dev/null 2>&1); then
    survived=1
  else
    survived=0
  fi

  restore

  echo "· ${desc}"
  if [ "${equivalent}" = 1 ]; then
    if [ "${survived}" = 1 ]; then
      echo "  ✓ 活下来了（语义等价，这是对的）"
      pass=$((pass + 1))
    else
      echo "  ⚠ 被杀了——说明它其实不等价，值得看一眼"
      fail=$((fail + 1))
    fi
  else
    if [ "${survived}" = 1 ]; then
      echo "  ✗ 活下来了：没有任何测试在守这一条"
      fail=$((fail + 1))
    else
      echo "  ✓ 被杀了"
      pass=$((pass + 1))
    fi
  fi
done

echo
echo "结果：${pass} 条如预期，${fail} 条要看"
[ "${fail}" -eq 0 ]
