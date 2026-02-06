use mob::{ChainConfig, MobError, Signer};
use std::path::PathBuf;
use std::sync::Arc;

// Note: Client is behind rpc-client feature, which we need to enable
#[cfg(feature = "rpc-client")]
use mob::Client;

#[derive(Debug)]
pub enum WalletError {
    MnemonicError(String),
    FileError(String),
    ClientError(String),
}

impl std::fmt::Display for WalletError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WalletError::MnemonicError(e) => write!(f, "Mnemonic error: {}", e),
            WalletError::FileError(e) => write!(f, "File error: {}", e),
            WalletError::ClientError(e) => write!(f, "Client error: {}", e),
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
    signer: Arc<Signer>,
    #[cfg(feature = "rpc-client")]
    client: Option<Client>,
    address: String,
}

impl Wallet {
    /// Create a new wallet from a mnemonic phrase
    pub fn from_mnemonic(mnemonic: &str, prefix: &str) -> Result<Self, WalletError> {
        // Signer::from_mnemonic takes String arguments
        let signer = Signer::from_mnemonic(
            mnemonic.to_string(),
            prefix.to_string(),
            None,
        )?;

        // address() returns String, not Result
        let address = signer.address();

        Ok(Self {
            signer: Arc::new(signer),
            #[cfg(feature = "rpc-client")]
            client: None,
            address,
        })
    }

    /// Generate a new random wallet with a mnemonic
    /// mob doesn't provide generate_mnemonic, so we use bip39 crate
    pub fn generate(prefix: &str) -> Result<(Self, String), WalletError> {
        use bip39::{Mnemonic, Language};

        // Generate 24-word mnemonic
        let mnemonic = Mnemonic::generate(24)
            .map_err(|e| WalletError::MnemonicError(format!("Failed to generate: {:?}", e)))?;

        let mnemonic_phrase = mnemonic.to_string();
        let wallet = Self::from_mnemonic(&mnemonic_phrase, prefix)?;

        Ok((wallet, mnemonic_phrase))
    }

    /// Load wallet from a file containing mnemonic
    pub fn from_file(path: PathBuf, prefix: &str) -> Result<Self, WalletError> {
        let mnemonic = std::fs::read_to_string(&path)
            .map_err(|e| WalletError::FileError(e.to_string()))?;

        Self::from_mnemonic(mnemonic.trim(), prefix)
    }

    /// Save mnemonic to a file
    pub fn save_mnemonic(mnemonic: &str, path: PathBuf) -> Result<(), WalletError> {
        std::fs::write(&path, mnemonic)
            .map_err(|e| WalletError::FileError(e.to_string()))?;
        Ok(())
    }

    /// Connect wallet to blockchain RPC
    /// Note: This requires rpc-client feature to be enabled
    #[cfg(feature = "rpc-client")]
    pub fn connect(
        &mut self,
        chain_id: &str,
        rpc_url: &str,
        prefix: &str,
    ) -> Result<(), WalletError> {
        let config = ChainConfig::new(chain_id, rpc_url, prefix);

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
    #[cfg(feature = "rpc-client")]
    pub fn client(&self) -> Option<&Client> {
        self.client.as_ref()
    }

    /// Get mutable blockchain client (if connected)
    #[cfg(feature = "rpc-client")]
    pub fn client_mut(&mut self) -> Option<&mut Client> {
        self.client.as_mut()
    }

    /// Get the signer
    pub fn signer(&self) -> &Arc<Signer> {
        &self.signer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wallet_from_mnemonic() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
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
