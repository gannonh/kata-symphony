import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["src/tests/**/*.vitest.test.ts"],
    exclude: ["dist/**"],
    // I/O-heavy golden-path and skill-bundle tests flake at the 5s default when
    // validate runs the full --affected graph in parallel on GitHub runners.
    testTimeout: 20_000,
  },
});
