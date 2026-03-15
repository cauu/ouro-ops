/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  theme: {
    extend: {
      boxShadow: {
        app: "0 18px 42px rgba(15,23,42,0.06)",
        card: "0 4px 18px rgba(15,23,42,0.06)",
      },
    },
  },
  plugins: [],
};
