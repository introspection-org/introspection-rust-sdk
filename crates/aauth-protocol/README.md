# aauth-protocol

[AAuth](https://github.com/dickhardt/AAuth) (`draft-hardt-oauth-aauth-protocol-11`) for Rust. AAuth is how an agent proves who it is, and what a person has allowed it to do, on every request it makes.

Each request carries an RFC 9421 HTTP Message Signature under the AAuth profile. The signer's key is named in `Signature-Key`, usually as a token that binds it: an **agent token** from the agent's provider, a **person token**, or an **auth token** from the person's Person Server.

This crate is the part of the protocol every role shares.

| Module | What it does |
| --- | --- |
| `keys`, `jwk`, `jwt` | Ed25519 signing keys, public JWKs and RFC 7638 thumbprints, compact JWS |
| `httpsig` | Sign a request, or verify one and learn which key signed it |
| `token` | Mint tokens, or verify one against its issuer's keys and the request's signer |
| `discovery` | Find an issuer's keys through its metadata document, over an HTTP client you choose |
| `resource` | The resource's side: authenticate a request, mint resource tokens, challenge |
| `requirement` | The `AAuth-Requirement` header |
| `mission`, `budget` | What a person approved, and what it may spend |
| `client` | The agent's Person Server client, and an SSRF-guarded HTTP client (`client` feature, on by default) |

The Person Server itself is the [`aauth-person-server`](https://crates.io/crates/aauth-person-server) crate.

## Sign as an agent

```rust
use aauth_protocol::httpsig::{sign_jwt, Outgoing};

let headers = sign_jwt(&session_key, &agent_token, &Outgoing {
    method: "POST",
    url: "https://api.booking.example/v1/reserve",
    content_type: None,
    body: Some(br#"{"offer_id":"fl-qf1"}"#),
    created: aauth_protocol::now_unix(),
})?;
// content-digest, content-type, signature-key, signature-input, signature
```

## Verify as a resource

```rust
use aauth_protocol::resource::authenticate;
use aauth_protocol::token::TokenKind;

let authenticated = authenticate(
    &request,                         // httpsig::Incoming: method, authority, path, headers, body
    "https://api.booking.example",    // this resource's identifier
    &[TokenKind::Auth],
    &VerifyOptions::default(),
    &resolver,                        // discovery::JwksResolver over a guarded ReqwestFetch
).await?;
authenticated.token.scope();
```

`authenticate` checks the signature first, then that the token's `cnf.jwk` is the key that signed. It checks the token against its issuer's published keys last. Nothing a token claims is believed until that last check passes.

Only the token's issuer is ever looked up, and only when it is a server identifier. A `Signature-Key: sig=jwks_uri` from an unauthenticated caller is refused, not fetched. Use `ReqwestFetch::guarded` for discovery: it calls `https` only, refuses IP literals and `localhost`, and its resolver refuses private addresses.

## Talk to a Person Server as an agent

```rust
use aauth_protocol::client::{guarded_client, Agent, Answer, DeniedNetworks, Person, PersonServerClient, Reach};

let client = PersonServerClient::new(guarded_client(DeniedNetworks::default())?, Reach::default());
let metadata = client.metadata("https://ps.acme.example").await?;
let agent = Agent { agent_token: &agent_token, key: &session_key };
let sam = Person { login_hint: Some("user:sam@acme.example"), tenant: None };
match client.propose_mission(&agent, &metadata, &json!({ "description": "Sydney, 19-23 October" }), &sam).await? {
    Answer::Pending { location, retry_after } => { /* poll `location` */ }
    Answer::Granted(mission) => { /* approved already */ }
    Answer::Denied { reason, .. } => { /* refused */ }
    _ => {}
}
```

## Interop

CI checks this crate against the JavaScript implementations. [`@hellocoop/httpsig`](https://www.npmjs.com/package/@hellocoop/httpsig), by the spec's author, verifies what this crate signs, and this crate verifies what it signs. CI also drives [`@introspection-sdk/person-server`](https://www.npmjs.com/package/@introspection-sdk/person-server) through this crate's client.

## Status

The crate implements protocol draft `-11`.

- **Algorithms:** signing is Ed25519; verification accepts Ed25519 and ES256.
- **`Signature-Key` schemes:** `jwt`, `jwks_uri` and `hwk` are supported; `jkt-jwt` and `x509` are not.
- **Revocation:** not checked.
- **`@query`:** follows RFC 9421 and includes the leading `?`. `@hellocoop/httpsig` 2.6 leaves the `?` out, so the two disagree on any signed request that has a query string.

Until 1.0 the API may change between minor versions.

## License

Apache-2.0
