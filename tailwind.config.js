/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        bg: "#F6F5F2",
        surface: "#FFFFFF",
        "surface-2": "#FBFBFA",
        elevate: "#EFEEEA",
        line: "rgba(20,20,25,0.08)",
        "line-strong": "rgba(20,20,25,0.14)",
        ink: "#1B1B1E",
        "ink-2": "#6E6E73",
        "ink-3": "#9A9A9F",
        accent: "#C4453F",
        "accent-hover": "#AE3A35",
        "accent-soft": "rgba(196,69,63,0.10)",
        warn: "#B0742F",
        "warn-soft": "rgba(176,116,47,0.12)",
      },
      fontFamily: {
        sans: ["Inter", "Noto Sans JP", "system-ui", "sans-serif"],
      },
      fontSize: {
        "time-lg": ["56px", { lineHeight: "1", letterSpacing: "0", fontWeight: "700" }],
        "time-md": ["30px", { lineHeight: "1", letterSpacing: "0", fontWeight: "700" }],
        h1: ["23px", { lineHeight: "1.25", letterSpacing: "0", fontWeight: "600" }],
        title: ["15px", { lineHeight: "1.4", fontWeight: "600" }],
        body: ["14px", { lineHeight: "1.6" }],
        meta: ["12.5px", { lineHeight: "1.5" }],
        micro: ["11px", { lineHeight: "1.4", letterSpacing: "0" }],
      },
      borderRadius: {
        chip: "6px",
        btn: "7px",
        card: "8px",
        panel: "12px",
      },
      spacing: {
        sidebar: "220px",
      },
      boxShadow: {
        card: "0 1px 2px rgba(20,20,25,0.05)",
        pop: "0 24px 60px -28px rgba(20,20,25,0.28)",
        accent: "0 1px 2px rgba(196,69,63,0.35)",
      },
      keyframes: {
        "rec-pulse": {
          "0%,100%": { opacity: "1" },
          "50%": { opacity: ".35" },
        },
        "seg-in": {
          from: { opacity: "0", transform: "translateY(4px)" },
          to: { opacity: "1", transform: "none" },
        },
      },
      animation: {
        "rec-pulse": "rec-pulse 1.6s ease-in-out infinite",
        "seg-in": "seg-in .28s ease-out",
      },
    },
  },
  plugins: [],
};
