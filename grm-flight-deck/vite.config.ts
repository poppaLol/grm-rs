import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const gatewayTarget = process.env.GRM_FLIGHT_DECK_DEV_PROXY_TARGET ?? "http://127.0.0.1:3001";

export default defineConfig({
  plugins: [react()],
  server: {
    host: "127.0.0.1",
    port: 8081,
    strictPort: false,
    proxy: {
      "/api": {
        target: gatewayTarget,
        changeOrigin: true
      }
    }
  }
});
