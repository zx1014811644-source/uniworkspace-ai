/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  theme: {
    extend: {
      fontFamily: {
        sans: [
          "SF Pro Text",
          "PingFang SC",
          "Helvetica Neue",
          "sans-serif",
        ],
      },
      colors: {
        shell: {
          frame: "#e6e2d9",
          bar: "#ddd8ce",
          side: "#f3f0e8",
          canvas: "#f7f5f0",
          ink: "#1d1c19",
          muted: "#6f6a60",
          line: "#d4cfc4",
          accent: "#1f6b4a",
        },
      },
    },
  },
  plugins: [],
};
