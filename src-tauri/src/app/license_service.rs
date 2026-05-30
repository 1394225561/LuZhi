use serde::{Deserialize, Serialize};

use crate::app::error::{AppError, AppResult};

const TRIAL_SECONDS: u64 = 14 * 24 * 60 * 60;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LicenseStatusKind {
    Trial,
    Expired,
    Activated,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenseStatus {
    pub kind: LicenseStatusKind,
    pub trial_days_remaining: u8,
    pub is_expired: bool,
    pub activated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct LocalTrialState {
    pub trial_started_at_secs: u64,
}

pub trait LicenseClock: Copy {
    fn now_secs(self) -> u64;
}

#[derive(Clone, Copy)]
pub struct SystemLicenseClock;

impl LicenseClock for SystemLicenseClock {
    fn now_secs(self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0)
    }
}

pub trait TrialStore {
    fn read(&mut self) -> AppResult<Option<LocalTrialState>>;
    fn write(&mut self, state: &LocalTrialState) -> AppResult<()>;
}

pub trait ActivationCredentialStore {
    fn activated_at_secs(&mut self) -> AppResult<Option<u64>>;
}

/// File-backed store is acceptable only for the non-secret local trial marker.
pub struct FileTrialStore {
    path: std::path::PathBuf,
}

impl FileTrialStore {
    pub fn new(path: std::path::PathBuf) -> Self {
        Self { path }
    }
}

impl TrialStore for FileTrialStore {
    fn read(&mut self) -> AppResult<Option<LocalTrialState>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let json =
            std::fs::read_to_string(&self.path).map_err(|error| AppError::LicenseFailed {
                reason: format!("读取本地授权状态失败: {error}"),
            })?;
        serde_json::from_str(&json)
            .map(Some)
            .map_err(|error| AppError::LicenseFailed {
                reason: format!("解析本地授权状态失败: {error}"),
            })
    }

    fn write(&mut self, state: &LocalTrialState) -> AppResult<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| AppError::LicenseFailed {
                reason: format!("创建授权状态目录失败: {error}"),
            })?;
        }
        let json =
            serde_json::to_string_pretty(state).map_err(|error| AppError::LicenseFailed {
                reason: format!("序列化授权状态失败: {error}"),
            })?;
        std::fs::write(&self.path, json).map_err(|error| AppError::LicenseFailed {
            reason: format!("写入授权状态失败: {error}"),
        })
    }
}

/// Phase 6 product default: no server activation protocol yet, so this returns
/// no entitlement. It preserves the command/interface boundary without storing
/// fake activation in a file.
pub struct NoopActivationCredentialStore;

impl ActivationCredentialStore for NoopActivationCredentialStore {
    fn activated_at_secs(&mut self) -> AppResult<Option<u64>> {
        Ok(None)
    }
}

pub struct LicenseService<'a, T, A, C>
where
    T: TrialStore,
    A: ActivationCredentialStore,
    C: LicenseClock,
{
    trial_store: &'a mut T,
    activation_store: &'a mut A,
    clock: C,
}

impl<'a, T, A, C> LicenseService<'a, T, A, C>
where
    T: TrialStore,
    A: ActivationCredentialStore,
    C: LicenseClock,
{
    pub fn new(trial_store: &'a mut T, activation_store: &'a mut A, clock: C) -> Self {
        Self {
            trial_store,
            activation_store,
            clock,
        }
    }

    pub fn status(self) -> AppResult<LicenseStatus> {
        let now = self.clock.now_secs();
        let state = match self.trial_store.read()? {
            Some(state) => state,
            None => {
                let state = LocalTrialState {
                    trial_started_at_secs: now,
                };
                self.trial_store.write(&state)?;
                state
            }
        };

        if self.activation_store.activated_at_secs()?.is_some() {
            return Ok(LicenseStatus {
                kind: LicenseStatusKind::Activated,
                trial_days_remaining: 0,
                is_expired: false,
                activated: true,
            });
        }

        let elapsed = now.saturating_sub(state.trial_started_at_secs);
        if elapsed >= TRIAL_SECONDS {
            return Ok(LicenseStatus {
                kind: LicenseStatusKind::Expired,
                trial_days_remaining: 0,
                is_expired: true,
                activated: false,
            });
        }

        let remaining = TRIAL_SECONDS - elapsed;
        let days = ((remaining + 24 * 60 * 60 - 1) / (24 * 60 * 60)).min(14) as u8;
        Ok(LicenseStatus {
            kind: LicenseStatusKind::Trial,
            trial_days_remaining: days,
            is_expired: false,
            activated: false,
        })
    }
}

#[cfg(test)]
#[derive(Clone, Copy)]
struct FixedClock {
    now_secs: u64,
}

#[cfg(test)]
impl LicenseClock for FixedClock {
    fn now_secs(self) -> u64 {
        self.now_secs
    }
}

#[cfg(test)]
#[derive(Default)]
struct MemoryTrialStore {
    state: Option<LocalTrialState>,
}

#[cfg(test)]
impl TrialStore for MemoryTrialStore {
    fn read(&mut self) -> AppResult<Option<LocalTrialState>> {
        Ok(self.state.clone())
    }

    fn write(&mut self, state: &LocalTrialState) -> AppResult<()> {
        self.state = Some(state.clone());
        Ok(())
    }
}

#[cfg(test)]
#[derive(Default)]
struct MemoryActivationCredentialStore {
    activated_at_secs: Option<u64>,
}

#[cfg(test)]
impl ActivationCredentialStore for MemoryActivationCredentialStore {
    fn activated_at_secs(&mut self) -> AppResult<Option<u64>> {
        Ok(self.activated_at_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_status_starts_fourteen_day_trial() {
        let mut trial_store = MemoryTrialStore::default();
        let mut activation_store = MemoryActivationCredentialStore::default();
        let clock = FixedClock { now_secs: 1_000 };
        let service = LicenseService::new(&mut trial_store, &mut activation_store, clock);

        let status = service.status().unwrap();

        assert_eq!(status.kind, LicenseStatusKind::Trial);
        assert_eq!(status.trial_days_remaining, 14);
        assert!(!status.is_expired);
    }

    #[test]
    fn trial_expires_after_fourteen_days() {
        let mut trial_store = MemoryTrialStore {
            state: Some(LocalTrialState {
                trial_started_at_secs: 1_000,
            }),
        };
        let mut activation_store = MemoryActivationCredentialStore::default();
        let clock = FixedClock {
            now_secs: 1_000 + 15 * 24 * 60 * 60,
        };
        let service = LicenseService::new(&mut trial_store, &mut activation_store, clock);

        let status = service.status().unwrap();

        assert_eq!(status.kind, LicenseStatusKind::Expired);
        assert_eq!(status.trial_days_remaining, 0);
        assert!(status.is_expired);
    }

    #[test]
    fn activated_status_comes_from_credential_store_and_overrides_trial_expiry() {
        let mut trial_store = MemoryTrialStore {
            state: Some(LocalTrialState {
                trial_started_at_secs: 1_000,
            }),
        };
        let mut activation_store = MemoryActivationCredentialStore {
            activated_at_secs: Some(2_000),
        };
        let clock = FixedClock {
            now_secs: 1_000 + 30 * 24 * 60 * 60,
        };
        let service = LicenseService::new(&mut trial_store, &mut activation_store, clock);

        let status = service.status().unwrap();

        assert_eq!(status.kind, LicenseStatusKind::Activated);
        assert!(status.activated);
        assert!(!status.is_expired);
    }

    #[test]
    fn file_trial_state_does_not_contain_activation_entitlement() {
        let state = LocalTrialState {
            trial_started_at_secs: 1_000,
        };
        let json = serde_json::to_string(&state).unwrap();

        assert!(json.contains("trialStartedAtSecs"));
        assert!(!json.contains("activated"));
    }

    #[test]
    fn local_trial_state_rejects_activation_entitlement_field() {
        let json = r#"{"trialStartedAtSecs":1000,"activatedAtSecs":2000}"#;
        let error = serde_json::from_str::<LocalTrialState>(json).unwrap_err();

        assert!(error.to_string().contains("unknown field"));
    }
}
