import assert from "node:assert/strict";
import { test } from "node:test";
import { key, setProfile } from "../src/profile.ts";

test("storage keys keep existing settings separate from named profiles", () => {
    const name = "beryl.audio";
    assert.equal(key(name), name);
    setProfile("host");
    assert.equal(key(name), "beryl.profile:host:beryl.audio");
    setProfile("guest");
    assert.equal(key(name), "beryl.profile:guest:beryl.audio");
    setProfile(null);
    assert.equal(key(name), name);
});
