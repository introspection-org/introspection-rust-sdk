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
| `resource` | The resource's side: authenticate a request, mint resource tokens (to its own Access Server, when it has one), challenge |
| `federation` | Four-party: a Person Server redeeming a resource token at the resource's Access Server, one exchange per call |
| `proposal` | Read a resource's R3 proposal summary, signed as a Person Server or Access Server |
| `requirement` | The `AAuth-Requirement` header |
| `mission`, `budget` | What a person approved, and what it may spend |
| `client` | The agent's Person Server client, and an SSRF-guarded HTTP client (`client` feature, on by default) |

The Person Server itself is the [`aauth-person-server`](https://crates.io/crates/aauth-person-server) crate, and an Access Server the [`aauth-access-server`](https://crates.io/crates/aauth-access-server) crate.

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

## Federate with an Access Server

A resource token whose `aud` is not the Person Server names the resource's own Access Server (`#access-server-federation`). Once the Person Server has verified the resource token and weighed it against the mission, `federate` makes one exchange with that server: the signed token request, or one poll of where it said to wait. It does its I/O only through the `Fetch` it is handed and reads no clock but `now`, so a durable workflow can run each call as a step.

```rust
use aauth_protocol::federation::{federate, AccessOutcome, AuthTokenRequest, Federate, FederationContext};

let tokens = AuthTokenRequest::new(resource_token, agent_token, presented_token);
let ctx = FederationContext { issuer: "https://ps.acme.example", key: &ps_key, fetch: &fetch, now };
let mut location = None;
loop {
    match federate(&Federate { access_server: &aud, tokens: &tokens, agent_jkt: &jkt, location: location.as_deref() }, &ctx).await? {
        AccessOutcome::Issued(issued) => break issued,          // verified per #auth-token-delivery
        AccessOutcome::Pending { location: next, .. } => location = Some(next),
        _ => unreachable!(),
    }
}
```

A refusal is a `FederationError::Refused` carrying the Access Server's own error and status; anything that cannot be verified is `as_unreachable` (`502`).

## Introspection

The [Introspection Rust SDK](https://docs.introspection.dev/reference/rust) (`introspection-sdk`) decides approvals and reads missions on Introspection's hosted Person Server with `client.aauth()`.

## Interop

The tests check this crate against [`@hellocoop/httpsig`](https://www.npmjs.com/package/@hellocoop/httpsig), the RFC 9421 implementation by the AAuth draft's author, using fixed requests in `tests/vectors/hellocoop.json`. Requests that library signed must verify here, and requests this crate signs must match, byte for byte, ones that library has verified. None of them carries a query string; see `@query` below.

## Status

The crate implements protocol draft `-11`.

- **Algorithms:** signing is Ed25519; verification accepts Ed25519 and ES256.
- **`Signature-Key` schemes:** `jwt`, `jwks_uri` and `hwk` are supported; `jkt-jwt` and `x509` are not.
- **Revocation:** not checked.
- **Four-party:** `requirement=claims` and `clarification`, `402` payment, and revocation at the Access Server are not served; `federate` refuses each `403 denied`, naming what the Access Server asked for.
- **`@query`:** follows RFC 9421 and includes the leading `?`. `@hellocoop/httpsig` 2.6 leaves the `?` out, so the two disagree on any signed request that has a query string.

Until 1.0 the API may change between minor versions.

## License

Apache-2.0
