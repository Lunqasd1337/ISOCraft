import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

const host = (
  globalThis as { process?: { env?: Record<string, string | undefined> } }
).process?.env?.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  plugins: [tailwindcss(), react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    rolldownOptions: {
      output: {
        codeSplitting: {
          groups: [
            {
              name: "react-vendor",
              test: /[\\/]node_modules[\\/](react|react-dom|scheduler)[\\/]/,
            },
            {
              name: "i18n-vendor",
              test: /[\\/]node_modules[\\/](i18next|react-i18next)[\\/]/,
            },
            {
              name: "icons-vendor",
              test: /[\\/]node_modules[\\/]lucide-react[\\/]/,
            },
            {
              name: "tauri-vendor",
              test: /[\\/]node_modules[\\/]@tauri-apps[\\/]/,
            },
          ],
        },
      },
    },
  },
}));
