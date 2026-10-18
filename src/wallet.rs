//! The wallet that pays for hosted voices (docs/LIP_SYNC.md §8.3).
//!
//! hvym-img-tools charges for GPU time over x402 on Stellar, with no free tier,
//! exactly as it charges Inkternity (hvym-img-tools docs/X402_BILLING.md and
//! CLIENT.md). This is Keyternity's half, built to the same wire contract:
//! - **Identity:** an ed25519 Stellar key signs every hosted call
//!   (`X-Ink-Pubkey`, `X-Ink-Auth`). It's made from 12 BIP-39 words by SEP-0005
//!   (account 0), as Inkternity makes its own, so the same words restore the same
//!   wallet in either app.
//! - **Payment:** a `402` carries a quote. The wallet pays it from its own account
//!   (USDC, or XLM) with the quote's memo, submits the transaction to Horizon
//!   itself, and hands the proxy the transaction's hash. HEAVYMETA never holds
//!   anyone's funds.
//!
//! Transactions are encoded here (XDR) rather than through the `stellar` CLI
//! Inkternity runs: two operations' worth of encoding is smaller than a CLI to
//! install, and tests cross-check it against stellar_sdk's (tests/all/wallet.rs).

use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------- strkeys

const VERSION_ACCOUNT: u8 = 6 << 3; // 'G'
const VERSION_SEED: u8 = 18 << 3; // 'S'
const BASE32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn crc16_xmodem(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

/// A Stellar strkey: version byte, 32 bytes, CRC16 (little-endian), in base32.
pub fn encode_strkey(version: u8, key: &[u8; 32]) -> String {
    let mut raw = vec![version];
    raw.extend_from_slice(key);
    let crc = crc16_xmodem(&raw);
    raw.extend_from_slice(&crc.to_le_bytes());
    let (mut out, mut bits, mut acc) = (String::new(), 0u32, 0u32);
    for b in raw {
        acc = (acc << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(BASE32[((acc >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(BASE32[((acc << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// A strkey's 32 bytes, if it's well formed, of this version, and its checksum holds.
pub fn decode_strkey(version: u8, s: &str) -> Option<[u8; 32]> {
    let s = s.trim();
    if s.len() != 56 {
        return None;
    }
    let (mut raw, mut bits, mut acc) = (vec![], 0u32, 0u32);
    for c in s.bytes() {
        acc = (acc << 5) | BASE32.iter().position(|&b| b == c)? as u32;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            raw.push((acc >> bits) as u8);
        }
    }
    if raw.len() != 35 || raw[0] != version {
        return None;
    }
    let crc = crc16_xmodem(&raw[..33]).to_le_bytes();
    if raw[33..] != crc {
        return None;
    }
    raw[1..33].try_into().ok()
}

/// A Stellar account address's public key bytes (`G…`).
pub fn decode_address(address: &str) -> Option<[u8; 32]> {
    decode_strkey(VERSION_ACCOUNT, address)
}

// ---------------------------------------------------------------- the wallet

/// The keypair that signs hosted calls and pays for them.
#[derive(Clone)]
pub struct Wallet {
    seed: [u8; 32],
    /// `G…`
    pub address: String,
    /// the 12 recovery words; empty for a wallet restored from its secret key
    pub mnemonic: String,
}

impl std::fmt::Debug for Wallet {
    // never print the secret
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Wallet({})", self.address)
    }
}

/// SEP-0005: ed25519 SLIP-10 derivation along m/44'/148'/`account`'.
fn sep0005(seed: &[u8], account: u32) -> [u8; 32] {
    use hmac::{Hmac, Mac};
    type HmacSha512 = Hmac<sha2::Sha512>;
    let mut mac = HmacSha512::new_from_slice(b"ed25519 seed").unwrap();
    mac.update(seed);
    let i = mac.finalize().into_bytes();
    let (mut key, mut chain): ([u8; 32], [u8; 32]) = (i[..32].try_into().unwrap(), i[32..].try_into().unwrap());
    for index in [44, 148, account] {
        let mut mac = HmacSha512::new_from_slice(&chain).unwrap();
        mac.update(&[0]);
        mac.update(&key);
        mac.update(&(index | 0x8000_0000).to_be_bytes());
        let i = mac.finalize().into_bytes();
        key = i[..32].try_into().unwrap();
        chain = i[32..].try_into().unwrap();
    }
    key
}

impl Wallet {
    fn from_seed(seed: [u8; 32], mnemonic: String) -> Wallet {
        let public = SigningKey::from_bytes(&seed).verifying_key().to_bytes();
        Wallet { seed, address: encode_strkey(VERSION_ACCOUNT, &public), mnemonic }
    }

    /// A new wallet, from 12 new words.
    pub fn generate() -> Result<Wallet, String> {
        let mut entropy = [0u8; 16];
        getrandom::getrandom(&mut entropy).map_err(|e| format!("no randomness for a new wallet: {e}"))?;
        let words = bip39::Mnemonic::from_entropy(&entropy).map_err(|e| e.to_string())?;
        Wallet::from_mnemonic(&words.to_string())
    }

    /// The wallet 12 or 24 recovery words make (SEP-0005, account 0, no passphrase).
    pub fn from_mnemonic(words: &str) -> Result<Wallet, String> {
        let normal = words.split_whitespace().map(str::to_lowercase).collect::<Vec<_>>().join(" ");
        let m = bip39::Mnemonic::parse_in_normalized(bip39::Language::English, &normal)
            .map_err(|e| format!("those aren't valid recovery words ({e})"))?;
        Ok(Wallet::from_seed(sep0005(&m.to_seed(""), 0), normal))
    }

    /// The wallet a secret key (`S…`) is.
    pub fn from_secret(secret: &str) -> Result<Wallet, String> {
        let seed = decode_strkey(VERSION_SEED, secret).ok_or("that isn't a valid secret key (S…)")?;
        Ok(Wallet::from_seed(seed, String::new()))
    }

    /// Recovery words or a secret key, whichever `input` is.
    pub fn restore(input: &str) -> Result<Wallet, String> {
        let t = input.trim();
        if !t.contains(char::is_whitespace) && t.starts_with('S') {
            Wallet::from_secret(t)
        } else {
            Wallet::from_mnemonic(t)
        }
    }

    /// The secret key (`S…`).
    pub fn secret(&self) -> String {
        encode_strkey(VERSION_SEED, &self.seed)
    }

    pub fn public_key(&self) -> [u8; 32] {
        SigningKey::from_bytes(&self.seed).verifying_key().to_bytes()
    }

    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        SigningKey::from_bytes(&self.seed).sign(message).to_bytes()
    }

    /// The two signed-identity headers for one hosted call (Inkternity's
    /// RequestSigner, byte for byte): `path` as the proxy sees it (`/tools/tts`),
    /// `lease_id` empty when there's none. The body isn't signed, by design.
    pub fn sign_request(&self, method: &str, path: &str, tool: &str, lease_id: &str) -> Vec<(&'static str, String)> {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let mut nonce = [0u8; 16];
        let _ = getrandom::getrandom(&mut nonce);
        self.sign_request_at(method, path, tool, lease_id, t, &hex(&nonce))
    }

    pub fn sign_request_at(&self, method: &str, path: &str, tool: &str, lease_id: &str, t: u64, nonce: &str) -> Vec<(&'static str, String)> {
        let s = |v: &str| serde_json::to_string(v).unwrap();
        // compact, keys in alphabetical order: the proxy verifies these exact bytes
        let payload = format!(
            "{{\"i\":\"ai-request\",\"lid\":{},\"m\":{},\"n\":{},\"p\":{},\"t\":{t},\"tool\":{}}}",
            s(lease_id),
            s(method),
            s(nonce),
            s(path),
            s(tool)
        );
        use base64::Engine;
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let sig = self.sign(payload.as_bytes());
        vec![("X-Ink-Pubkey", self.address.clone()), ("X-Ink-Auth", format!("{}.{}", b64.encode(sig), b64.encode(payload.as_bytes())))]
    }

    /// Read a saved wallet. `Ok(None)`: there isn't one yet.
    pub fn load(path: &Path) -> Result<Option<Wallet>, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("couldn't read the wallet at {}: {e}", path.display())),
        };
        let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("the wallet file is damaged: {e}"))?;
        let words = v["app_mnemonic"].as_str().unwrap_or_default();
        let wallet = if words.is_empty() {
            Wallet::from_secret(v["app_secret"].as_str().unwrap_or_default())?
        } else {
            Wallet::from_mnemonic(words)?
        };
        Ok(Some(wallet))
    }

    /// Save it, with the field names Inkternity's key file uses.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut v = serde_json::json!({ "app_pub": self.address, "app_secret": self.secret() });
        if !self.mnemonic.is_empty() {
            v["app_mnemonic"] = self.mnemonic.clone().into();
        }
        std::fs::write(path, serde_json::to_string_pretty(&v).unwrap()).map_err(|e| format!("couldn't save the wallet: {e}"))
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Where Keyternity keeps its wallet.
pub fn default_path() -> PathBuf {
    directories_next::ProjectDirs::from("com", "keyternity", "keyternity")
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(std::env::temp_dir)
        .join("wallet.json")
}

/// The wallet at `path`, made (and saved) on first use.
pub fn load_or_create(path: &Path) -> Result<Wallet, String> {
    if let Some(w) = Wallet::load(path)? {
        return Ok(w);
    }
    let w = Wallet::generate()?;
    w.save(path)?;
    Ok(w)
}

static CURRENT: std::sync::Mutex<Option<Wallet>> = std::sync::Mutex::new(None);

/// Keyternity's wallet (`default_path`), read once, made on first use.
pub fn current() -> Result<Wallet, String> {
    let mut c = CURRENT.lock().unwrap();
    if let Some(w) = &*c {
        return Ok(w.clone());
    }
    let w = load_or_create(&default_path())?;
    *c = Some(w.clone());
    Ok(w)
}

/// Make `w` Keyternity's wallet (Restore), saved for next time.
pub fn replace(w: Wallet) -> Result<(), String> {
    w.save(&default_path())?;
    *CURRENT.lock().unwrap() = Some(w);
    Ok(())
}

// ---------------------------------------------------------------- the quote

/// What a `402` asks for (hvym-img-tools X402_BILLING.md §4.1).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Challenge {
    /// `USDC`, or `XLM`/`native`
    pub asset: String,
    /// the asset's issuer; empty for XLM
    pub issuer: String,
    /// decimal, e.g. `0.75`
    pub amount: String,
    pub pay_to: String,
    /// `public` or `testnet`
    pub network: String,
    pub window_s: f64,
    /// the text memo that binds the payment to this quote
    pub memo: String,
    pub price_id: String,
}

impl Challenge {
    /// A 402's body (`{"x402": {...}}`), or `/warm/price`'s flat answer.
    pub fn parse(body: &str) -> Option<Challenge> {
        let v: serde_json::Value = serde_json::from_str(body).ok()?;
        let x = if v["x402"].is_object() { &v["x402"] } else { &v };
        let s = |k: &str| x[k].as_str().unwrap_or_default().to_string();
        let window_s = x["window_s"].as_f64().or_else(|| x["window_s"].as_str().and_then(|w| w.parse().ok())).unwrap_or(0.);
        Some(Challenge { asset: s("asset"), issuer: s("issuer"), amount: s("amount"), pay_to: s("pay_to"), network: s("network"), window_s, memo: s("memo"), price_id: s("price_id") })
    }

    pub fn native(&self) -> bool {
        matches!(self.asset.to_uppercase().as_str(), "XLM" | "NATIVE")
    }

    /// The asset the payment is in. `None`: one this wallet can't pay with.
    pub fn asset(&self) -> Option<Asset> {
        if self.native() {
            return Some(Asset::Native);
        }
        let issuer = decode_address(&self.issuer)?;
        let code = self.asset.as_bytes();
        (!code.is_empty() && code.len() <= 12 && code.iter().all(u8::is_ascii_alphanumeric)).then(|| Asset::Credit(self.asset.clone(), issuer))
    }
}

/// `0.75` → 7_500_000 stroops (1 unit = 10^7). Never through floats.
pub fn to_stroops(amount: &str) -> Option<i64> {
    let a = amount.trim();
    let (int, frac) = a.split_once('.').unwrap_or((a, ""));
    if int.is_empty() && frac.is_empty() {
        return None;
    }
    let int = if int.is_empty() { "0" } else { int };
    if frac.len() > 7 || !int.bytes().all(|b| b.is_ascii_digit()) || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    format!("{int}{frac:0<7}").parse().ok()
}

/// A Stellar network: its passphrase (part of every transaction's hash) and Horizon.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Network {
    Public,
    Testnet,
}

impl Network {
    pub fn from_name(name: &str) -> Option<Network> {
        match name.to_lowercase().as_str() {
            "public" | "mainnet" | "pubnet" => Some(Network::Public),
            "testnet" | "test" => Some(Network::Testnet),
            _ => None,
        }
    }
    pub fn passphrase(self) -> &'static str {
        match self {
            Network::Public => "Public Global Stellar Network ; September 2015",
            Network::Testnet => "Test SDF Network ; September 2015",
        }
    }
    pub fn horizon(self) -> &'static str {
        match self {
            Network::Public => "https://horizon.stellar.org",
            Network::Testnet => "https://horizon-testnet.stellar.org",
        }
    }
}

// ---------------------------------------------------------------- transactions

#[derive(Clone, Debug, PartialEq)]
pub enum Asset {
    Native,
    /// code (1–12 characters) and issuer
    Credit(String, [u8; 32]),
}

pub enum Op {
    Payment { to: [u8; 32], asset: Asset, stroops: i64 },
    /// let the account hold `asset` (a trustline, at the maximum limit)
    Trust(Asset),
}

/// XDR, the bits of it a payment needs.
#[derive(Default)]
struct Xdr(Vec<u8>);

impl Xdr {
    fn u32(&mut self, v: u32) {
        self.0.extend(v.to_be_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend(v.to_be_bytes());
    }
    fn bytes(&mut self, b: &[u8]) {
        self.0.extend(b);
    }
    /// variable-length opaque or string: length, bytes, zero padding to 4
    fn var(&mut self, b: &[u8]) {
        self.u32(b.len() as u32);
        self.0.extend(b);
        self.0.extend(std::iter::repeat_n(0, (4 - b.len() % 4) % 4));
    }
    fn account(&mut self, key: &[u8; 32]) {
        self.u32(0); // KEY_TYPE_ED25519
        self.bytes(key);
    }
    fn asset(&mut self, asset: &Asset) {
        match asset {
            Asset::Native => self.u32(0),
            Asset::Credit(code, issuer) => {
                let (kind, width) = if code.len() <= 4 { (1, 4) } else { (2, 12) };
                self.u32(kind);
                let mut padded = code.as_bytes().to_vec();
                padded.resize(width, 0);
                self.bytes(&padded);
                self.account(issuer);
            }
        }
    }
}

/// A signed transaction: its envelope (base64 XDR, for Horizon) and its hash (hex,
/// what the proxy is handed as `X-Payment`).
#[derive(Debug)]
pub struct SignedTx {
    pub envelope: String,
    pub hash: String,
}

/// One transaction from `wallet` with sequence number `seq`, valid until `max_time`
/// (unix seconds), a text memo, and `ops`.
pub fn build_tx(wallet: &Wallet, network: Network, seq: i64, fee_per_op: u32, max_time: u64, memo: &str, ops: &[Op]) -> Result<SignedTx, String> {
    if memo.len() > 28 {
        return Err("the payment memo is longer than Stellar allows".into());
    }
    let mut tx = Xdr::default();
    tx.account(&wallet.public_key()); // source, as a MuxedAccount (KEY_TYPE_ED25519 is 0 there too)
    tx.u32(fee_per_op * ops.len() as u32);
    tx.u64(seq as u64);
    tx.u32(1); // PRECOND_TIME
    tx.u64(0);
    tx.u64(max_time);
    if memo.is_empty() {
        tx.u32(0); // MEMO_NONE
    } else {
        tx.u32(1); // MEMO_TEXT
        tx.var(memo.as_bytes());
    }
    tx.u32(ops.len() as u32);
    for op in ops {
        tx.u32(0); // no per-operation source
        match op {
            Op::Payment { to, asset, stroops } => {
                tx.u32(1); // PAYMENT
                tx.account(to);
                tx.asset(asset);
                tx.u64(*stroops as u64);
            }
            Op::Trust(asset) => {
                tx.u32(6); // CHANGE_TRUST
                tx.asset(asset);
                tx.u64(i64::MAX as u64);
            }
        }
    }
    tx.u32(0); // ext v0

    // hash: sha256(network id || ENVELOPE_TYPE_TX || transaction)
    let mut payload = Sha256::digest(network.passphrase().as_bytes()).to_vec();
    payload.extend(2u32.to_be_bytes());
    payload.extend(&tx.0);
    let hash = Sha256::digest(&payload);
    let sig = wallet.sign(&hash);

    let mut env = Xdr::default();
    env.u32(2); // ENVELOPE_TYPE_TX
    env.bytes(&tx.0);
    env.u32(1); // one signature
    env.bytes(&wallet.public_key()[28..]); // its hint: the key's last 4 bytes
    env.var(&sig);
    use base64::Engine;
    Ok(SignedTx { envelope: base64::engine::general_purpose::STANDARD.encode(&env.0), hash: hex(&hash) })
}

// ---------------------------------------------------------------- Horizon

/// What Horizon says about an account.
#[derive(Clone, Debug, Default)]
pub struct Account {
    pub sequence: i64,
    /// (asset code, or `XLM`; issuer; balance as Horizon writes it)
    pub balances: Vec<(String, String, String)>,
}

impl Account {
    pub fn balance(&self, code: &str, issuer: &str) -> Option<&str> {
        self.balances.iter().find(|(c, i, _)| c == code && (code == "XLM" || i == issuer)).map(|(_, _, b)| b.as_str())
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .http_status_as_error(false)
        .build()
        .new_agent()
}

fn read_body(resp: ureq::http::Response<ureq::Body>) -> Result<(u16, String), String> {
    let status = resp.status().as_u16();
    let mut text = String::new();
    resp.into_body().into_reader().read_to_string(&mut text).map_err(|e| format!("Stellar's answer was cut off: {e}"))?;
    Ok((status, text))
}

/// The account's sequence number and balances. `Ok(None)`: it doesn't exist on the
/// network yet (nobody has sent it XLM).
pub fn account(horizon: &str, address: &str) -> Result<Option<Account>, String> {
    let resp = agent().get(&format!("{}/accounts/{address}", horizon.trim_end_matches('/'))).call().map_err(|e| format!("couldn't reach Stellar ({horizon}): {e}"))?;
    let (status, text) = read_body(resp)?;
    match status {
        200 => {}
        404 => return Ok(None),
        _ => return Err(format!("Stellar ({horizon}) answered {status}")),
    }
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("Stellar's answer wasn't readable: {e}"))?;
    let sequence = v["sequence"].as_str().and_then(|s| s.parse().ok()).ok_or("Stellar sent no sequence number")?;
    let balances = v["balances"]
        .as_array()
        .map(|bs| {
            bs.iter()
                .filter_map(|b| {
                    let code = if b["asset_type"] == "native" { "XLM".to_string() } else { b["asset_code"].as_str()?.to_string() };
                    Some((code, b["asset_issuer"].as_str().unwrap_or_default().to_string(), b["balance"].as_str()?.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Some(Account { sequence, balances }))
}

/// Why a payment didn't go through.
#[derive(Debug, PartialEq)]
pub enum PayError {
    /// the wallet doesn't hold enough (of XLM, or of the asset)
    Funds(String),
    Other(String),
}

impl std::fmt::Display for PayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PayError::Funds(s) | PayError::Other(s) => f.write_str(s),
        }
    }
}

/// Send a signed transaction. Its result codes, if Stellar refused it.
pub fn submit(horizon: &str, tx: &SignedTx) -> Result<(), PayError> {
    let form = format!("tx={}", tx.envelope.replace('+', "%2B").replace('/', "%2F").replace('=', "%3D"));
    let resp = agent()
        .post(&format!("{}/transactions", horizon.trim_end_matches('/')))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(form.as_bytes())
        .map_err(|e| PayError::Other(format!("couldn't reach Stellar ({horizon}): {e}")))?;
    let (status, text) = read_body(resp).map_err(PayError::Other)?;
    if status == 200 {
        return Ok(());
    }
    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    let codes = &v["extras"]["result_codes"];
    let mut all: Vec<String> = codes["operations"].as_array().map(|o| o.iter().filter_map(|c| c.as_str().map(String::from)).collect()).unwrap_or_default();
    all.extend(codes["transaction"].as_str().map(String::from));
    let has = |c: &str| all.iter().any(|a| a == c);
    if has("op_underfunded") || has("tx_insufficient_balance") || has("op_low_reserve") {
        return Err(PayError::Funds(if has("op_low_reserve") {
            "the wallet needs a little more XLM (about 0.5) to hold USDC".into()
        } else {
            "the wallet doesn't have enough to pay".into()
        }));
    }
    Err(PayError::Other(if all.is_empty() { format!("Stellar refused the payment ({status})") } else { format!("Stellar refused the payment ({})", all.join(", ")) }))
}

/// Pay what `ch` asks, from `wallet`, on `network` through `horizon`: open a
/// trustline to the asset first if the wallet has none. The paid transaction's hash.
pub fn pay(wallet: &Wallet, ch: &Challenge, network: Network, horizon: &str) -> Result<String, PayError> {
    let asset = ch.asset().ok_or_else(|| PayError::Other(format!("can't pay in {}", ch.asset)))?;
    let to = decode_address(&ch.pay_to).ok_or_else(|| PayError::Other("the payment address isn't a Stellar address".into()))?;
    let stroops = to_stroops(&ch.amount).filter(|s| *s > 0).ok_or_else(|| PayError::Other(format!("can't read the price {:?}", ch.amount)))?;
    let acct = account(horizon, &wallet.address)
        .map_err(PayError::Other)?
        .ok_or_else(|| PayError::Funds("the wallet is empty: send it some XLM and USDC first".into()))?;
    let max_time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) + 300;
    let mut ops = vec![];
    if let Asset::Credit(code, _) = &asset {
        if acct.balance(code, &ch.issuer).is_none() {
            ops.push(Op::Trust(asset.clone()));
            // a new trustline holds nothing, so this payment would fail anyway
            let tx = build_tx(wallet, network, acct.sequence + 1, 10_000, max_time, "", &ops).map_err(PayError::Other)?;
            submit(horizon, &tx)?;
            return Err(PayError::Funds(format!("the wallet can hold {code} now: send it some, then try again")));
        }
    }
    ops.push(Op::Payment { to, asset, stroops });
    let tx = build_tx(wallet, network, acct.sequence + 1, 10_000, max_time, &ch.memo, &ops).map_err(PayError::Other)?;
    submit(horizon, &tx)?;
    Ok(tx.hash)
}
