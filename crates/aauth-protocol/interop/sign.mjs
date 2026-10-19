// Signs requests with @hellocoop/httpsig and prints them, for the Rust verifier.
import { generateKeyPairSync } from "node:crypto";
import { fetch } from "@hellocoop/httpsig";

const { privateKey } = generateKeyPairSync("ed25519");
const signingKey = { ...privateKey.export({ format: "jwk" }), alg: "Ed25519" };
const { d: _d, ...publicJwk } = signingKey;
const b64 = (v) => Buffer.from(JSON.stringify(v)).toString("base64url");
// Unsigned: the Rust side checks only the HTTP signature, not the token.
const jwt = `${b64({ alg: "Ed25519", typ: "aa-agent+jwt" })}.${b64({
  iss: "https://agents.example",
  exp: Math.floor(Date.now() / 1000) + 600,
  cnf: { jwk: publicJwk },
})}.sig`;

const cases = [];
for (const [method, path, body] of [
  ["POST", "/mission", JSON.stringify({ description: "Sydney" })],
  ["GET", "/pending/1", undefined],
]) {
  const url = `https://ps.example${path}`;
  const { headers } = await fetch(url, {
    method,
    body,
    headers: body ? { "content-type": "application/json" } : {},
    signingKey,
    signatureKey: { type: "jwt", jwt },
    dryRun: true,
  });
  cases.push({ method, path, body, headers: Object.fromEntries(headers) });
}
console.log(JSON.stringify({ jwk: publicJwk, cases }));
