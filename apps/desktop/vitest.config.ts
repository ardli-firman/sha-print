import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// Unit tests run against jsdom; the shell itself is loaded by Tauri, so tests mock the IPC module.
export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
    restoreMocks: true,
  },
});
