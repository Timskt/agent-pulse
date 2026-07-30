//! 手机看板 —— 只读远程视图（v1.3 远程层 P1）
//!
//! 痛点 #4「不在电脑前就等于停摆」的下半场：推送负责告诉你「出事了」，
//! 看板负责让你「看清是哪个会话」。手机浏览器打开一个网址就行，不用装东西。
//!
//! 安全上按三条来，缺一条都不开门：
//! 1. 总开关默认关；
//! 2. 开了也只听 `127.0.0.1`，除非用户在设置里显式勾「允许局域网访问」；
//! 3. 无论听哪个地址，`/` 和 `/api/state` 都必须带令牌，且用定长比较，
//!    不会因为逐字符提前返回而泄露前缀。令牌为空（比如生成后写盘失败）
//!    时一律拒绝，宁可看不到也不要裸奔。
//!
//! **只读**不是靠约定，是靠路由表：这里只有两个 GET 端点，非 GET 直接 405，
//! 没有任何一条路径能改配置、能续跑、能停监控。
//!
//! 吐出去的数据也是删减过的：项目名（只有 basename）、状态、注意力、
//! 续跑次数、token 与花费。PID、TTY、完整命令行、会话文件路径留在本机——
//! 那些对「看一眼」没用，被人瞄到却足够拼出你在跑什么。

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::adapters::SessionStatus;
use crate::config::{AppConfig, ConfigManager};
use crate::i18n::I18n;
use crate::monitor::{EngineEvent, LogLevel, MonitorEngine};

/// 请求头最大字节数；看板只有 GET，8 KiB 装不下的一律不接
const MAX_HEAD: usize = 8 * 1024;
/// 单个连接的读超时：手机切后台留下的半开连接不能一直占着 fd
const READ_TIMEOUT: Duration = Duration::from_secs(5);

const TEXT: &str = "text/plain; charset=utf-8";
const HTML: &str = "text/html; charset=utf-8";
const JSON: &str = "application/json; charset=utf-8";

/// 正在监听的实例
struct Running {
    /// 绑定地址；只有它变了才需要重启监听
    addr: String,
    handle: JoinHandle<()>,
}

/// 看板服务
pub struct RemoteService {
    engine: Arc<MonitorEngine>,
    config_manager: Arc<ConfigManager>,
    running: Mutex<Option<Running>>,
}

impl RemoteService {
    pub fn new(engine: Arc<MonitorEngine>, config_manager: Arc<ConfigManager>) -> Self {
        Self {
            engine,
            config_manager,
            running: Mutex::new(None),
        }
    }

    /// 让服务和配置对齐：该开的开、该停的停、地址变了就重启
    ///
    /// 令牌和界面语言是每次请求现读的，所以改这两项不用重启监听，
    /// 保存设置立刻生效。
    pub async fn sync(&self) {
        let cfg = self.config_manager.get();
        let mut running = self.running.lock().await;

        if !cfg.remote.enabled {
            if let Some(current) = running.take() {
                current.handle.abort();
                self.log(
                    LogLevel::Info,
                    self.i18n().t("log.remote_stopped").to_string(),
                )
                .await;
            }
            return;
        }

        // 没有令牌等于把会话公开出去，先补一个再开门；补不上就干脆不开
        if cfg.remote.token.trim().is_empty() && !self.generate_token(&cfg) {
            return;
        }

        let host = if cfg.remote.bind_all {
            "0.0.0.0"
        } else {
            "127.0.0.1"
        };
        let addr = format!("{host}:{}", cfg.remote.port);
        if running.as_ref().is_some_and(|r| r.addr == addr) {
            return;
        }
        if let Some(current) = running.take() {
            current.handle.abort();
        }

        let listener = match TcpListener::bind(&addr).await {
            Ok(listener) => listener,
            Err(e) => {
                let i18n = self.i18n();
                let message = i18n.tf(
                    "err.remote_bind",
                    &[
                        ("port", &cfg.remote.port.to_string()),
                        ("detail", &e.to_string()),
                    ],
                );
                self.log(LogLevel::Error, message).await;
                return;
            }
        };

        let engine = self.engine.clone();
        let config_manager = self.config_manager.clone();
        let handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let engine = engine.clone();
                        let config_manager = config_manager.clone();
                        // 一个连接卡住不能拖累其它请求，各自一个任务
                        tokio::spawn(async move {
                            let _ = serve(stream, engine, config_manager).await;
                        });
                    }
                    // accept 失败多半是暂时的（fd 用尽之类）。直接 break 会让
                    // 看板从此哑掉且没人知道，所以歇一下继续。
                    Err(e) => {
                        tracing::warn!("[AgentPulse] 看板 accept 失败: {e}");
                        tokio::time::sleep(Duration::from_millis(200)).await;
                    }
                }
            }
        });
        *running = Some(Running {
            addr: addr.clone(),
            handle,
        });
        drop(running);

        let i18n = self.i18n();
        // 日志里不带令牌：活动日志是会被截图发出去的
        let started = i18n.tf("log.remote_started", &[("url", &format!("http://{addr}/"))]);
        self.log(LogLevel::Success, started).await;
        if cfg.remote.bind_all {
            self.log(LogLevel::Warn, i18n.t("log.remote_lan").to_string())
                .await;
        }
    }

    fn i18n(&self) -> I18n {
        I18n::from_code(&self.config_manager.get().language)
    }

    async fn log(&self, level: LogLevel, message: String) {
        self.engine
            .push_event(EngineEvent::new(level, None, message))
            .await;
    }

    /// 生成并落盘一个令牌；返回是否成功
    ///
    /// `ConfigManager::update` 先写盘后改内存，所以写盘失败时内存里的令牌
    /// 仍然是空的——请求处理那边会因此拒绝一切访问，正是我们想要的方向。
    fn generate_token(&self, cfg: &AppConfig) -> bool {
        let mut next = cfg.clone();
        next.remote.token = uuid::Uuid::new_v4().simple().to_string();
        match self.config_manager.update(next) {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!("[AgentPulse] 看板令牌写入失败，不启动看板: {e}");
                false
            }
        }
    }
}

/// 处理一个连接：读一个请求、回一个响应、关掉
///
/// 不做 keep-alive。看板是「几秒一次 GET」的量，省下的复用远不如
/// 「一个连接只对应一次请求」带来的确定性值钱。
async fn serve(
    mut stream: TcpStream,
    engine: Arc<MonitorEngine>,
    config_manager: Arc<ConfigManager>,
) -> std::io::Result<()> {
    let Some(request) = read_head(&mut stream).await else {
        return respond(&mut stream, "400 Bad Request", TEXT, "bad request\n").await;
    };

    let first_line = request.lines().next().unwrap_or_default();
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();

    // 只读服务，非 GET 一律拒绝——包括 POST/PUT/DELETE
    if method != "GET" {
        return respond(&mut stream, "405 Method Not Allowed", TEXT, "read-only\n").await;
    }

    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path == "/favicon.ico" {
        return respond(&mut stream, "204 No Content", TEXT, "").await;
    }
    if path != "/" && path != "/api/state" {
        return respond(&mut stream, "404 Not Found", TEXT, "not found\n").await;
    }

    let cfg = config_manager.get();
    let token = cfg.remote.token.trim();
    // 令牌为空说明生成或落盘出了问题，这时候一个字节都不给（fail closed）
    let authed = !token.is_empty()
        && presented_token(&request, query).is_some_and(|given| secret_eq(&given, token));
    if !authed {
        return respond(&mut stream, "401 Unauthorized", TEXT, "unauthorized\n").await;
    }

    let i18n = I18n::from_code(&cfg.language);
    if path == "/" {
        let nonce = uuid::Uuid::new_v4().simple().to_string();
        respond_with_nonce(&mut stream, HTML, &page(&i18n, &nonce), Some(&nonce)).await
    } else {
        respond(
            &mut stream,
            "200 OK",
            JSON,
            &state_json(&engine, &i18n).await,
        )
        .await
    }
}

/// 读到请求头结束为止，超过 `MAX_HEAD` 或超时都当无效请求
async fn read_head(stream: &mut TcpStream) -> Option<String> {
    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        let read = timeout(READ_TIMEOUT, stream.read(&mut chunk))
            .await
            .ok()?
            .ok()?;
        if read == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..read]);
        if find_head_end(&buf) {
            break;
        }
        if buf.len() > MAX_HEAD {
            return None;
        }
    }
    String::from_utf8(buf).ok()
}

/// 请求头是否已经收完
fn find_head_end(buf: &[u8]) -> bool {
    buf.windows(4).any(|w| w == b"\r\n\r\n")
}

/// 从 `?token=` 或 `Authorization: Bearer` 里取令牌
///
/// 两种都支持：链接直接存书签走 query，脚本或反代走 header。
fn presented_token(request: &str, query: &str) -> Option<String> {
    for pair in query.split('&') {
        if let Some(value) = pair.strip_prefix("token=") {
            return Some(percent_decode(value));
        }
    }
    request
        .lines()
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        .and_then(|(_, value)| value.trim().strip_prefix("Bearer "))
        .map(|value| value.trim().to_string())
}

/// `%XX` 解码（够 URL 里塞令牌用；非法转义原样保留）
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => match u8::from_str_radix(&s[i + 1..i + 3], 16) {
                Ok(byte) => {
                    out.push(byte);
                    i += 3;
                }
                Err(_) => {
                    out.push(b'%');
                    i += 1;
                }
            },
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 定长比较
///
/// 逐字符提前 return 会把「猜对了几位」透过响应时间漏出去。令牌不长，
/// 全部比完也就几十个字节的事。
fn secret_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

async fn respond(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &str,
) -> std::io::Result<()> {
    respond_full(stream, status, content_type, body, None).await
}

async fn respond_with_nonce(
    stream: &mut TcpStream,
    content_type: &str,
    body: &str,
    nonce: Option<&str>,
) -> std::io::Result<()> {
    respond_full(stream, "200 OK", content_type, body, nonce).await
}

/// 统一出口，安全响应头写在一处
///
/// 故意**不给** `Access-Control-Allow-Origin`：看板只在自己的页面里被读，
/// 加了 CORS 等于允许任意网站拿着你的令牌来抓数据。
async fn respond_full(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &str,
    nonce: Option<&str>,
) -> std::io::Result<()> {
    // 页面里只有一段自带的内联脚本，用 nonce 放行它，其它一律不许执行；
    // 会话数据全部走 textContent，所以也没有注入 HTML 的入口。
    let csp = match nonce {
        Some(nonce) => format!(
            "default-src 'none'; style-src 'unsafe-inline'; script-src 'nonce-{nonce}'; \
             connect-src 'self'; base-uri 'none'; form-action 'none'"
        ),
        None => "default-src 'none'".to_string(),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {len}\r\n\
         Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\
         Referrer-Policy: no-referrer\r\nContent-Security-Policy: {csp}\r\n\
         Connection: close\r\n\r\n",
        len = body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body.as_bytes()).await?;
    stream.flush().await
}

/// 会话快照 → 看板 JSON
///
/// 这里就是「给多少」的边界：状态和注意力已经翻译成当前语言的成品文案，
/// 数字也已经格式化好，页面上的 JS 只负责往 `textContent` 里放。
/// 不含 id / pid / tty / 命令行 / 会话文件路径。
async fn state_json(engine: &MonitorEngine, i18n: &I18n) -> String {
    let state = engine.state.lock().await;
    let sessions: Vec<_> = state
        .sessions
        .iter()
        .map(|session| {
            let usage = session.usage.as_ref();
            json!({
                "project": base_name(&session.working_dir),
                "agent": session.agent_name,
                "status": i18n.t(status_key(&session.status)),
                "attention": session.attention.key(),
                "attention_label": i18n.t(session.attention.i18n_key()),
                "pending": session.attention.is_pending(),
                "detail": session.attention_detail.clone().unwrap_or_default(),
                "resumed": if session.resume_count > 0 {
                    i18n.tf("remote.resumed", &[("count", &session.resume_count.to_string())])
                } else {
                    String::new()
                },
                "usage": usage
                    .filter(|u| u.total_tokens > 0)
                    .map(|u| format!("{} tokens · ${:.2}", format_tokens(u.total_tokens), u.cost_usd))
                    .unwrap_or_default(),
                "at": i18n.tf("remote.updated", &[("time", short_time(&session.last_activity))]),
            })
        })
        .collect();

    json!({
        "state_label": i18n.t(if state.running { "remote.watching" } else { "remote.idle" }),
        "running": state.running,
        "sessions_total": state.status.sessions_total,
        "pending": state.status.pending_attention,
        "cost_today": format!("{:.2}", state.status.cost_today),
        "sessions": sessions,
    })
    .to_string()
}

/// 状态 → i18n 键
///
/// 和 `notify` 模块一个路子：`I18n::t` 只吃 `&'static str`，所以键必须
/// 显式列出来，不能 `format!` 拼。少列一个分支编译器会提醒。
fn status_key(status: &SessionStatus) -> &'static str {
    match status {
        SessionStatus::Active => "status.active",
        SessionStatus::Suspended => "status.suspended",
        SessionStatus::Interrupted => "status.interrupted",
        SessionStatus::Completed => "status.completed",
        SessionStatus::Exited => "status.exited",
    }
}

/// 路径最后一段；手机屏幕窄，全路径没法看
fn base_name(path: &str) -> &str {
    path.rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or(path)
}

/// `%Y-%m-%d %H:%M:%S` → `HH:MM`
fn short_time(stamp: &str) -> &str {
    if stamp.len() >= 16 {
        &stamp[11..16]
    } else {
        stamp
    }
}

/// 1234 → `1.2k`，1234567 → `1.2M`
fn format_tokens(tokens: u64) -> String {
    match tokens {
        n if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1_000_000.0),
        n if n >= 1_000 => format!("{:.1}k", n as f64 / 1_000.0),
        n => n.to_string(),
    }
}

/// 看板页面
///
/// 单文件：没有外链、没有构建产物，所以 CSP 里 `default-src 'none'` 就够用。
/// 文案在服务端注进去，看板跟着桌面端的语言设置走，不会界面中文、看板英文。
fn page(i18n: &I18n, nonce: &str) -> String {
    let labels = json!({
        "title": i18n.t("remote.title"),
        "readonly": i18n.t("remote.readonly"),
        "sessions": i18n.t("remote.metric_sessions"),
        "pending": i18n.t("remote.metric_pending"),
        "cost": i18n.t("remote.metric_cost"),
        "empty": i18n.t("remote.empty"),
        "offline": i18n.t("remote.offline"),
    })
    .to_string()
    // JSON 里只要出现 `</script` 就会提前闭合脚本块。文案里本不该有尖括号，
    // 但转义掉比相信文案便宜。
    .replace('<', "\\u003c");

    TEMPLATE
        .replace("__LANG__", i18n.lang().as_str())
        .replace("__TITLE__", &escape_html(i18n.t("remote.title")))
        .replace("__NONCE__", nonce)
        .replace("__LABELS__", &labels)
}

/// 只用在 `<title>` 上：其它文字全部走 `textContent`，不需要转义
fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

const TEMPLATE: &str = r##"<!doctype html>
<html lang="__LANG__">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="color-scheme" content="light">
<title>__TITLE__</title>
<style>
*{box-sizing:border-box;margin:0}
body{max-width:520px;margin:0 auto;padding:16px;background:#fafafa;color:#171717;
font:14px/1.5 -apple-system,BlinkMacSystemFont,"Helvetica Neue",Arial,sans-serif}
header{display:flex;align-items:center;gap:8px;margin-bottom:14px}
h1{font-size:15px;font-weight:600;letter-spacing:-.01em}
.tag{padding:2px 6px;border:1px solid #e5e5e5;border-radius:999px;background:#f5f5f5;
color:#737373;font-size:10px;white-space:nowrap}
.tag.on{background:#ecfdf5;border-color:#a7f3d0;color:#047857}
.metrics{display:grid;grid-template-columns:repeat(3,1fr);gap:8px;margin-bottom:14px}
.metric{padding:10px;border:1px solid #e5e5e5;border-radius:10px;background:#fff}
.metric b{display:block;font-size:18px;font-variant-numeric:tabular-nums}
.metric span{color:#a3a3a3;font-size:10px}
.card{display:flex;gap:10px;padding:12px;margin-bottom:8px;border:1px solid #e5e5e5;
border-radius:10px;background:#fff}
.dot{flex:none;width:6px;height:6px;margin-top:6px;border-radius:999px;background:#d4d4d4}
.dot.needs_input{background:#ef4444}
.dot.completed{background:#10b981}
.dot.rate_limited{background:#f59e0b}
.dot.error{background:#525252}
.row{display:flex;flex-wrap:wrap;align-items:center;gap:6px}
.project{font:600 12px ui-monospace,SFMono-Regular,Menlo,monospace}
.agent{color:#a3a3a3;font-size:10px}
.meta{margin-top:4px;color:#a3a3a3;font-size:10px;font-variant-numeric:tabular-nums}
.detail{margin-top:4px;color:#525252;font-size:11px;word-break:break-word}
.empty{padding:28px 12px;color:#a3a3a3;font-size:12px;text-align:center}
.offline{padding:10px;margin-bottom:8px;border:1px solid #fde68a;border-radius:10px;
background:#fffbeb;color:#b45309;font-size:12px;text-align:center}
[hidden]{display:none}
</style>
</head>
<body>
<header>
<h1 id="title"></h1>
<span class="tag" id="readonly"></span>
<span class="tag" id="state"></span>
</header>
<p class="offline" id="offline" hidden></p>
<div class="metrics">
<div class="metric"><b id="m-sessions">–</b><span id="l-sessions"></span></div>
<div class="metric"><b id="m-pending">–</b><span id="l-pending"></span></div>
<div class="metric"><b id="m-cost">–</b><span id="l-cost"></span></div>
</div>
<div id="list"></div>
<script nonce="__NONCE__">
const L = __LABELS__;
const $ = (id) => document.getElementById(id);
$("title").textContent = L.title;
$("readonly").textContent = L.readonly;
$("l-sessions").textContent = L.sessions;
$("l-pending").textContent = L.pending;
$("l-cost").textContent = L.cost;

// 一切用户数据都只经过 textContent，页面里没有一处 innerHTML，
// 所以项目名里带尖括号也只是几个字符，注入不进来。
function node(tag, cls, value) {
  const el = document.createElement(tag);
  if (cls) el.className = cls;
  if (value) el.textContent = value;
  return el;
}

function render(data) {
  const state = $("state");
  state.textContent = data.state_label;
  state.classList.toggle("on", data.running);
  $("m-sessions").textContent = data.sessions_total;
  $("m-pending").textContent = data.pending;
  $("m-cost").textContent = "$" + data.cost_today;

  const list = $("list");
  list.textContent = "";
  if (!data.sessions.length) {
    list.append(node("p", "empty", L.empty));
    return;
  }
  for (const s of data.sessions) {
    const card = node("div", "card");
    card.append(node("span", "dot " + s.attention));
    const body = node("div");
    const row = node("div", "row");
    row.append(node("span", "project", s.project), node("span", "agent", s.agent));
    row.append(node("span", "tag", s.status));
    if (s.pending) row.append(node("span", "tag", s.attention_label));
    if (s.resumed) row.append(node("span", "agent", s.resumed));
    body.append(row);
    if (s.detail) body.append(node("p", "detail", s.detail));
    const meta = [s.at, s.usage].filter(Boolean).join("  ·  ");
    if (meta) body.append(node("p", "meta", meta));
    card.append(body);
    list.append(card);
  }
}

// 令牌跟着当前地址走：链接存了书签，刷新也还带着
async function tick() {
  try {
    const res = await fetch("/api/state" + location.search, { cache: "no-store" });
    if (!res.ok) throw new Error(res.status);
    render(await res.json());
    $("offline").hidden = true;
  } catch {
    // 电脑上的应用退了、或者手机刚从锁屏回来都会走到这儿。
    // 页面保留上一次的数据，只在顶上挂个提示，别让人以为会话全没了。
    $("offline").textContent = L.offline;
    $("offline").hidden = false;
  }
}

tick();
setInterval(tick, 4000);
// 息屏期间的轮询没人看，回到前台立刻补一次，省电也省得看旧数据
document.addEventListener("visibilitychange", () => {
  if (!document.hidden) tick();
});
</script>
</body>
</html>
"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_eq_matches_only_exact() {
        assert!(secret_eq("abc123", "abc123"));
        assert!(!secret_eq("abc123", "abc124"));
        // 长度不同直接判否；这一条本身不算泄露，令牌长度不是秘密
        assert!(!secret_eq("abc", "abc123"));
        assert!(!secret_eq("", "abc"));
    }

    #[test]
    fn reads_token_from_query() {
        let head = "GET /?token=abc HTTP/1.1\r\nHost: x\r\n\r\n";
        assert_eq!(presented_token(head, "token=abc"), Some("abc".into()));
        assert_eq!(
            presented_token(head, "foo=1&token=a%20b"),
            Some("a b".into())
        );
    }

    #[test]
    fn reads_token_from_bearer_header() {
        // Host 也含冒号，所以这条同时钉住「别拿第一个带冒号的行当 header」
        let head =
            "GET /api/state HTTP/1.1\r\nHost: 127.0.0.1:17650\r\nAuthorization: Bearer tok\r\n\r\n";
        assert_eq!(presented_token(head, ""), Some("tok".into()));
    }

    #[test]
    fn no_token_means_none() {
        let head = "GET / HTTP/1.1\r\nHost: x\r\n\r\n";
        assert_eq!(presented_token(head, ""), None);
    }

    #[test]
    fn head_end_needs_blank_line() {
        assert!(!find_head_end(b"GET / HTTP/1.1\r\nHost: x\r\n"));
        assert!(find_head_end(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n"));
    }

    #[test]
    fn page_has_no_leftover_placeholders() {
        let html = page(&I18n::new(crate::i18n::Lang::En), "n0nce");
        assert!(!html.contains("__"), "模板里还有没替换的占位符");
        assert!(html.contains("nonce=\"n0nce\""));
        assert!(html.contains("Read-only"));
    }

    #[test]
    fn formats_are_compact() {
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1_500), "1.5k");
        assert_eq!(format_tokens(2_400_000), "2.4M");
        assert_eq!(base_name("/Users/sky/code/agent-pulse"), "agent-pulse");
        assert_eq!(base_name("/Users/sky/code/agent-pulse/"), "agent-pulse");
        assert_eq!(short_time("2026-07-30 14:05:09"), "14:05");
        assert_eq!(short_time("odd"), "odd");
    }

    /// 手搓的 HTTP 得真的是 HTTP：状态行、Content-Length、安全头一次钉住
    #[tokio::test]
    async fn response_is_well_formed_http() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let head = read_head(&mut stream).await.unwrap();
            assert!(head.starts_with("GET /?token=x HTTP/1.1"));
            respond_with_nonce(&mut stream, HTML, "<p>ok</p>", Some("n1"))
                .await
                .unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        client
            .write_all(b"GET /?token=x HTTP/1.1\r\nHost: t\r\n\r\n")
            .await
            .unwrap();
        let mut raw = String::new();
        client.read_to_string(&mut raw).await.unwrap();
        server.await.unwrap();

        assert!(raw.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(raw.contains("Content-Length: 9\r\n"));
        assert!(raw.contains("script-src 'nonce-n1'"));
        assert!(raw.contains("Cache-Control: no-store"));
        // 故意不给 CORS：加了等于允许任意网站拿着你的令牌来抓数据
        assert!(!raw.to_ascii_lowercase().contains("access-control-allow"));
        assert!(raw.ends_with("<p>ok</p>"));
    }
}
