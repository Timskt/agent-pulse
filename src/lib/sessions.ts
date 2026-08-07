import type { AgentSession, AttentionLevel, SessionStatus } from "../types";

/**
 * 总览里的会话视图范围。
 *
 * 这里描述的是“用户现在想看哪一批”，不是检测状态本身；Rust 仍然是状态与
 * 注意力的唯一事实来源，前端只在已经拿到的快照上做筛选和排序。
 */
export type SessionScope = "all" | "attention" | "stalled" | "active";

/** 注意力越需要人处理，越应该靠前。 */
const ATTENTION_WEIGHT: Record<AttentionLevel, number> = {
  needs_input: 0,
  error: 1,
  rate_limited: 2,
  completed: 3,
  none: 4,
};

const STATUS_WEIGHT: Record<SessionStatus, number> = {
  interrupted: 0,
  suspended: 1,
  active: 2,
  completed: 3,
  exited: 4,
};

export function isStalledSession(session: AgentSession): boolean {
  return session.status === "interrupted" || session.status === "suspended";
}

export function sessionInScope(
  session: AgentSession,
  scope: SessionScope,
): boolean {
  switch (scope) {
    case "attention":
      // 与 Rust `AttentionLevel::is_pending()` 的语义保持一致：已完成值得展示，
      // 但不属于“等我处理”。这里只筛 Rust 给出的枚举，不重算检测结论。
      return (
        session.attention === "needs_input" ||
        session.attention === "rate_limited" ||
        session.attention === "error"
      );
    case "stalled":
      return isStalledSession(session);
    case "active":
      return session.status === "active";
    case "all":
      return true;
  }
}

/**
 * 搜索只覆盖列表本来就展示、或用户能用来认出终端的元数据。
 * 不碰会话正文，也不把查询持久化到磁盘。
 */
export function sessionMatchesQuery(
  session: AgentSession,
  query: string,
): boolean {
  const needle = query.trim().toLocaleLowerCase();
  if (!needle) return true;

  return [
    session.agent_name,
    session.adapter_id,
    session.working_dir,
    session.terminal_app,
    session.tty,
    session.command,
    String(session.pid),
  ].some((value) => value?.toLocaleLowerCase().includes(needle));
}

export function sortSessions(sessions: readonly AgentSession[]): AgentSession[] {
  return [...sessions].sort(
    (a, b) =>
      ATTENTION_WEIGHT[a.attention] - ATTENTION_WEIGHT[b.attention] ||
      STATUS_WEIGHT[a.status] - STATUS_WEIGHT[b.status] ||
      b.last_activity.localeCompare(a.last_activity),
  );
}

export function selectVisibleSessions(
  sessions: readonly AgentSession[],
  scope: SessionScope,
  query: string,
): AgentSession[] {
  return sortSessions(
    sessions.filter(
      (session) =>
        sessionInScope(session, scope) && sessionMatchesQuery(session, query),
    ),
  );
}
