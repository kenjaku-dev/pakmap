// Vitest — unit tests only (src); Playwright owns e2e/*.spec.ts.
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  // The react plugin is required for the TSX component tests (jsdom pragma
  // per file); pure logic tests are unaffected.
  plugins: [react()],
  test: {
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
  },
});
