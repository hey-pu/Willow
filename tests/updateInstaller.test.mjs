import assert from "node:assert/strict";
import test from "node:test";
import { createUpdateInstaller } from "../src/lib/updateInstaller.ts";

function mockUpdate(overrides = {}) {
  const calls = [];
  return {
    calls,
    version: "0.5.2",
    async download(progress) {
      calls.push("download");
      progress({ event: "Started", data: { contentLength: 100 } });
      progress({ event: "Progress", data: { chunkLength: 35 } });
      progress({ event: "Progress", data: { chunkLength: 65 } });
      progress({ event: "Finished" });
    },
    async install(options) {
      assert.equal(options.restartAfterInstall, true);
      calls.push("install");
    },
    async close() { calls.push("close"); },
    ...overrides,
  };
}

test("downloads, verifies, installs and releases the reviewed version", async () => {
  const update = mockUpdate();
  const installer = createUpdateInstaller(async () => update);
  const states = [];
  const unsubscribe = installer.subscribe(() => states.push(installer.getSnapshot()));
  assert.equal(await installer.install("0.5.2"), "installed");
  assert.deepEqual(update.calls, ["download", "install", "close"]);
  assert.deepEqual(states.filter(state => state.phase === "downloading").map(state => state.percent), [null, 0, 35, 100]);
  assert.deepEqual(states.slice(-2).map(state => state.phase), ["verifying", "installing"]);
  unsubscribe();
});

test("rejects an absent or mismatched manifest before downloading", async () => {
  const update = mockUpdate({ version: "0.5.1" });
  const installer = createUpdateInstaller(async () => update);
  assert.equal(await installer.install("0.5.2"), "failed");
  assert.deepEqual(update.calls, ["close"]);
  const missing = createUpdateInstaller(async () => null);
  assert.equal(await missing.install("0.5.2"), "unavailable");
  assert.equal(missing.getSnapshot().phase, "failed");
});

test("signature or network failures block installation and allow retry", async () => {
  const update = mockUpdate({ async download() { throw new Error("Invalid signature"); } });
  const installer = createUpdateInstaller(async () => update);
  assert.equal(await installer.install("0.5.2"), "failed");
  assert.deepEqual(update.calls, ["close"]);
  assert.equal(installer.getSnapshot().phase, "failed");
  update.download = mockUpdate().download.bind(update);
  assert.equal(await installer.install("0.5.2"), "installed");
  assert.deepEqual(update.calls, ["close", "install", "close"]);
});

test("deduplicates simultaneous banner and Settings install actions", async () => {
  let resolveCheck;
  let checkCount = 0;
  const update = mockUpdate();
  const installer = createUpdateInstaller(() => {
    checkCount++;
    return new Promise(resolve => { resolveCheck = resolve; });
  });
  const first = installer.install("0.5.2");
  await installer.install("0.5.2");
  assert.equal(checkCount, 1);
  resolveCheck(update);
  assert.equal(await first, "installed");
  assert.deepEqual(update.calls, ["download", "install", "close"]);
});

test("reports download progress without a Content-Length", async () => {
  const update = mockUpdate({
    async download(progress) {
      progress({ event: "Started", data: {} });
      progress({ event: "Progress", data: { chunkLength: 10 } });
      progress({ event: "Finished" });
    },
  });
  const installer = createUpdateInstaller(async () => update);
  const values = [];
  installer.subscribe(() => {
    const state = installer.getSnapshot();
    if (state.phase === "downloading") values.push(state.percent);
  });
  await installer.install("0.5.2");
  assert.deepEqual(values, [null, null, null]);
});
