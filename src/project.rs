use clap::ValueEnum;

/// A project that this tool can generate and analyze mutants for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Project {
    /// Bitcoin Core (https://github.com/bitcoin/bitcoin)
    #[value(name = "bitcoin-core")]
    BitcoinCore,
    /// libsecp256k1 (https://github.com/bitcoin-core/secp256k1)
    #[value(name = "secp256k1")]
    Secp256k1,
}

impl Default for Project {
    fn default() -> Self {
        Project::BitcoinCore
    }
}

impl Project {
    /// The upstream repository URL for this project.
    pub fn repository_url(&self) -> &'static str {
        match self {
            Project::BitcoinCore => "https://github.com/bitcoin/bitcoin",
            Project::Secp256k1 => "https://github.com/bitcoin-core/secp256k1",
        }
    }

    /// The name used for this project in the SQLite `projects` table.
    pub fn db_name(&self) -> &'static str {
        match self {
            Project::BitcoinCore => "Bitcoin Core",
            Project::Secp256k1 => "secp256k1",
        }
    }
}
