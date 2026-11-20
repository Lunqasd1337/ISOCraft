/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  darkMode: "class",
  theme: {
    extend: {
      colors: {
        canvas: "rgb(var(--bg-canvas-rgb) / <alpha-value>)",
        sidebar: "rgb(var(--bg-sidebar-rgb) / <alpha-value>)",
        surface: "rgb(var(--bg-surface-rgb) / <alpha-value>)",
        subtle: "var(--bg-subtle)",
        "border-subtle": "var(--border-subtle)",
        "text-primary": "rgb(var(--text-primary-rgb) / <alpha-value>)",
        "text-secondary": "rgb(var(--text-secondary-rgb) / <alpha-value>)",
        accent: "rgb(var(--accent-rgb) / <alpha-value>)",
        "accent-hover": "rgb(var(--accent-hover-rgb) / <alpha-value>)",
        "accent-fg": "rgb(var(--accent-fg-rgb) / <alpha-value>)",
        "status-success": "rgb(var(--status-success-rgb) / <alpha-value>)",
        "status-success-bg": "var(--status-success-bg)",
        "status-warning": "rgb(var(--status-warning-rgb) / <alpha-value>)",
        "status-warning-bg": "var(--status-warning-bg)",
        "status-danger": "rgb(var(--status-danger-rgb) / <alpha-value>)",
        "status-danger-bg": "var(--status-danger-bg)",
        "switch-off-bg": "var(--switch-off-bg)",
        "switch-off-border": "var(--switch-off-border)",
      },
      fontFamily: {
        sans: [
          "'Segoe UI Variable'",
          "'Segoe UI'",
          "system-ui",
          "-apple-system",
          "sans-serif",
        ],
        mono: ["'Cascadia Mono'", "'Consolas'", "monospace"],
      },
    },
  },
  plugins: [],
};
