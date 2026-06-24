import type { Config } from "tailwindcss";

export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      fontFamily: {
        mono: ["JetBrains Mono", "Consolas", "monospace"],
        sans: ["Inter", "Segoe UI", "Arial", "sans-serif"]
      },
      colors: {
        carbon: "#111418",
        panel: "#191d23",
        panelHi: "#232a32",
        line: "#3a424d",
        mint: "#2cf5bf",
        amber: "#f3d24f",
        danger: "#ff4b4b",
        timing: "#d7e34d"
      }
    }
  },
  plugins: []
} satisfies Config;
