// Runs @introspection-sdk/person-server as https://ps.example on 127.0.0.1,
// trusting the agent provider whose JWKS is in the given file, and prints
// its port. The Rust agent reaches it with a connect-to override.
import { readFileSync } from "node:fs";
import { createServer } from "node:http";
import { Readable } from "node:stream";

import { createPersonServer } from "@introspection-sdk/person-server";

const issuer = "https://ps.example";
const ap = "https://agents.example";
const jwks = JSON.parse(readFileSync(process.argv[2], "utf8"));
const people = { "user:sam@acme.example": { id: "sam", name: "Sam" } };

const ps = await createPersonServer({
  issuer,
  trustedAgentProviders: [ap],
  adminToken: "admin",
  directory: { person: (hint) => people[hint] },
  fetch: async (url) => {
    if (url === `${ap}/.well-known/aauth-agent.json`)
      return Response.json({ issuer: ap, jwks_uri: `${ap}/jwks.json` });
    if (url === `${ap}/jwks.json`) return Response.json(jwks);
    return new Response(null, { status: 404 });
  },
});

const server = createServer(async (req, res) => {
  const hasBody = req.method !== "GET" && req.method !== "HEAD" && req.method !== "DELETE";
  const response = await ps.handle(
    new Request(new URL(req.url, issuer), {
      method: req.method,
      headers: req.headers,
      body: hasBody ? Readable.toWeb(req) : undefined,
      duplex: "half",
    }),
  );
  res.writeHead(response.status, Object.fromEntries(response.headers));
  res.end(Buffer.from(await response.arrayBuffer()));
});
server.listen(0, "127.0.0.1", () => console.log(server.address().port));
