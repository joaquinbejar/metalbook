//! Client SDK for metalbook: instruction builders for every instruction, PDA helpers and
//! account decoders, built on the program's own layouts (`metalbook_core::accounts`).
//!
//! Instruction data is a one-byte tag followed by little-endian fields, exactly as each handler
//! in `metalbook-program/src/processor/` documents it; account lists follow the same order and
//! writability. The LiteSVM tests build every metalbook instruction through this crate.

pub mod decode;
pub mod ix;
pub mod pda;

pub use metalbook_core as core;
pub use solana_address::Address;

/// SPL Token.
pub const TOKEN_PROGRAM: Address =
    Address::from_str_const("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

/// The associated token account program.
pub const ATA_PROGRAM: Address =
    Address::from_str_const("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

/// The system program.
pub const SYSTEM_PROGRAM: Address = Address::from_str_const("11111111111111111111111111111111");

/// The upgradeable BPF loader.
pub const UPGRADEABLE_LOADER: Address =
    Address::from_str_const("BPFLoaderUpgradeab1e11111111111111111111111");

/// The Pyth receiver program (mainnet and devnet).
pub const PYTH_RECEIVER: Address =
    Address::from_str_const("rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ");
