/// <reference types="vitest/config" />
import { fileURLToPath } from "node:url";

import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import { VitePWA } from "vite-plugin-pwa";

const src = fileURLToPath(new URL("./src", import.meta.url));
const nodeModule = (path: string) => fileURLToPath(new URL(`./node_modules/${path}`, import.meta.url));

// The wasm of Automerge is loaded explicitly (`src/sync/wasm.ts`) in the page
// and in the worker, so every import resolves to the slim builds that do not
// try to instantiate it on import.
const automergeSlim = [
  { find: /^@automerge\/automerge$/, replacement: nodeModule("@automerge/automerge/dist/mjs/entrypoints/slim.js") },
  { find: /^@automerge\/automerge-repo$/, replacement: nodeModule("@automerge/automerge-repo/dist/entrypoints/slim.js") },
];

// `TT_SERVER` points the dev server's proxy at a running tt-server.
const server = process.env.TT_SERVER ?? "http://127.0.0.1:8080";

export default defineConfig(({ mode }) => ({
  plugins: [
    react(),
    tailwindcss(),
    VitePWA({
      registerType: "autoUpdate",
      injectRegister: false,
      includeAssets: ["icon.svg", "icon-192.png", "icon-512.png"],
      manifest: {
        name: "tt",
        short_name: "tt",
        description: "Offline-first time tracking",
        start_url: "/",
        display: "standalone",
        background_color: "#1e1e2e",
        theme_color: "#181825",
        icons: [
          { src: "/icon-192.png", sizes: "192x192", type: "image/png" },
          { src: "/icon-512.png", sizes: "512x512", type: "image/png" },
          { src: "/icon.svg", sizes: "any", type: "image/svg+xml", purpose: "any" },
        ],
      },
      workbox: {
        globPatterns: ["**/*.{js,css,html,svg,png,wasm,woff2}"],
        // The Automerge wasm is ~3 MB.
        maximumFileSizeToCacheInBytes: 12 * 1024 * 1024,
        navigateFallback: "/index.html",
        navigateFallbackDenylist: [/^\/api\//, /^\/sync/],
        runtimeCaching: [
          {
            // Only this origin's API: calls to other member servers (failover)
            // go straight to the network.
            urlPattern: ({ url, sameOrigin }) => sameOrigin && url.pathname.startsWith("/api/"),
            handler: "NetworkFirst",
            options: { cacheName: "tt-api", networkTimeoutSeconds: 5 },
          },
        ],
      },
      devOptions: { enabled: mode === "pwa-dev", type: "module" },
    }),
  ],
  resolve: {
    alias: [...automergeSlim, { find: "@", replacement: src }],
  },
  worker: {
    format: "es",
  },
  build: {
    target: "es2022",
    sourcemap: true,
    // One app chunk (~0.8 MB) plus the Automerge wasm; both are precached.
    chunkSizeWarningLimit: 1200,
  },
  server: {
    proxy: {
      "/api": { target: server, changeOrigin: true },
      // tt-server refuses websocket upgrades whose `Origin` is neither a
      // member's client URL nor its own host; the dev server's origin is
      // neither, so the proxy drops the header (a missing `Origin` is allowed).
      "/sync": {
        target: server.replace(/^http/, "ws"),
        ws: true,
        changeOrigin: true,
        configure: (proxy) => proxy.on("proxyReqWs", (request) => request.removeHeader("origin")),
      },
    },
  },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
    setupFiles: ["src/test/setup.ts"],
    env: { TZ: "UTC" },
    css: false,
  },
}));
