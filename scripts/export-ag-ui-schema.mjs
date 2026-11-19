// Export @ag-ui/core's zod schemas as one JSON Schema document (draft 2020-12),
// the snapshot tests/agui_schema_parity_test.rs holds the AG-UI types to.
// usage: npm i --prefix <dir> @ag-ui/core@<version> zod@4 && node scripts/export-ag-ui-schema.mjs <dir> > crates/introspection-sdk/tests/fixtures/ag-ui-core.schema.json
import { createRequire } from "node:module";
const req = createRequire(process.argv[2] + "/package.json");
const S = await import(req.resolve("@ag-ui/core/schemas"));
const { z } = await import(req.resolve("zod"));
const version = req("@ag-ui/core/package.json").version;
const reg = z.registry();
for (const [name, s] of Object.entries(S)) {
  if (name.endsWith("Schema") && s?._zod) reg.add(s, { id: name.replace(/Schema$/, "") });
}
const out = z.toJSONSchema(reg, { target: "draft-2020-12", unrepresentable: "any", io: "input", uri: (id) => `#/$defs/${id}` });
const defs = {};
for (const [id, schema] of Object.entries(out.schemas)) { const { $id, $schema, ...rest } = schema; defs[id] = rest; }
const sorted = Object.fromEntries(Object.keys(defs).sort().map((k) => [k, defs[k]]));
console.log(JSON.stringify({ $schema: "https://json-schema.org/draft/2020-12/schema", title: `@ag-ui/core@${version}`, $defs: sorted }, null, 2));
