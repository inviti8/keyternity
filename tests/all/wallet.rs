//! The hosted-voice wallet (src/wallet.rs): keys, signing, quotes and transactions.

use keyternity_lib::wallet::{self, Asset, Challenge, Network, Op, Wallet};

/// SEP-0005's first test vector: these words make this account (m/44'/148'/0').
const WORDS: &str = "illness spike retreat truth genius clock brain pass fit cave bargain toe";
const ADDRESS: &str = "GDRXE2BQUC3AZNPVFSCEZ76NJ3WWL25FYFK6RGZGIEKWE4SOOHSUJUJ6";
const SECRET: &str = "SBGWSG6BTNCKCOB3DIFBGCVMUPQFYPA2G4O34RMTB343OYPXU5DJDVMN";

#[test]
fn recovery_words_make_the_same_wallet_as_inkternity() {
    let w = Wallet::from_mnemonic(WORDS).unwrap();
    assert_eq!(w.address, ADDRESS);
    assert_eq!(w.secret(), SECRET);
    // the secret alone restores it too, and so do words typed sloppily
    assert_eq!(Wallet::restore(SECRET).unwrap().address, ADDRESS);
    assert_eq!(Wallet::restore(&format!("  {}\n", WORDS.to_uppercase().replace(' ', "   "))).unwrap().address, ADDRESS);
    assert!(Wallet::restore("illness spike retreat truth genius clock brain pass fit cave bargain bargain").is_err(), "bad checksum");
    assert!(Wallet::restore(&SECRET.replace('B', "C")).is_err());
}

#[test]
fn a_new_wallet_has_words_that_restore_it() {
    let w = Wallet::generate().unwrap();
    assert_eq!(w.mnemonic.split(' ').count(), 12);
    assert_eq!(Wallet::from_mnemonic(&w.mnemonic).unwrap().address, w.address);
    assert_ne!(Wallet::generate().unwrap().address, w.address);
    assert!(!format!("{w:?}").contains(&w.secret()), "never printed");
}

#[test]
fn strkeys_round_trip_and_reject_typos() {
    let key = wallet::decode_address(ADDRESS).unwrap();
    assert_eq!(wallet::encode_strkey(6 << 3, &key), ADDRESS);
    let typo = ADDRESS.replacen('R', "S", 1);
    assert!(wallet::decode_address(&typo).is_none());
    assert!(wallet::decode_address(SECRET).is_none(), "a secret isn't an address");
    assert!(wallet::decode_address("GABC").is_none());
}

#[test]
fn the_wallet_saves_and_loads() {
    let path = std::env::temp_dir().join(format!("keyternity_wallet_test_{}.json", std::process::id()));
    let _ = std::fs::remove_file(&path);
    assert!(Wallet::load(&path).unwrap().is_none());
    let made = wallet::load_or_create(&path).unwrap();
    assert_eq!(wallet::load_or_create(&path).unwrap().address, made.address, "made once");
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(v["app_pub"], made.address.as_str());
    assert_eq!(v["app_mnemonic"], made.mnemonic.as_str());
    // a wallet from a secret key has no words, and saves without them
    Wallet::from_secret(SECRET).unwrap().save(&path).unwrap();
    let back = Wallet::load(&path).unwrap().unwrap();
    assert_eq!((back.address.as_str(), back.mnemonic.as_str()), (ADDRESS, ""));
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn requests_are_signed_as_the_proxy_verifies_them() {
    use base64::Engine;
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    let w = Wallet::from_mnemonic(WORDS).unwrap();
    let h = w.sign_request_at("POST", "/tools/tts", "tts", "", 1_700_000_000, "00112233445566778899aabbccddeeff");
    assert_eq!(h[0], ("X-Ink-Pubkey", ADDRESS.to_string()));
    assert_eq!(h[1].0, "X-Ink-Auth");
    let (sig, payload) = h[1].1.split_once('.').unwrap();
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let payload = b64.decode(payload).unwrap();
    // byte for byte what Inkternity's RequestSigner and hvym-img-tools' tests build
    assert_eq!(
        String::from_utf8(payload.clone()).unwrap(),
        r#"{"i":"ai-request","lid":"","m":"POST","n":"00112233445566778899aabbccddeeff","p":"/tools/tts","t":1700000000,"tool":"tts"}"#
    );
    let key = VerifyingKey::from_bytes(&wallet::decode_address(ADDRESS).unwrap()).unwrap();
    let sig = Signature::from_slice(&b64.decode(sig).unwrap()).unwrap();
    key.verify(&payload, &sig).unwrap();
    // a fresh nonce every time
    let auth = |h: Vec<(&str, String)>| h[1].1.clone();
    assert_ne!(auth(w.sign_request("POST", "/tools/tts", "tts", "")), auth(w.sign_request("POST", "/tools/tts", "tts", "")));
}

#[test]
fn a_402_quote_is_read() {
    let body = r#"{"detail":"no live paid window for this identity","x402":{"asset":"USDC","issuer":"GA5ZSEJYB37JRC5AVCIA5MOP4RHTM335X2KGX3IHOJAPP5RE34K4KZVN","amount":"0.75","pay_to":"GA32CYGMEIFC7FVWEWMAUWVBG52ZSR7AEYCFDZZQ5KJFNBHB3FRYXEEZ","network":"public","window_s":900.0,"memo":"9f2c","price_id":"9f2c","horizon":"https://horizon.stellar.org"}}"#;
    let ch = Challenge::parse(body).unwrap();
    assert_eq!((ch.asset.as_str(), ch.amount.as_str(), ch.window_s, ch.memo.as_str()), ("USDC", "0.75", 900., "9f2c"));
    assert!(matches!(ch.asset(), Some(Asset::Credit(c, _)) if c == "USDC"));
    // /warm/price answers flat
    let flat = Challenge::parse(r#"{"asset":"XLM","amount":"2","window_s":"900","pay_to":"x","network":"testnet"}"#).unwrap();
    assert_eq!((flat.window_s, flat.asset()), (900., Some(Asset::Native)));
    assert!(Challenge { asset: "USDC".into(), issuer: "nope".into(), ..Default::default() }.asset().is_none());
    assert!(Challenge::parse("not json").is_none());
}

#[test]
fn prices_become_stroops_without_floats() {
    for (a, s) in [("0.75", Some(7_500_000)), ("1", Some(10_000_000)), (".5", Some(5_000_000)), ("0.0000001", Some(1)), ("12.3456789", Some(123_456_789))] {
        assert_eq!(wallet::to_stroops(a), s, "{a}");
    }
    for bad in ["", ".", "1.23456789", "-1", "1e3", "0.7a"] {
        assert_eq!(wallet::to_stroops(bad), None, "{bad}");
    }
    assert_eq!(Network::from_name("public"), Some(Network::Public));
    assert_eq!(Network::from_name("TESTNET"), Some(Network::Testnet));
    assert_eq!(Network::from_name("futurenet"), None);
}

/// A payment exactly as `wallet::pay` builds one, for the cross-check below.
fn sample_tx() -> wallet::SignedTx {
    let w = Wallet::from_mnemonic(WORDS).unwrap();
    let usdc = Asset::Credit("USDC".into(), wallet::decode_address("GA5ZSEJYB37JRC5AVCIA5MOP4RHTM335X2KGX3IHOJAPP5RE34K4KZVN").unwrap());
    let to = wallet::decode_address("GA32CYGMEIFC7FVWEWMAUWVBG52ZSR7AEYCFDZZQ5KJFNBHB3FRYXEEZ").unwrap();
    let ops = [Op::Trust(usdc.clone()), Op::Payment { to, asset: usdc, stroops: 7_500_000 }];
    wallet::build_tx(&w, Network::Testnet, 123_456_789_012, 10_000, 1_700_000_300, "9f2c0123456789abcdef01234567", &ops).unwrap()
}

/// Checked against stellar_sdk (hvym-img-tools' test environment has it):
/// `TransactionEnvelope.from_xdr(envelope, testnet)` reads back the source, fee,
/// sequence, time bounds, memo and both operations, its `hash_hex()` is `hash`,
/// and the signature verifies. Pinned here, so a change to the encoding fails.
#[test]
fn transactions_encode_as_stellar_reads_them() {
    let tx = sample_tx();
    assert_eq!(tx.hash, SAMPLE_HASH);
    assert_eq!(tx.envelope, SAMPLE_ENVELOPE);
    let long = "x".repeat(29);
    assert!(wallet::build_tx(&Wallet::from_mnemonic(WORDS).unwrap(), Network::Public, 1, 100, 0, &long, &[]).is_err(), "Stellar's text memo limit");
}

const SAMPLE_HASH: &str = "6c6ee47ef0ec74a33762987b8129d27e2f2a5e9bf8e040af15a30e2e0505d4d6";
const SAMPLE_ENVELOPE: &str = "AAAAAgAAAADjcmgwoLYMtfUshEz/zU7tZeulwVXomyZBFWJyTnHlRAAATiAAAAAcvpkaFAAAAAEAAAAAAAAAAAAAAABlU/IsAAAAAQAAABw5ZjJjMDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3AAAAAgAAAAAAAAAGAAAAAVVTREMAAAAAO5kROA7+mIugqJAOsc/kTzZvfb6Ua+0HckD39iTfFcV//////////wAAAAAAAAABAAAAADehYMwiCi+WtiWYClqhN3WZR+AmBFHnMOqSVoTh2WOLAAAAAVVTREMAAAAAO5kROA7+mIugqJAOsc/kTzZvfb6Ua+0HckD39iTfFcUAAAAAAHJw4AAAAAAAAAABTnHlRAAAAEDXG/q7u1oWdiqSmd8cI7MTYb9A57uNMp3pr4xs41XbtmwZMlrIyrLINKbM74sWN8C/06XwxB2jPBQ/SljlTKMF";

