# aauth-person-server

An [AAuth](https://github.com/dickhardt/AAuth) Person Server (`draft-hardt-oauth-aauth-protocol-11`, three-party and four-party) you can run instead of building one. It speaks for your people when agents want to act for them:

- it verifies each agent by its HTTP Message Signature and agent token, from agent providers you trust;
- it puts each **mission** an agent proposes to whoever decides for the person, then issues **person tokens** and **auth tokens** under it;
- it weighs each auth token request against the mission's **budget**: what fits is issued at once and spent, and what does not goes back to the approver;
- it turns whatever needs a human into an **approval**, which you deliver however you reach people;
- for a resource with its own **Access Server** (a resource token whose `aud` is that server), it weighs the mission and budget first, then redeems the request there and relays the Access Server's auth token, wait or refusal. The `aauth-access-server` crate is one.

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

// When the approver answers, under the tenant it was asked under:
ps.decide(&approval.tenant, &approval.id, Decision::approve().with_budget(budget).by("dana@acme.example")).await?;
```

You implement `Directory`, which says who you speak for and who decides for them. `Hooks` is optional and reports approvals, decisions and issued tokens. The default `Store` is in memory; for more than one process or a restart, put a database behind it. Every `Store` call is scoped by the AAuth `tenant` the agent names, or by the server's default tenant when it names none (`"default"` unless `Config::default_tenant` sets another; `Config::require_tenant` refuses such a request), so one Person Server can serve many organisations. To run it on hyper, see [`examples/server.rs`](examples/server.rs). Each flag defaults to an environment variable (`--help` lists them); the signing key, subject secret and admin token come from the environment only (`PS_SIGNING_KEY`, `PS_SUBJECT_SECRET`, `PS_ADMIN_TOKEN`), since anything on the host can read a process's arguments:

```bash
PS_ADMIN_TOKEN=secret cargo run -p aauth-person-server --example server -- \
  --issuer https://ps.acme.example --default-tenant acme
```

The crate documentation covers the fixed policy, the routes, and what to put in front of it in production.

## Introspection

Introspection can host a Person Server for you instead. Your app then decides its approvals and reads its missions with `client.aauth()` in the [Introspection Rust SDK](https://docs.introspection.dev/reference/rust) (`introspection-sdk`).

## License

Apache-2.0
