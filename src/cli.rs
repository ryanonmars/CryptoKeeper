use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "keeper",
    about = "Encrypted storage for cryptocurrency private keys and seed phrases",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize a new vault with a master password
    Init,

    /// Store an existing private key or seed phrase
    Add,

    /// List all stored entries
    List,

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

    /// Export vault as an encrypted backup
    Export {
        /// Output file path
        file: String,
    },

    /// Import entries from an encrypted backup
    Import {
        /// Backup file path
        file: String,
    },

    /// Change the master password
    Passwd,
}
