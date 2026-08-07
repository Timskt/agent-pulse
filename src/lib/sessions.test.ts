import { describe, expect, it } from "vitest";
import type { AgentSession } from "../types";
import {
  selectVisibleSessions,
  sessionInScope,
  sessionMatchesQuery,
  sortSessions,
} from "./sessions";

function session(
  id: string,
  overrides: Partial<AgentSession> = {},
): AgentSession {
  return {
    id,
    runtime_generation: `${id}:4242:opaque-generation`,
    adapter_id: "claude-code",
    agent_name: "Claude Code",
    pid: 100,
    command: "claude",
    working_dir: `/work/${id}`,
    session_file: `/tmp/${id}.jsonl`,
    discovered_at: "2026-08-07T01:00:00Z",
    last_activity: "2026-08-07T02:00:00Z",
    status: "active",
    resume_count: 0,
    last_resume_at: null,
    resume_streak: 0,
    resume_failures: 0,
    attention: "none",
    attention_detail: null,
    detection_evidence: null,
    interrupt_reason: "none",
    resume_tactic: "nudge",
    tty: "/dev/ttys003",
    terminal_app: "iTerm2",
    usage: null,
    ...overrides,
  };
}

describe("session view", () => {
  it("searches agent, project path, terminal, tty, command and pid", () => {
    const target = session("agent-pulse", {
      pid: 4242,
      command: "codex resume",
      terminal_app: "Windsurf",
    });

    for (const query of [
      "claude",
      "agent-pulse",
      "windsurf",
      "ttys003",
      "codex resume",
      "4242",
    ]) {
      expect(sessionMatchesQuery(target, query), query).toBe(true);
    }
    expect(sessionMatchesQuery(target, "unrelated")).toBe(false);
  });

  it("trims query and matches without case sensitivity", () => {
    expect(sessionMatchesQuery(session("Alpha"), "  ALPHA  ")).toBe(true);
    expect(sessionMatchesQuery(session("Alpha"), "   ")).toBe(true);
  });

  it("attention scope matches Rust pending semantics and excludes completed", () => {
    for (const attention of ["needs_input", "rate_limited", "error"] as const) {
      expect(
        sessionInScope(session(attention, { attention }), "attention"),
        attention,
      ).toBe(true);
    }
    expect(
      sessionInScope(session("done", { attention: "completed" }), "attention"),
    ).toBe(false);
    expect(sessionInScope(session("quiet"), "attention")).toBe(false);
  });

  it("stalled scope follows the two statuses that can be manually resumed", () => {
    expect(
      sessionInScope(session("i", { status: "interrupted" }), "stalled"),
    ).toBe(true);
    expect(
      sessionInScope(session("s", { status: "suspended" }), "stalled"),
    ).toBe(true);
    expect(sessionInScope(session("a", { status: "active" }), "stalled")).toBe(
      false,
    );
  });

  it("sorts human attention before status and recent activity", () => {
    const ordered = sortSessions([
      session("active"),
      session("error", { attention: "error", status: "interrupted" }),
      session("input", { attention: "needs_input", status: "active" }),
      session("rate", { attention: "rate_limited", status: "suspended" }),
    ]);

    expect(ordered.map((item) => item.id)).toEqual([
      "input",
      "error",
      "rate",
      "active",
    ]);
  });

  it("combines scope and search instead of letting one replace the other", () => {
    const sessions = [
      session("alpha", { attention: "needs_input" }),
      session("beta", { attention: "needs_input" }),
      session("alpha-quiet"),
    ];

    expect(
      selectVisibleSessions(sessions, "attention", "alpha").map((item) => item.id),
    ).toEqual(["alpha"]);
  });
});
