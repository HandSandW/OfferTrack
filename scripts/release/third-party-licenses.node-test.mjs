import assert from "node:assert/strict";
import test from "node:test";
import {
  flattenPnpmLicenseReport,
  resolvedCargoPackageIds,
  validateMissingLicensePolicy,
} from "./third-party-licenses.mjs";

test("Cargo production traversal includes normal and build edges but not dev-only edges", () => {
  const metadata = {
    resolve: {
      root: "root",
      nodes: [
        {
          id: "root",
          deps: [
            { pkg: "normal", dep_kinds: [{ kind: null }] },
            { pkg: "build", dep_kinds: [{ kind: "build" }] },
            { pkg: "dev", dep_kinds: [{ kind: "dev" }] },
          ],
        },
        { id: "normal", deps: [] },
        { id: "build", deps: [] },
        { id: "dev", deps: [] },
      ],
    },
  };
  assert.deepEqual([...resolvedCargoPackageIds(metadata)].sort(), [
    "build",
    "normal",
  ]);
});

test("pnpm license groups flatten to deterministic production packages", () => {
  assert.deepEqual(
    flattenPnpmLicenseReport({
      MIT: [
        {
          name: "react",
          versions: ["19.2.8"],
          paths: ["C:/cache/react"],
          license: "MIT",
          homepage: "https://react.dev/",
        },
      ],
    }),
    [
      {
        ecosystem: "pnpm",
        name: "react",
        version: "19.2.8",
        license: "MIT",
        sourceUrl: "https://react.dev/",
        packageDirectory: "C:/cache/react",
      },
    ],
  );
});

test("missing Cargo license files require an exact reviewed override set", () => {
  validateMissingLicensePolicy(["missing@1.0.0"], {
    formatVersion: 1,
    cargoMissingLicenseFiles: { "missing@1.0.0": {} },
  });
  assert.throws(
    () =>
      validateMissingLicensePolicy(["new@2.0.0"], {
        formatVersion: 1,
        cargoMissingLicenseFiles: { "missing@1.0.0": {} },
      }),
    /override set changed/,
  );
});
