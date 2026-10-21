# aauth-access-server

An [AAuth](https://github.com/dickhardt/AAuth) Access Server (`draft-hardt-oauth-aauth-protocol-11`, four-party, §Access Server Federation): a resource's own policy, beside the person's.

A resource that keeps its own policy names this server as the `aud` of its resource tokens. The person's Person Server redeems them here, signing as itself and sending the resource token, the agent's token and the token the resource saw. This server verifies all of it, asks your `Policy`, and issues an auth token for the resource, refuses, or makes the Person Server wait while someone decides. Both sides must agree: the Person Server for the person (mission, budget, approver), and the Access Server for the resource (its customers, its limits).

It is a Rust port of [`@introspection-sdk/access-server`](https://www.npmjs.com/package/@introspection-sdk/access-server).

```rust
use aauth_access_server::{AccessDecision, AccessRequest, AccessServer, Config, Verdict};

let access = AccessServer::new(
    Config::new("https://as.flightsector.example", |request: &AccessRequest| {
        match request.tenant.as_deref() {
            Some("acme") => Verdict::allow(),
            Some("globex") => Verdict::Approval, // someone at Flight Sector decides
            _ => Verdict::deny("no Flight Sector account"),
        }
    })
    .resources(["https://booking.flightsector.example"])
    .person_servers(["https://ps.acme.example"])
    .agent_providers(["https://api.introspection.dev"])
    .signing_key(aauth_protocol::SigningKey::from_pkcs8_pem(&pem)?),
)?;

// Any framework: hand it an http::Request<Bytes>, get an http::Response<Bytes>.
let response = access.handle(request).await;

// Settle a request the policy waited on; the Person Server's next poll collects it.
access.decide(&pending.id, AccessDecision::approve().by("ops@flightsector.example"))?;
```

The resource names its Access Server with `ResourceTokenParams::access_server`, and accepts the auth tokens it issues with `AuthTokenIssuers::AccessServer`, both in `aauth-protocol`. The Person Server's side is `aauth_protocol::federation`, which `aauth-person-server` runs for any resource token whose `aud` is not itself.

Pending requests are kept in the process's memory. Not yet served: `requirement=claims` and `clarification`, `402` payment, `revocation_endpoint`, and the `subagent_token` and `upstream_token` parameters.

## License

Apache-2.0
