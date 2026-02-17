use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "keeper",
    about = "Encrypted storage for cryptocurrency private keys and seed phrases",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize a new vault with a master password
    Init,

    /// Store an existing private key or seed phrase
    Add,

    /// List all stored entries (optionally filter by type: privatekey, seedphrase, password)
    List {
        /// Filter by entry type (e.g. "password", "privatekey", "seedphrase")
        #[arg(name = "filter")]
        filter: Option<String>,
    },

    /// View entry details and optionally reveal the secret
    View {
        /// Name or index number of the entry
        name: String,
    },

    /// Edit an existing entry's fields
    Edit {
        /// Name or index number of the entry
        name: String,
    },

    /// Rename an entry
    Rename {
        /// Current name or index number of the entry
        old_name: String,
        /// New name for the entry
        new_name: String,
    },

    /// Delete an entry (with confirmation)
    Delete {
        /// Name or index number of the entry
        name: String,
    },

    /// Copy a secret to the clipboard (auto-clears after 10 seconds)
    Copy {
        /// Name or index number of the entry
        name: String,
    },

    /// Search entries by name, network, or notes
    Search {
        /// Search query
        query: String,
    },

    /// Export vault as an encrypted backup (creates backup.ck in the specified directory)
    Export {
        /// Directory path where backup.ck will be created
        directory: String,
    },

    /// Import entries from an encrypted backup
    Import {
        /// Backup file path
        file: String,
    },

    /// Change the master password
    Passwd,

    /// Recover vault access using your recovery question
    Recover,

    /// View or change configuration settings
    Config {
        /// Display current configuration
        #[arg(long)]
        show: bool,

        /// Set clipboard auto-clear timeout in seconds
        #[arg(long)]
        clipboard_timeout: Option<u64>,
    },

    /// Derive and save the public address for an entry from its private key or seed phrase
    Derive {
        /// Name or index number of the entry
        name: String,
    },
}
