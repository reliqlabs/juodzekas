use mob::{ChainConfig, MobError, RustSigner};
use std::sync::Arc;

use mob::Client;

#[derive(Debug)]
pub enum WalletError {
    MnemonicError(String),
    ClientError(String),
}

impl std::fmt::Display for WalletError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WalletError::MnemonicError(e) => write!(f, "Mnemonic error: {e}"),
            WalletError::ClientError(e) => write!(f, "Client error: {e}"),
        }
    }
}

impl std::error::Error for WalletError {}

impl From<MobError> for WalletError {
    fn from(e: MobError) -> Self {
        WalletError::ClientError(e.to_string())
    }
}

pub struct Wallet {
    signer: Arc<RustSigner>,
    client: Option<Client>,
    address: String,
}

impl Wallet {
    /// Create a new wallet from a mnemonic phrase
    pub fn from_mnemonic(mnemonic: &str, prefix: &str) -> Result<Self, WalletError> {
        // RustSigner::from_mnemonic takes String arguments
        let signer = RustSigner::from_mnemonic(mnemonic.to_string(), prefix.to_string(), None)?;

        // address() returns String, not Result
        let address = signer.address();

        Ok(Self {
            signer: Arc::new(signer),
            client: None,
            address,
        })
    }

    /// Generate a new random wallet with a mnemonic
    /// mob doesn't provide generate_mnemonic, so we use bip39 crate
    pub fn generate(prefix: &str) -> Result<(Self, String), WalletError> {
        use bip39::Mnemonic;

        // Generate 24-word mnemonic
        let mnemonic = Mnemonic::generate(24)
            .map_err(|e| WalletError::MnemonicError(format!("Failed to generate: {e:?}")))?;

        let mnemonic_phrase = mnemonic.to_string();
        let wallet = Self::from_mnemonic(&mnemonic_phrase, prefix)?;

        Ok((wallet, mnemonic_phrase))
    }

    /// Connect wallet to blockchain RPC
    pub fn connect(
        &mut self,
        chain_id: &str,
        rpc_url: &str,
        prefix: &str,
    ) -> Result<(), WalletError> {
        let config = ChainConfig::new(
            chain_id.to_string(),
            rpc_url.to_string(),
            prefix.to_string(),
        );

        // Client::new is synchronous in mob (it creates its own runtime)
        let client = Client::new_with_signer(config, Arc::clone(&self.signer))?;

        self.client = Some(client);
        Ok(())
    }

    /// Get wallet address
    pub fn address(&self) -> &str {
        &self.address
    }

    /// Get the blockchain client (if connected)
    pub fn client(&self) -> Option<&Client> {
        self.client.as_ref()
    }

    /// Take the client out (for handing to a background thread).
    pub fn take_client(&mut self) -> Option<Client> {
        self.client.take()
    }

    /// Return the client after a background thread is done with it.
    pub fn set_client(&mut self, client: Client) {
        self.client = Some(client);
    }

    /// Get a clone of the signer Arc.
    pub fn signer(&self) -> Arc<RustSigner> {
        Arc::clone(&self.signer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wallet_from_mnemonic() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";
        let wallet = Wallet::from_mnemonic(mnemonic, "xion");
        assert!(wallet.is_ok());
    }

    #[test]
    fn test_wallet_generate() {
        let result = Wallet::generate("xion");
        assert!(result.is_ok());
        let (wallet, mnemonic) = result.unwrap();
        assert!(!mnemonic.is_empty());
        assert!(!wallet.address().is_empty());
    }

    #[test]
    fn test_invalid_mnemonic() {
        let wallet = Wallet::from_mnemonic("invalid mnemonic", "xion");
        assert!(wallet.is_err());
    }
}
