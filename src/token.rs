use crate::config::AgentConfig;
use crate::runtime::TokenRecovery;
use crate::transport::TransportError;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone)]
pub struct SharedAgentToken {
    inner: Arc<RwLock<String>>,
}

impl SharedAgentToken {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(token.into())),
        }
    }

    pub fn get(&self) -> String {
        self.inner
            .read()
            .map(|token| token.clone())
            .unwrap_or_default()
    }

    pub fn set(&self, token: impl Into<String>) {
        if let Ok(mut current) = self.inner.write() {
            *current = token.into();
        }
    }
}

pub struct SharedTokenRecovery<R> {
    inner: R,
    shared_token: SharedAgentToken,
}

impl<R> SharedTokenRecovery<R> {
    pub fn new(inner: R, shared_token: SharedAgentToken) -> Self {
        Self {
            inner,
            shared_token,
        }
    }
}

impl<R> TokenRecovery for SharedTokenRecovery<R>
where
    R: TokenRecovery,
{
    fn recover_from_transport_error(
        &mut self,
        config: &mut AgentConfig,
        error: &TransportError,
    ) -> bool {
        let recovered = self.inner.recover_from_transport_error(config, error);
        if recovered {
            self.shared_token.set(config.token.clone());
            if let TransportError::InvalidClientToken { operation, .. } = error {
                println!("{}", token_recovered_smoke_line(operation));
            }
        }
        recovered
    }
}

pub fn token_recovered_smoke_line(operation: &str) -> String {
    let operation = operation.trim().replace(' ', "_");
    crate::smoke_summary::smoke_event_line("token_recovered", &[("operation", &operation)])
}
