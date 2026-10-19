// Verifies the Rust-signed requests in the given file with @hellocoop/httpsig.
import { readFileSync } from "node:fs";
import { verify } from "@hellocoop/httpsig";

const fixture = JSON.parse(readFileSync(process.argv[2], "utf8"));
let failed = false;
for (const c of fixture.cases) {
  const result = await verify(
    {
      method: c.method,
      authority: c.authority,
      path: c.path,
      headers: c.headers,
      body: c.body ?? undefined,
    },
    // The requests carry a fixed created time, so the freshness window is not under test here.
    { maxClockSkew: Number.MAX_SAFE_INTEGER, requireContentDigest: true },
  );
  console.log(
    `${c.method} ${c.authority}${c.path}: ${result.verified ? "verified" : `FAILED ${result.error}`}`,
  );
  failed ||= !result.verified;
}
process.exit(failed ? 1 : 0);
