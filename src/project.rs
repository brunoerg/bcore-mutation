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

    /// Returns true if the given changed-file path should be excluded from
    /// mutation. Documentation, tooling, benchmarks and other non-source
    /// files are not worth mutating, and the exact set differs per project
    /// because each repository has its own layout and auxiliary files.
    pub fn should_skip_file(&self, path: &str) -> bool {
        self.skip_substrings().iter().any(|s| path.contains(s))
            || self.skip_suffixes().iter().any(|s| path.ends_with(s))
    }

    /// Path substrings that mark a file as non-source for this project.
    fn skip_substrings(&self) -> &'static [&'static str] {
        match self {
            Project::BitcoinCore => &[
                "doc",
                "contrib",
                "fuzz",
                "bench",
                "util",
                "sanitizer_supressions",
                "test_framework.py",
            ],
            Project::Secp256k1 => &[
                // e.g. tests_impl and bench_impl
                "tests_",
                "bench_",
                "doc",
                "contrib",
                "examples",
                "ci",
                "tools",
                "bench",
            ],
        }
    }

    /// File suffixes that mark a file as non-source for this project.
    fn skip_suffixes(&self) -> &'static [&'static str] {
        match self {
            Project::BitcoinCore => &[".txt"],
            // CMakeLists.txt, *.md, configure.ac, Makefile.am, etc.
            Project::Secp256k1 => &[".txt", ".md", ".ac", ".am"],
        }
    }
}
