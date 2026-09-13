import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const productionFieldOperator = "tn_01j91h09tpj5ehwbwfwfxpak2b";

test("production delegates Field authorization through the Field directory root", async () => {
  const [manifest, example] = await Promise.all([
    readFile(new URL("../tachyon.yml", import.meta.url), "utf8"),
    readFile(new URL("../.env.example", import.meta.url), "utf8"),
  ]);

  assert.match(
    manifest,
    new RegExp(`name: FIELD_OPERATOR_ID\\n(?:[ \\t]*#.*\\n)*[ \\t]*value: ${productionFieldOperator}`),
  );
  assert.match(example, new RegExp(`^FIELD_OPERATOR_ID=${productionFieldOperator}$`, "m"));
  assert.doesNotMatch(
    manifest,
    /name: FIELD_OPERATOR_ID\n(?:[ \t]*#.*\n)*[ \t]*value: tn_01m2a6ztfmd95p70gwgyshbevq/,
  );
});
