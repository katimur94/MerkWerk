import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// MerkWerk-App (Tauri-2-Frontend). Konfiguration folgt dem Standardmuster
// für Tauri + Vite: fester Dev-Server-Port, kein Browser-Autoopen (Tauri
// steuert das Fenster selbst), src-tauri/ vom Watcher ausgenommen.
// https://v2.tauri.app/start/frontend/vite/
export default defineConfig({
  plugins: [react()],

  // Tauris eigener Dev-Prozess soll die Konsolenausgabe kontrollieren.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // Änderungen an src-tauri/ sollen den Vite-Dev-Server nicht neu laden;
      // `tauri dev` baut das Rust-Backend separat neu.
      ignored: ["**/src-tauri/**"],
    },
  },
});
