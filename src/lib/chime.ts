/**
 * 提醒声
 *
 * 不打包音频文件：一个 mp3 至少几十 KB，而这里要的只是「叮」一声。
 * Web Audio 现场合成两个音符，包体积零增长，音量也能直接调。
 *
 * `AudioContext` 懒创建——浏览器（以及 WebView）要求用户交互后才能出声，
 * 应用启动时就 new 一个会被挂起。
 */

let context: AudioContext | null = null;

function audioContext(): AudioContext | null {
  if (context) return context;
  const Ctor =
    window.AudioContext ??
    (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
  if (!Ctor) return null;
  context = new Ctor();
  return context;
}

/** 单个正弦音符，短促的 attack/decay 包络，避免爆音 */
function note(ctx: AudioContext, freq: number, startAt: number, gain: number) {
  const osc = ctx.createOscillator();
  const envelope = ctx.createGain();
  osc.type = "sine";
  osc.frequency.value = freq;
  envelope.gain.setValueAtTime(0, startAt);
  envelope.gain.linearRampToValueAtTime(gain, startAt + 0.01);
  envelope.gain.exponentialRampToValueAtTime(0.0001, startAt + 0.28);
  osc.connect(envelope).connect(ctx.destination);
  osc.start(startAt);
  osc.stop(startAt + 0.3);
}

/**
 * 播放提醒声
 *
 * @param volume 0-100，来自配置里的音量
 * @param urgent 需要人立刻回应时音高更高、两声更急
 */
export function playChime(volume: number, urgent = false) {
  const ctx = audioContext();
  if (!ctx) return;
  // WebView 可能把上下文挂起（窗口最小化过），响之前先唤醒
  if (ctx.state === "suspended") void ctx.resume();

  const gain = Math.min(Math.max(volume, 0), 100) / 100 * 0.25;
  if (gain <= 0) return;

  const now = ctx.currentTime;
  if (urgent) {
    note(ctx, 880, now, gain);
    note(ctx, 1174.7, now + 0.16, gain);
  } else {
    note(ctx, 659.3, now, gain);
    note(ctx, 987.8, now + 0.14, gain * 0.8);
  }
}
