//! 续跑决策的纯核心。
//!
//! 平台脚本只负责“怎么投递”，监控循环只负责采集事实；这里负责回答两件事：
//! 1. 一份中断证据是否已经连续稳定到可以执行；
//! 2. 某条 transport 是否满足自动/手动动作的安全边界。
//!
//! 这层不读进程、不碰数据库、不操作窗口，因此策略可以用确定性的序列测试守住。

/// 一次续跑允许对桌面造成多大影响。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryPolicy {
    /// 自动续跑：只允许精确、后台、可验证的通道。
    BackgroundOnly,
    /// 用户明确点击：后台通道优先，必要时允许可见的前台降级。
    AllowForeground,
}

/// transport 能把输入定位到多精确。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetCertainty {
    Exact,
    Window,
    CurrentFocus,
    Unknown,
}

/// transport 对用户当前桌面的打扰程度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Background,
    ChangesTab,
    StealsFocus,
}

/// transport/会话能提供哪一级落地核验。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verification {
    ProtocolAck,
    Transcript,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransportCapability {
    pub target: TargetCertainty,
    pub visibility: Visibility,
    pub verification: Verification,
}

impl TransportCapability {
    /// 自动动作的硬门槛刻意比手动动作高：无人值守时，宁可延后也不能抢焦点盲敲。
    pub fn permits(self, policy: DeliveryPolicy) -> bool {
        match policy {
            DeliveryPolicy::BackgroundOnly => {
                self.target == TargetCertainty::Exact
                    && self.visibility == Visibility::Background
                    && self.verification != Verification::None
            }
            DeliveryPolicy::AllowForeground => self.target != TargetCertainty::Unknown,
        }
    }
}

/// 会话级的时序判定状态。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ResumeDecisionState {
    #[default]
    Observing,
    /// 已经看到中断，但还没达到连续稳定观测门槛。
    Suspected {
        evidence_hash: u64,
        observations: u32,
    },
    /// 同一版证据已稳定，可以进入协调队列。
    Eligible {
        decision_id: String,
        evidence_hash: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionObservation {
    Healthy,
    Suspicious,
    Confirmed { evidence_hash: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionTransition {
    pub state: ResumeDecisionState,
    /// 只有从未满足门槛变为满足门槛的那一刻为 true。
    pub became_eligible: bool,
}

/// 将本轮观测合并进会话级状态。
///
/// `required_observations` 就是配置里的“连续无活动次数”。旧实现把它错误地乘到秒数上，
/// 既没有连续观测，也无法在证据变化时重置。这里按它原本的产品语义执行。
pub fn reduce_decision(
    previous: &ResumeDecisionState,
    observation: DecisionObservation,
    required_observations: u32,
    session_generation: &str,
) -> DecisionTransition {
    let required = required_observations.max(1);
    let (state, became_eligible) = match observation {
        DecisionObservation::Healthy | DecisionObservation::Suspicious => {
            (ResumeDecisionState::Observing, false)
        }
        DecisionObservation::Confirmed { evidence_hash } => match previous {
            ResumeDecisionState::Eligible {
                decision_id,
                evidence_hash: previous_hash,
            } if *previous_hash == evidence_hash => (
                ResumeDecisionState::Eligible {
                    decision_id: decision_id.clone(),
                    evidence_hash,
                },
                false,
            ),
            ResumeDecisionState::Suspected {
                evidence_hash: previous_hash,
                observations,
            } if *previous_hash == evidence_hash => {
                let observations = observations.saturating_add(1);
                if observations >= required {
                    (
                        ResumeDecisionState::Eligible {
                            decision_id: decision_id(session_generation, evidence_hash),
                            evidence_hash,
                        },
                        true,
                    )
                } else {
                    (
                        ResumeDecisionState::Suspected {
                            evidence_hash,
                            observations,
                        },
                        false,
                    )
                }
            }
            _ if required == 1 => (
                ResumeDecisionState::Eligible {
                    decision_id: decision_id(session_generation, evidence_hash),
                    evidence_hash,
                },
                true,
            ),
            _ => (
                ResumeDecisionState::Suspected {
                    evidence_hash,
                    observations: 1,
                },
                false,
            ),
        },
    };

    DecisionTransition {
        state,
        became_eligible,
    }
}

pub fn decision_id(session_generation: &str, evidence_hash: u64) -> String {
    format!("{session_generation}:{evidence_hash:016x}")
}

impl ResumeDecisionState {
    pub fn eligible(&self) -> Option<(&str, u64)> {
        match self {
            Self::Eligible {
                decision_id,
                evidence_hash,
            } => Some((decision_id, *evidence_hash)),
            _ => None,
        }
    }

    pub fn observation_progress(&self) -> Option<u32> {
        match self {
            Self::Suspected { observations, .. } => Some(*observations),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_delivery_requires_exact_background_verified_transport() {
        let safe = TransportCapability {
            target: TargetCertainty::Exact,
            visibility: Visibility::Background,
            verification: Verification::Transcript,
        };
        assert!(safe.permits(DeliveryPolicy::BackgroundOnly));

        for unsafe_capability in [
            TransportCapability {
                target: TargetCertainty::Window,
                ..safe
            },
            TransportCapability {
                visibility: Visibility::StealsFocus,
                ..safe
            },
            TransportCapability {
                verification: Verification::None,
                ..safe
            },
        ] {
            assert!(!unsafe_capability.permits(DeliveryPolicy::BackgroundOnly));
            assert!(unsafe_capability.permits(DeliveryPolicy::AllowForeground));
        }
    }

    #[test]
    fn confirmed_evidence_must_be_stable_for_consecutive_observations() {
        let first = reduce_decision(
            &ResumeDecisionState::Observing,
            DecisionObservation::Confirmed { evidence_hash: 7 },
            3,
            "cx-session",
        );
        assert_eq!(first.state.observation_progress(), Some(1));
        assert!(!first.became_eligible);

        let second = reduce_decision(
            &first.state,
            DecisionObservation::Confirmed { evidence_hash: 7 },
            3,
            "cx-session",
        );
        assert_eq!(second.state.observation_progress(), Some(2));

        let third = reduce_decision(
            &second.state,
            DecisionObservation::Confirmed { evidence_hash: 7 },
            3,
            "cx-session",
        );
        assert!(third.became_eligible);
        assert_eq!(third.state.eligible().map(|(_, hash)| hash), Some(7));
    }

    #[test]
    fn evidence_change_and_counter_evidence_reset_stability() {
        let suspected = ResumeDecisionState::Suspected {
            evidence_hash: 7,
            observations: 2,
        };
        let changed = reduce_decision(
            &suspected,
            DecisionObservation::Confirmed { evidence_hash: 8 },
            3,
            "cx-session",
        );
        assert_eq!(changed.state.observation_progress(), Some(1));

        let healthy = reduce_decision(
            &changed.state,
            DecisionObservation::Healthy,
            3,
            "cx-session",
        );
        assert_eq!(healthy.state, ResumeDecisionState::Observing);
    }

    #[test]
    fn eligible_decision_is_stable_for_same_evidence() {
        let eligible = ResumeDecisionState::Eligible {
            decision_id: "cx:0007".into(),
            evidence_hash: 7,
        };
        let next = reduce_decision(
            &eligible,
            DecisionObservation::Confirmed { evidence_hash: 7 },
            3,
            "cx-session",
        );
        assert_eq!(next.state, eligible);
        assert!(!next.became_eligible);
    }
}
