import js from "@eslint/js";
import tsParser from "@typescript-eslint/parser";

export default [
  {
    ignores: [
      "**/dist/**",
      "**/node_modules/**",
      "**/src-tauri/**",
      "**/target/**",
      "target/**",
      "crates/**",
      "third_party/**",
      "scratch/**",
    ],
  },
  js.configs.recommended,
  {
    files: ["**/*.ts", "**/*.tsx"],
    languageOptions: {
      parser: tsParser,
      ecmaVersion: "latest",
      sourceType: "module",
      parserOptions: {
        ecmaFeatures: { jsx: true },
      },
    },
    rules: {
      "no-unused-vars": "off",
      // The TypeScript compiler already reports undefined names (and knows the DOM
      // lib globals this flat config does not declare), so no-undef adds only noise.
      "no-undef": "off",
    },
  },
];
