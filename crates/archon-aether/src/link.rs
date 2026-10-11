//! A reconnecting handle to Aether (ARCHITECTURE §16). The daemon's event
//! pump owns reconnection and calls [`AetherLink::replace`]; everyone else
//! asks for the current client and gets `Unavailable` while disconnected,
//! never a stale connection.

use std::sync::{Arc, RwLock};

use archon_core::error::ArchonError;

use crate::client::AetherClient;

#[derive(Debug, Default)]
pub struct AetherLink {
    current: RwLock<Option<Arc<AetherClient>>>,
}

impl AetherLink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn client(&self) -> Result<Arc<AetherClient>, ArchonError> {
        let guard = self
            .current
            .read()
            .map_err(|_| ArchonError::Unavailable("aether link lock poisoned".into()))?;
        let client = guard
            .clone()
            .ok_or_else(|| ArchonError::Unavailable("not connected to aetherd".into()))?;
        // Between the reader seeing the connection end and the pump replacing
        // it, the installed client is dead; say so rather than hand it out.
        match client.disconnect_reason() {
            Some(reason) => Err(ArchonError::Unavailable(reason)),
            None => Ok(client),
        }
    }

    /// `false` while disconnected, including before the pump notices a drop.
    pub fn is_connected(&self) -> bool {
        self.current
            .read()
            .is_ok_and(|guard| guard.as_ref().is_some_and(|client| client.is_connected()))
    }

    /// Install a fresh connection (`Some`) or mark the link down (`None`).
    pub fn replace(&self, client: Option<Arc<AetherClient>>) -> Result<(), ArchonError> {
        let mut guard = self
            .current
            .write()
            .map_err(|_| ArchonError::Unavailable("aether link lock poisoned".into()))?;
        *guard = client;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_link_is_disconnected_and_says_so() {
        let link = AetherLink::new();
        assert!(!link.is_connected());
        assert!(matches!(link.client(), Err(ArchonError::Unavailable(_))));
        link.replace(None).unwrap();
        assert!(!link.is_connected());
    }
}
