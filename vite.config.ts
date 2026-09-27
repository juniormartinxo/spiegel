import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// O Tauri espera o servidor de desenvolvimento numa porta fixa.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
});
