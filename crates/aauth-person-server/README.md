# aauth-person-server

An [AAuth](https://github.com/dickhardt/AAuth) Person Server (`draft-hardt-oauth-aauth-protocol-11`, three-party) you can run instead of building one. It speaks for your people when agents want to act for them:

- it verifies each agent by its HTTP Message Signature and agent token, from agent providers you trust;
- it puts each **mission** an agent proposes to whoever decides for the person, then issues **person tokens** and **auth tokens** under it;
- it weighs each auth token request against the mission's **budget**: what fits is issued at once and spent, and what does not goes back to the approver;
- it turns whatever needs a human into an **approval**, which you deliver however you reach people.

It is a Rust port of [`@introspection-sdk/person-server`](https://www.npmjs.com/package/@introspection-sdk/person-server); an agent cannot tell the two apart.

```rust
use aauth_person_server::{Config, Decision, PersonServer};

let ps = PersonServer::new(
    Config::new("https://ps.acme.example", directory, ["https://api.introspection.dev"])
        .signing_key(aauth_protocol::SigningKey::from_pkcs8_pem(&pem)?)
        .subject_secret(secret)
        .hooks(notifier)        // on_approval: reach whoever decides
        .admin_token(admin),    // turns on /consent and /missions
)?;

// Any framework: hand it an http::Request<Bytes>, get an http::Response<Bytes>.
let response = ps.handle(request).await;

// When the approver answers:
ps.decide(&approval_id, Decision::approve().with_budget(budget).by("dana@acme.example")).await?;
```

You implement `Directory`, which says who you speak for and who decides for them. `Hooks` is optional and reports approvals, decisions and issued tokens. The default `Store` is in memory; for more than one process, put a database behind it. To run it on hyper, see [`examples/server.rs`](examples/server.rs):

```bash
PS_ISSUER=https://ps.acme.example PS_TRUSTED=https://api.introspection.dev \
PS_ADMIN_TOKEN=secret cargo run -p aauth-person-server --example server
```

The crate documentation covers the fixed policy, the routes, and what to put in front of it in production.

## License

Apache-2.0
