//! Posting a Hermes update as a `Full` `PriceUpdateV2` through Wormhole and the Pyth receiver
//! (milestone 7b-2), as the receiver's own CLI does (`pyth-crosschain`,
//! `target_chains/solana/cli/src/main.rs`, `process_write_encoded_vaa_and_post_price_update`):
//!
//! 1. create an account owned by the Wormhole core bridge, `init_encoded_vaa`, write the VAA
//!    with `write_encoded_vaa`, and `verify_encoded_vaa_v1` against the guardian set the VAA
//!    names;
//! 2. the receiver's `post_update` with the feed's Merkle message and proof, which checks the
//!    encoded VAA is verified and writes a `PriceUpdateV2` with `VerificationLevel::Full`.
//!
//! [`post_instructions`] adds metalbook's `sample_settlement` after `post_update` and then
//! returns the rent of both temporary accounts (`reclaim_rent`, `close_encoded_vaa`), so a post
//! leaves nothing behind. Layouts, from the sources:
//!
//! - the Hermes binary update (`pythnet_sdk::wire::v1::AccumulatorUpdateData`, Pyth's own
//!   big-endian format with `u8` sequence lengths): `"PNAU"`, major 1, minor, a `u8`-prefixed
//!   trailing header, proof variant 0 (`WormholeMerkle`), the VAA with a `u16` length, then a
//!   `u8` count of updates, each a `u16`-prefixed message and a `u8` count of 20-byte nodes;
//! - Anchor instruction data: `sha256("global:<name>")[..8]` and Borsh arguments (`u32`
//!   little-endian lengths);
//! - an encoded VAA account holds the VAA from byte [`VAA_START`] (discriminator 8, header 34:
//!   status, write authority, version; length 4);
//! - the guardian set is `["GuardianSet", index as u32 big-endian]` under Wormhole, the
//!   receiver's config `["config"]` and its fee treasury `["treasury", [id]]`.

use solana_address::Address;
use solana_instruction::{AccountMeta, Instruction};

use crate::KeeperError;

/// The Pyth receiver program (mainnet and devnet).
pub const RECEIVER: Address =
    Address::from_str_const("rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ");

/// The Wormhole core bridge the receiver's config names (mainnet and devnet).
pub const WORMHOLE: Address =
    Address::from_str_const("HDwcJBJXjL9FpJ7UBsYBtaDjsBUhuLCUYoz3zr8SWWaQ");

/// The system program.
const SYSTEM_PROGRAM: Address = Address::from_str_const("11111111111111111111111111111111");

/// Anchor account discriminator of an encoded VAA, `sha256("account:EncodedVaa")[..8]`.
pub const ENCODED_VAA_DISCRIMINATOR: [u8; 8] = [226, 101, 163, 4, 133, 160, 84, 245];

/// Offset of the write authority in an encoded VAA (after the discriminator and the status).
pub const ENCODED_VAA_AUTHORITY_OFFSET: usize = 9;

/// Offset of the VAA in an encoded VAA account.
pub const VAA_START: usize = 46;

/// Longest VAA written in one `write_encoded_vaa` (the receiver CLI's `VAA_SPLIT_INDEX`); a
/// longer one would not fit the first transaction. Hermes' VAAs are about 300 bytes.
pub const MAX_VAA_LEN: usize = 755;

/// The receiver's fee treasury used (`DEFAULT_TREASURY_ID`).
pub const TREASURY_ID: u8 = 0;

/// Anchor discriminators, `sha256("global:<name>")[..8]`.
mod disc {
    pub const INIT_ENCODED_VAA: [u8; 8] = [209, 193, 173, 25, 91, 202, 181, 218];
    pub const WRITE_ENCODED_VAA: [u8; 8] = [199, 208, 110, 177, 150, 76, 118, 42];
    pub const VERIFY_ENCODED_VAA_V1: [u8; 8] = [103, 56, 177, 229, 240, 103, 68, 73];
    pub const CLOSE_ENCODED_VAA: [u8; 8] = [48, 221, 174, 198, 231, 7, 152, 38];
    pub const POST_UPDATE: [u8; 8] = [133, 95, 207, 175, 11, 79, 118, 44];
    pub const RECLAIM_RENT: [u8; 8] = [218, 200, 19, 197, 227, 89, 192, 22];
}

/// One feed's message in an accumulator update, with its Merkle proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MerkleUpdate {
    /// The price message (`PriceFeedMessage`, Pyth's wire format).
    pub message: Vec<u8>,
    /// The Merkle path, 20-byte Keccak-160 nodes.
    pub proof: Vec<[u8; 20]>,
}

impl MerkleUpdate {
    /// The feed id of a price message (type byte 0, then the 32-byte id), if it is one.
    #[must_use]
    pub fn feed_id(&self) -> Option<[u8; 32]> {
        match self.message.split_first() {
            Some((0, rest)) => rest.get(..32).and_then(|id| id.try_into().ok()),
            _ => None,
        }
    }
}

impl MerkleUpdate {
    /// The print a `PriceFeedMessage` carries (Pyth's wire format, big-endian): type 0, feed id,
    /// price `i64`, conf `u64`, exponent `i32`, publish time `i64`, previous publish time `i64`,
    /// then the EMA fields.
    ///
    /// # Errors
    ///
    /// `KeeperError::Update` if the message is not a price message or is too short.
    pub fn print(&self) -> Result<crate::PrintMsg, KeeperError> {
        let m = &self.message;
        if m.first() != Some(&0) {
            return Err(KeeperError::Update("not a price message"));
        }
        let field = |at: usize| -> Result<[u8; 8], KeeperError> {
            m.get(at..at.saturating_add(8))
                .and_then(|b| b.try_into().ok())
                .ok_or(KeeperError::Update("price message too short"))
        };
        let expo: [u8; 4] = m
            .get(49..53)
            .and_then(|b| b.try_into().ok())
            .ok_or(KeeperError::Update("price message too short"))?;
        Ok(crate::PrintMsg {
            price: i64::from_be_bytes(field(33)?),
            conf: u64::from_be_bytes(field(41)?),
            expo: i32::from_be_bytes(expo),
            publish_time: i64::from_be_bytes(field(53)?),
            prev_publish_time: i64::from_be_bytes(field(61)?),
        })
    }
}

/// A parsed Hermes binary update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccumulatorUpdate {
    /// The guardian-signed VAA whose payload is the Merkle root.
    pub vaa: Vec<u8>,
    /// The feeds' messages and proofs.
    pub updates: Vec<MerkleUpdate>,
}

/// A cursor over the update bytes; every read is checked.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], KeeperError> {
        let (head, tail) = self
            .0
            .split_at_checked(n)
            .ok_or(KeeperError::Update("truncated"))?;
        self.0 = tail;
        Ok(head)
    }

    fn u8(&mut self) -> Result<u8, KeeperError> {
        self.take(1)?
            .first()
            .copied()
            .ok_or(KeeperError::Update("truncated"))
    }

    fn u16_be(&mut self) -> Result<usize, KeeperError> {
        let b: [u8; 2] = self
            .take(2)?
            .try_into()
            .map_err(|_| KeeperError::Update("truncated"))?;
        Ok(usize::from(u16::from_be_bytes(b)))
    }
}

impl AccumulatorUpdate {
    /// Parses a Hermes binary update.
    ///
    /// # Errors
    ///
    /// `KeeperError::Update` for a wrong magic, version or proof type, or truncated or trailing
    /// bytes.
    pub fn parse(bytes: &[u8]) -> Result<Self, KeeperError> {
        let mut r = Reader(bytes);
        if r.take(4)? != b"PNAU" {
            return Err(KeeperError::Update("not an accumulator update"));
        }
        if r.u8()? != 1 {
            return Err(KeeperError::Update("unsupported major version"));
        }
        let _minor = r.u8()?;
        let trailing = usize::from(r.u8()?);
        r.take(trailing)?;
        if r.u8()? != 0 {
            return Err(KeeperError::Update("not a Wormhole Merkle proof"));
        }
        let vaa_len = r.u16_be()?;
        let vaa = r.take(vaa_len)?.to_vec();
        let count = r.u8()?;
        let mut updates = Vec::with_capacity(usize::from(count));
        for _ in 0..count {
            let len = r.u16_be()?;
            let message = r.take(len)?.to_vec();
            let nodes = r.u8()?;
            let mut proof = Vec::with_capacity(usize::from(nodes));
            for _ in 0..nodes {
                let node: [u8; 20] = r
                    .take(20)?
                    .try_into()
                    .map_err(|_| KeeperError::Update("truncated"))?;
                proof.push(node);
            }
            updates.push(MerkleUpdate { message, proof });
        }
        if !r.0.is_empty() {
            return Err(KeeperError::Update("trailing bytes"));
        }
        Ok(Self { vaa, updates })
    }

    /// The guardian set index the VAA is signed by (bytes 1..5, big-endian, after the
    /// version byte 1).
    ///
    /// # Errors
    ///
    /// `KeeperError::Update` for a VAA that is not version 1 or is too short.
    pub fn guardian_set_index(&self) -> Result<u32, KeeperError> {
        match self.vaa.split_first() {
            Some((1, rest)) => rest
                .get(..4)
                .and_then(|b| b.try_into().ok())
                .map(u32::from_be_bytes)
                .ok_or(KeeperError::Update("VAA too short")),
            _ => Err(KeeperError::Update("not a version 1 VAA")),
        }
    }

    /// The update for `feed`.
    ///
    /// # Errors
    ///
    /// `KeeperError::Update` if the update does not carry `feed`.
    pub fn for_feed(&self, feed: &[u8; 32]) -> Result<&MerkleUpdate, KeeperError> {
        self.updates
            .iter()
            .find(|u| u.feed_id().as_ref() == Some(feed))
            .ok_or(KeeperError::Update("feed not in the update"))
    }
}

/// The compute budget program.
const COMPUTE_BUDGET: Address =
    Address::from_str_const("ComputeBudget111111111111111111111111111111");

/// `SetComputeUnitLimit` (compute budget instruction 2): the receiver CLI asks 600,000 for the
/// verify and post transaction.
#[must_use]
pub fn compute_unit_limit(units: u32) -> Instruction {
    let mut data = vec![2u8];
    data.extend_from_slice(&units.to_le_bytes());
    Instruction {
        program_id: COMPUTE_BUDGET,
        accounts: Vec::new(),
        data,
    }
}

/// `["GuardianSet", index]` under Wormhole.
#[must_use]
pub fn guardian_set(index: u32) -> Address {
    Address::find_program_address(&[b"GuardianSet", &index.to_be_bytes()], &WORMHOLE).0
}

/// The receiver's config, `["config"]`.
#[must_use]
pub fn receiver_config() -> Address {
    Address::find_program_address(&[b"config"], &RECEIVER).0
}

/// The receiver's fee treasury, `["treasury", [TREASURY_ID]]`.
#[must_use]
pub fn treasury() -> Address {
    Address::find_program_address(&[b"treasury", &[TREASURY_ID]], &RECEIVER).0
}

fn borsh_bytes(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), KeeperError> {
    let len = u32::try_from(bytes.len()).map_err(|_| KeeperError::Update("too long"))?;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}

/// The accounts the two transactions create, with the rent each needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostAccounts {
    /// Fee payer and write authority of both temporary accounts.
    pub payer: Address,
    /// A fresh keypair's address for the encoded VAA (signs the first transaction).
    pub encoded_vaa: Address,
    /// A fresh keypair's address for the `PriceUpdateV2` (signs the second transaction).
    pub price_update: Address,
    /// Rent-exempt lamports for `VAA_START + vaa.len()` bytes.
    pub encoded_vaa_rent: u64,
}

/// The two transactions of one post.
///
/// **If the second transaction fails after the first landed**, the verified encoded VAA stays
/// open with its rent (`VAA_START + vaa.len()` bytes, about 2.37 million lamports for a 292-byte
/// VAA) until a `close_encoded_vaa` signed by its write authority, the payer, returns it: no one
/// else can. The keeper's chain closes it right after the failure and keeps retrying the
/// orphans it knows about (milestone 7c).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostInstructions {
    /// Signed by the payer and the encoded VAA keypair: create, init, write, verify.
    pub verify: Vec<Instruction>,
    /// Signed by the payer and the price update keypair: `post_update`,
    /// `sample_settlement`, then `reclaim_rent` and `close_encoded_vaa`.
    pub post_and_sample: Vec<Instruction>,
}

/// `close_encoded_vaa`: returns the encoded VAA's rent to its write authority (used after a
/// post, and alone to clean up when the second transaction fails).
#[must_use]
pub fn close_encoded_vaa(write_authority: &Address, encoded_vaa: &Address) -> Instruction {
    Instruction {
        program_id: WORMHOLE,
        accounts: vec![
            AccountMeta::new(*write_authority, true),
            AccountMeta::new(*encoded_vaa, false),
        ],
        data: disc::CLOSE_ENCODED_VAA.to_vec(),
    }
}

/// Rent-exempt lamports for `size` bytes under the default rent (3,480 lamports per byte-year,
/// two years, 128 bytes of account overhead), which every cluster uses.
///
/// # Errors
///
/// `KeeperError::Update` on overflow.
pub fn rent_exempt(size: usize) -> Result<u64, KeeperError> {
    u64::try_from(size)
        .ok()
        .and_then(|s| s.checked_add(128))
        .and_then(|s| s.checked_mul(3_480 * 2))
        .ok_or(KeeperError::Update("account too large"))
}

/// The size of the encoded VAA account for `update`.
///
/// # Errors
///
/// `KeeperError::Update` on overflow.
pub fn encoded_vaa_size(update: &AccumulatorUpdate) -> Result<usize, KeeperError> {
    VAA_START
        .checked_add(update.vaa.len())
        .ok_or(KeeperError::Update("VAA too long"))
}

/// Builds the two transactions that post `feed` from `update` and sample `series` with it
/// (`sample` is metalbook's `sample_settlement` for that series and `a.price_update`).
///
/// # Errors
///
/// `KeeperError::Update` if the update does not carry `feed` or the VAA is malformed.
pub fn post_instructions(
    update: &AccumulatorUpdate,
    feed: &[u8; 32],
    a: &PostAccounts,
    sample: Instruction,
) -> Result<PostInstructions, KeeperError> {
    let merkle = update.for_feed(feed)?;
    if update.vaa.len() > MAX_VAA_LEN {
        return Err(KeeperError::Update("VAA longer than one write"));
    }
    let size = u64::try_from(encoded_vaa_size(update)?)
        .map_err(|_| KeeperError::Update("VAA too long"))?;

    // System program `CreateAccount` (index 0): lamports, space, owner.
    let mut create = 0u32.to_le_bytes().to_vec();
    create.extend_from_slice(&a.encoded_vaa_rent.to_le_bytes());
    create.extend_from_slice(&size.to_le_bytes());
    create.extend_from_slice(WORMHOLE.as_ref());
    let create = Instruction {
        program_id: SYSTEM_PROGRAM,
        accounts: vec![
            AccountMeta::new(a.payer, true),
            AccountMeta::new(a.encoded_vaa, true),
        ],
        data: create,
    };
    let authority_and_vaa = vec![
        AccountMeta::new_readonly(a.payer, true),
        AccountMeta::new(a.encoded_vaa, false),
    ];
    let init = Instruction {
        program_id: WORMHOLE,
        accounts: authority_and_vaa.clone(),
        data: disc::INIT_ENCODED_VAA.to_vec(),
    };
    let mut write = disc::WRITE_ENCODED_VAA.to_vec();
    write.extend_from_slice(&0u32.to_le_bytes());
    borsh_bytes(&mut write, &update.vaa)?;
    let write = Instruction {
        program_id: WORMHOLE,
        accounts: authority_and_vaa,
        data: write,
    };
    let verify = Instruction {
        program_id: WORMHOLE,
        accounts: vec![
            AccountMeta::new_readonly(a.payer, true),
            AccountMeta::new(a.encoded_vaa, false),
            AccountMeta::new_readonly(guardian_set(update.guardian_set_index()?), false),
        ],
        data: disc::VERIFY_ENCODED_VAA_V1.to_vec(),
    };

    // `post_update(PostUpdateParams { merkle_price_update: { message, proof }, treasury_id })`.
    let mut post = disc::POST_UPDATE.to_vec();
    borsh_bytes(&mut post, &merkle.message)?;
    // The proof is a Borsh `Vec<[u8; 20]>`: the node count, then the nodes.
    let nodes = u32::try_from(merkle.proof.len()).map_err(|_| KeeperError::Update("too long"))?;
    post.extend_from_slice(&nodes.to_le_bytes());
    for node in &merkle.proof {
        post.extend_from_slice(node);
    }
    post.push(TREASURY_ID);
    let post = Instruction {
        program_id: RECEIVER,
        accounts: vec![
            AccountMeta::new(a.payer, true),
            AccountMeta::new_readonly(a.encoded_vaa, false),
            AccountMeta::new_readonly(receiver_config(), false),
            AccountMeta::new(treasury(), false),
            AccountMeta::new(a.price_update, true),
            AccountMeta::new_readonly(SYSTEM_PROGRAM, false),
            AccountMeta::new_readonly(a.payer, true),
        ],
        data: post,
    };
    let reclaim = Instruction {
        program_id: RECEIVER,
        accounts: vec![
            AccountMeta::new(a.payer, true),
            AccountMeta::new(a.price_update, false),
        ],
        data: disc::RECLAIM_RENT.to_vec(),
    };
    let close = close_encoded_vaa(&a.payer, &a.encoded_vaa);
    Ok(PostInstructions {
        verify: vec![create, init, write, verify],
        post_and_sample: vec![post, sample, reclaim, close],
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::disallowed_macros
)]
mod tests {
    use super::*;

    /// A minimal update: one VAA of 6 bytes (version 1, guardian set 7, no signatures) and one
    /// price message for feed `[9; 32]` with a two-node proof.
    fn sample() -> Vec<u8> {
        let mut b = b"PNAU".to_vec();
        b.extend_from_slice(&[1, 0, 2, 0xaa, 0xbb, 0]); // major, minor, 2-byte trailer, proof 0
        let vaa = [1u8, 0, 0, 0, 7, 0];
        b.extend_from_slice(&(vaa.len() as u16).to_be_bytes());
        b.extend_from_slice(&vaa);
        b.push(1);
        let mut msg = vec![0u8];
        msg.extend_from_slice(&[9; 32]);
        msg.extend_from_slice(&[5; 4]);
        b.extend_from_slice(&(msg.len() as u16).to_be_bytes());
        b.extend_from_slice(&msg);
        b.push(2);
        b.extend_from_slice(&[1; 20]);
        b.extend_from_slice(&[2; 20]);
        b
    }

    #[test]
    fn parses_the_wire_format() {
        let u = AccumulatorUpdate::parse(&sample()).unwrap();
        assert_eq!(u.vaa, vec![1, 0, 0, 0, 7, 0]);
        assert_eq!(u.guardian_set_index().unwrap(), 7);
        assert_eq!(u.updates.len(), 1);
        assert_eq!(u.updates[0].feed_id(), Some([9; 32]));
        assert_eq!(u.updates[0].proof, vec![[1; 20], [2; 20]]);
        assert!(u.for_feed(&[9; 32]).is_ok());
        assert!(u.for_feed(&[8; 32]).is_err());
    }

    #[test]
    fn rejects_malformed_updates() {
        let good = sample();
        let mut magic = good.clone();
        magic[0] = b'X';
        let mut major = good.clone();
        major[4] = 2;
        let mut proof_type = good.clone();
        proof_type[9] = 1;
        let mut trailing = good.clone();
        trailing.push(0);
        for bad in [magic, major, proof_type, trailing] {
            assert!(AccumulatorUpdate::parse(&bad).is_err());
        }
        for cut in 0..good.len() {
            assert!(AccumulatorUpdate::parse(&good[..cut]).is_err(), "{cut}");
        }
    }

    #[test]
    fn post_update_data_is_borsh() {
        let u = AccumulatorUpdate::parse(&sample()).unwrap();
        let a = PostAccounts {
            payer: Address::new_from_array([1; 32]),
            encoded_vaa: Address::new_from_array([2; 32]),
            price_update: Address::new_from_array([3; 32]),
            encoded_vaa_rent: 10,
        };
        let sample_ix = compute_unit_limit(1);
        let p = post_instructions(&u, &[9; 32], &a, sample_ix.clone()).unwrap();
        let post = &p.post_and_sample[0].data;
        let msg = &u.updates[0].message;
        assert_eq!(post[..8], disc::POST_UPDATE);
        assert_eq!(post[8..12], (msg.len() as u32).to_le_bytes());
        assert_eq!(&post[12..12 + msg.len()], msg.as_slice());
        let at = 12 + msg.len();
        assert_eq!(post[at..at + 4], 2u32.to_le_bytes());
        assert_eq!(post.len(), at + 4 + 40 + 1);
        assert_eq!(post[post.len() - 1], TREASURY_ID);
        assert_eq!(p.post_and_sample[1], sample_ix);
        // The create instruction sizes the account for the VAA.
        let create = &p.verify[0].data;
        assert_eq!(create[12..20], ((VAA_START + 6) as u64).to_le_bytes());
        assert!(post_instructions(&u, &[8; 32], &a, sample_ix).is_err());
    }
}
