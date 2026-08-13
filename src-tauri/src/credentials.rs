use crate::models::{KimiRegion, ProviderId};
use serde::{Deserialize, Serialize};
use std::fmt;

pub const CREDENTIAL_SERVICE: &str = "com.rupingliu.quotadock";
pub const DEEPSEEK_ACCOUNT: &str = "deepseek-api-key";
pub const KIMI_CHINA_ACCOUNT: &str = "kimi-cn-api-key";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialStoreErrorKind {
    NotFound,
    Unavailable,
    OperationFailed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CredentialStoreError {
    kind: CredentialStoreErrorKind,
}

impl CredentialStoreError {
    pub fn new(kind: CredentialStoreErrorKind) -> Self {
        Self { kind }
    }

    pub fn kind(self) -> CredentialStoreErrorKind {
        self.kind
    }
}

impl fmt::Debug for CredentialStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialStoreError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for CredentialStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self.kind {
            CredentialStoreErrorKind::NotFound => "未找到已保存的凭据。",
            CredentialStoreErrorKind::Unavailable => "系统凭据存储当前不可用。",
            CredentialStoreErrorKind::OperationFailed => "系统凭据存储操作失败。",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for CredentialStoreError {}

/// Narrow backend-only interface. Implementations receive a fixed account name,
/// never a user-controlled service or account identifier.
pub trait CredentialStore: Send + Sync {
    fn set_password(&self, account: &'static str, secret: &str)
        -> Result<(), CredentialStoreError>;
    fn get_password(&self, account: &'static str) -> Result<String, CredentialStoreError>;
    fn delete_credential(&self, account: &'static str) -> Result<(), CredentialStoreError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsCredentialStore;

#[cfg(windows)]
impl WindowsCredentialStore {
    fn entry(account: &'static str) -> Result<keyring::v1::Entry, CredentialStoreError> {
        keyring::v1::Entry::new(CREDENTIAL_SERVICE, account).map_err(map_keyring_error)
    }
}

#[cfg(windows)]
impl CredentialStore for WindowsCredentialStore {
    fn set_password(
        &self,
        account: &'static str,
        secret: &str,
    ) -> Result<(), CredentialStoreError> {
        Self::entry(account)?
            .set_password(secret)
            .map_err(map_keyring_error)
    }

    fn get_password(&self, account: &'static str) -> Result<String, CredentialStoreError> {
        Self::entry(account)?
            .get_password()
            .map_err(map_keyring_error)
    }

    fn delete_credential(&self, account: &'static str) -> Result<(), CredentialStoreError> {
        Self::entry(account)?
            .delete_credential()
            .map_err(map_keyring_error)
    }
}

#[cfg(windows)]
fn map_keyring_error(error: keyring::v1::Error) -> CredentialStoreError {
    use keyring::v1::Error;

    let kind = match error {
        Error::NoEntry => CredentialStoreErrorKind::NotFound,
        Error::NoDefaultStore | Error::NoStorageAccess(_) => CredentialStoreErrorKind::Unavailable,
        _ => CredentialStoreErrorKind::OperationFailed,
    };
    CredentialStoreError::new(kind)
}

#[cfg(not(windows))]
impl CredentialStore for WindowsCredentialStore {
    fn set_password(
        &self,
        _account: &'static str,
        _secret: &str,
    ) -> Result<(), CredentialStoreError> {
        Err(CredentialStoreError::new(
            CredentialStoreErrorKind::Unavailable,
        ))
    }

    fn get_password(&self, _account: &'static str) -> Result<String, CredentialStoreError> {
        Err(CredentialStoreError::new(
            CredentialStoreErrorKind::Unavailable,
        ))
    }

    fn delete_credential(&self, _account: &'static str) -> Result<(), CredentialStoreError> {
        Err(CredentialStoreError::new(
            CredentialStoreErrorKind::Unavailable,
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CredentialAvailability {
    Configured,
    NotConfigured,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialStatus {
    pub provider_id: ProviderId,
    pub region: Option<KimiRegion>,
    pub availability: CredentialAvailability,
}

pub type ProviderCredentialStatus = CredentialStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CredentialTarget {
    provider_id: ProviderId,
    region: Option<KimiRegion>,
    account: &'static str,
}

impl CredentialTarget {
    const DEEPSEEK: Self = Self {
        provider_id: ProviderId::DeepSeek,
        region: None,
        account: DEEPSEEK_ACCOUNT,
    };
    const KIMI_CHINA: Self = Self {
        provider_id: ProviderId::Kimi,
        region: Some(KimiRegion::China),
        account: KIMI_CHINA_ACCOUNT,
    };
    fn resolve(provider_id: ProviderId, region: Option<KimiRegion>) -> Result<Self, String> {
        match (provider_id, region) {
            (ProviderId::DeepSeek, None) => Ok(Self::DEEPSEEK),
            (ProviderId::Kimi, Some(KimiRegion::China)) => Ok(Self::KIMI_CHINA),
            (ProviderId::Kimi, None) => Err("Kimi 凭据必须指定国内区域。".to_string()),
            (ProviderId::DeepSeek, Some(_)) => Err("DeepSeek 凭据不接受区域参数。".to_string()),
            (ProviderId::Codex, _) => Err("Codex 不使用 API Key 凭据。".to_string()),
        }
    }

    fn status(self, availability: CredentialAvailability) -> CredentialStatus {
        CredentialStatus {
            provider_id: self.provider_id,
            region: self.region,
            availability,
        }
    }
}

pub fn set_provider_credential_with_store<S: CredentialStore>(
    store: &S,
    provider_id: ProviderId,
    region: Option<KimiRegion>,
    secret: &str,
) -> Result<CredentialStatus, String> {
    let target = CredentialTarget::resolve(provider_id, region)?;
    let secret = secret.trim();
    if secret.is_empty() {
        return Err("API Key 不能为空。".to_string());
    }

    store
        .set_password(target.account, secret)
        .map_err(|error| error.to_string())?;
    Ok(target.status(CredentialAvailability::Configured))
}

pub fn delete_provider_credential_with_store<S: CredentialStore>(
    store: &S,
    provider_id: ProviderId,
    region: Option<KimiRegion>,
) -> Result<CredentialStatus, String> {
    let target = CredentialTarget::resolve(provider_id, region)?;
    store
        .delete_credential(target.account)
        .map_err(|error| error.to_string())?;
    Ok(target.status(CredentialAvailability::NotConfigured))
}

pub fn provider_credential_status_with_store<S: CredentialStore>(
    store: &S,
) -> Vec<ProviderCredentialStatus> {
    [CredentialTarget::DEEPSEEK, CredentialTarget::KIMI_CHINA]
        .into_iter()
        .map(|target| {
            let availability = match store.get_password(target.account) {
                Ok(_) => CredentialAvailability::Configured,
                Err(error) if error.kind() == CredentialStoreErrorKind::NotFound => {
                    CredentialAvailability::NotConfigured
                }
                Err(_) => CredentialAvailability::Unavailable,
            };
            target.status(availability)
        })
        .collect()
}

/// Backend-only secret access for provider refresh code. This function must never be
/// exposed as a Tauri command.
pub fn load_provider_credential<S: CredentialStore>(
    store: &S,
    provider_id: ProviderId,
    region: Option<KimiRegion>,
) -> Result<String, CredentialStoreError> {
    let target = CredentialTarget::resolve(provider_id, region)
        .map_err(|_| CredentialStoreError::new(CredentialStoreErrorKind::OperationFailed))?;
    store.get_password(target.account)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemoryCredentialStore {
        entries: Mutex<HashMap<&'static str, String>>,
        failure: Mutex<Option<CredentialStoreErrorKind>>,
    }

    impl MemoryCredentialStore {
        fn fail_with(&self, kind: CredentialStoreErrorKind) {
            *self.failure.lock().unwrap() = Some(kind);
        }

        fn maybe_fail(&self) -> Result<(), CredentialStoreError> {
            match *self.failure.lock().unwrap() {
                Some(kind) => Err(CredentialStoreError::new(kind)),
                None => Ok(()),
            }
        }
    }

    impl CredentialStore for MemoryCredentialStore {
        fn set_password(
            &self,
            account: &'static str,
            secret: &str,
        ) -> Result<(), CredentialStoreError> {
            self.maybe_fail()?;
            self.entries
                .lock()
                .unwrap()
                .insert(account, secret.to_string());
            Ok(())
        }

        fn get_password(&self, account: &'static str) -> Result<String, CredentialStoreError> {
            self.maybe_fail()?;
            self.entries
                .lock()
                .unwrap()
                .get(account)
                .cloned()
                .ok_or_else(|| CredentialStoreError::new(CredentialStoreErrorKind::NotFound))
        }

        fn delete_credential(&self, account: &'static str) -> Result<(), CredentialStoreError> {
            self.maybe_fail()?;
            self.entries
                .lock()
                .unwrap()
                .remove(account)
                .map(|_| ())
                .ok_or_else(|| CredentialStoreError::new(CredentialStoreErrorKind::NotFound))
        }
    }

    #[test]
    fn fixed_accounts_are_distinct() {
        assert_eq!(
            CredentialTarget::resolve(ProviderId::DeepSeek, None)
                .unwrap()
                .account,
            DEEPSEEK_ACCOUNT
        );
        assert_eq!(
            CredentialTarget::resolve(ProviderId::Kimi, Some(KimiRegion::China))
                .unwrap()
                .account,
            KIMI_CHINA_ACCOUNT
        );
        assert_ne!(DEEPSEEK_ACCOUNT, KIMI_CHINA_ACCOUNT);
    }

    #[test]
    fn memory_store_sets_replaces_and_deletes_without_cross_provider_access() {
        let store = MemoryCredentialStore::default();

        set_provider_credential_with_store(&store, ProviderId::DeepSeek, None, " ds-one ").unwrap();
        set_provider_credential_with_store(
            &store,
            ProviderId::Kimi,
            Some(KimiRegion::China),
            "kimi-one",
        )
        .unwrap();
        set_provider_credential_with_store(&store, ProviderId::DeepSeek, None, "ds-two").unwrap();

        assert_eq!(
            load_provider_credential(&store, ProviderId::DeepSeek, None).unwrap(),
            "ds-two"
        );
        assert_eq!(
            load_provider_credential(&store, ProviderId::Kimi, Some(KimiRegion::China)).unwrap(),
            "kimi-one"
        );

        delete_provider_credential_with_store(&store, ProviderId::DeepSeek, None).unwrap();
        assert!(load_provider_credential(&store, ProviderId::DeepSeek, None).is_err());
        assert_eq!(
            load_provider_credential(&store, ProviderId::Kimi, Some(KimiRegion::China)).unwrap(),
            "kimi-one"
        );
    }

    #[test]
    fn statuses_only_expose_configuration_state() {
        let store = MemoryCredentialStore::default();
        let secret = "secret-that-must-not-escape";
        set_provider_credential_with_store(&store, ProviderId::DeepSeek, None, secret).unwrap();

        let statuses = provider_credential_status_with_store(&store);
        let serialized = serde_json::to_string(&statuses).unwrap();
        let debug = format!("{statuses:?}");

        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].availability, CredentialAvailability::Configured);
        assert_eq!(
            statuses[1].availability,
            CredentialAvailability::NotConfigured
        );
        assert!(!serialized.contains(secret));
        assert!(!debug.contains(secret));
        assert!(!serialized.contains("apiKey"));
    }

    #[test]
    fn unavailable_store_is_reported_without_platform_details() {
        let store = MemoryCredentialStore::default();
        store.fail_with(CredentialStoreErrorKind::Unavailable);

        let statuses = provider_credential_status_with_store(&store);
        let error = set_provider_credential_with_store(
            &store,
            ProviderId::DeepSeek,
            None,
            "never-print-me",
        )
        .unwrap_err();

        assert!(statuses
            .iter()
            .all(|status| status.availability == CredentialAvailability::Unavailable));
        assert_eq!(error, "系统凭据存储当前不可用。");
        assert!(!error.contains("never-print-me"));
    }

    #[test]
    fn deleting_a_missing_entry_returns_a_sanitized_error() {
        let store = MemoryCredentialStore::default();

        let error =
            delete_provider_credential_with_store(&store, ProviderId::DeepSeek, None).unwrap_err();

        assert_eq!(error, "未找到已保存的凭据。");
    }

    #[test]
    fn invalid_targets_and_empty_secrets_never_reach_the_store() {
        let store = MemoryCredentialStore::default();

        assert!(
            set_provider_credential_with_store(&store, ProviderId::Codex, None, "secret").is_err()
        );
        assert!(
            set_provider_credential_with_store(&store, ProviderId::DeepSeek, None, "  ").is_err()
        );
        assert!(
            set_provider_credential_with_store(&store, ProviderId::Kimi, None, "secret").is_err()
        );
        assert!(store.entries.lock().unwrap().is_empty());
    }
}
