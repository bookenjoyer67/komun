---
classification: public
project: proj-komun
doc_type: reference
---

# Reference: the WASM crypto boundary

What crosses the boundary between the browser's cryptography and the server?

The cryptographic boundary of this project is the wasm crate (`docs/CRYPTO.md:105` `is the cryptographic boundary; changes there require rebuilding the wasm`). Secret keys never leave the client (`docs/CRYPTO.md:100` `Secret keys never leave the client; the server stores public keys and wrapped bundles only.`). The schema follows that: the account row carries a public key and a wrapped bundle (`migrations/001_schema.sql:25` `encryption_public_key BYTEA,`), and a message carries `ciphertext` plus `nonce` (`migrations/001_schema.sql:232` `ciphertext BYTEA NOT NULL,`).

## Which exported functions form the boundary?

The message entry point is an exported Rust function over a recipient's x25519 public key (`crates/wasm/src/lib.rs:49` `pub fn encrypt_message(plaintext: &[u8], recipient_x25519_pk: &[u8])`). The conversation key comes from a separate export (`crates/wasm/src/lib.rs:84` `pub fn derive_shared_key(my_x25519_sk: &[u8], their_x25519_pk: &[u8])`).

## Which rules does the boundary impose?

- Never log keys, bundles, passwords, derived keys or plaintext messages (`docs/CRYPTO.md:103` `Never log keys, bundles, passwords, derived keys, or plaintext messages.`).
- Use a fresh random nonce for every authenticated encryption operation (`docs/CRYPTO.md:104` `Every AEAD operation uses a fresh random nonce; never reuse a nonce with the same key.`).
- Rebuild the wasm package and the frontend together after any change here (`docs/CRYPTO.md:105` `changes there require rebuilding the wasm`).

## What does this design not protect against?

Browser-delivered encryption cannot defend against a server that ships modified JavaScript (`docs/CRYPTO.md:93` `cannot protect against a malicious server that`).
