// Web app manifest icons; every `src` is a file in `public/` (checked by src/pwa-icons.test.ts).
export const pwaIcons = [
  { src: "/icon-192.png", sizes: "192x192", type: "image/png", purpose: "any" },
  { src: "/icon-512.png", sizes: "512x512", type: "image/png", purpose: "any" },
  { src: "/icon-maskable-512.png", sizes: "512x512", type: "image/png", purpose: "maskable" },
  { src: "/icon.svg", sizes: "any", type: "image/svg+xml", purpose: "any" },
];
